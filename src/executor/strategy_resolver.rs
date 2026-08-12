use tracing::info;

use crate::compiler::context::CompilationContext;
use crate::compiler::ir::StrategyIR;
use crate::executor::DefaultExecutor;
use crate::types::{ExecutionGraph, ExecutionNode, ExecutionNodeKind, ExecutionSubgraph};

impl DefaultExecutor {
    /// Belt-and-suspenders: strategy sub-nodes built at compile time never
    /// carry the request's assembled messages (the pipeline only injects them
    /// into top-level nodes). Copy the parent node's messages (and the
    /// per-request tool allowlist, when present) into any LLM sub-node that
    /// lacks them so requests never go out with an empty `messages` array and
    /// tool definitions remain available for sub-node dispatch.
    pub(crate) fn propagate_parent_messages(
        node: &ExecutionNode,
        subgraph: &mut ExecutionSubgraph,
    ) {
        let Some(messages) = node
            .config
            .get("messages")
            .filter(|v| v.as_array().map(|a| !a.is_empty()).unwrap_or(false))
            .cloned()
        else {
            return;
        };
        let tool_allowlist = node.config.get("tool_allowlist").cloned();
        for sub_node in &mut subgraph.nodes {
            if !matches!(
                sub_node.kind,
                ExecutionNodeKind::LLMGenerate
                    | ExecutionNodeKind::LLMReview
                    | ExecutionNodeKind::LLMJudge
            ) {
                continue;
            }
            let has_messages = sub_node
                .config
                .get("messages")
                .and_then(|v| v.as_array())
                .map(|a| !a.is_empty())
                .unwrap_or(false);
            if !has_messages {
                sub_node
                    .config
                    .insert("messages".to_string(), messages.clone());
            }
            if sub_node.config.get("tool_allowlist").is_none() {
                if let Some(ref allowlist) = tool_allowlist {
                    sub_node
                        .config
                        .insert("tool_allowlist".to_string(), allowlist.clone());
                }
            }
        }
    }

    #[tracing::instrument(skip(self, node), fields(node_id = %node.id, strategy = ?node.strategy))]
    pub(crate) async fn resolve_strategy_impl(&self, node: &ExecutionNode) -> ExecutionSubgraph {
        if let Some(prebuilt) = &node.subgraph {
            let mut subgraph = prebuilt.clone();
            Self::propagate_parent_messages(node, &mut subgraph);
            return subgraph;
        }

        let strategy = self.strategies.get(&node.strategy);
        if let Some(s) = strategy {
            let mut ctx = CompilationContext::new();
            if !node.model.is_empty() {
                ctx.available_models.push(node.model.clone());
            }
            let ir = strategy_ir_from_node(node);
            match s.lower(&ir, &ctx) {
                Ok(pg) => {
                    let eg = pg.to_execution_graph(
                        node.strategy.clone(),
                        &node.retry_policy,
                        &node.fallback,
                        &node.config,
                    );
                    let mut subgraph = execution_graph_to_subgraph(eg, node);
                    Self::propagate_parent_messages(node, &mut subgraph);
                    return subgraph;
                }
                Err(e) => {
                    tracing::warn!(
                        node_id = %node.id,
                        strategy = ?node.strategy,
                        error = %e,
                        "strategy lowering failed, falling back to passthrough"
                    );
                }
            }
            ExecutionSubgraph {
                nodes: vec![node.clone()],
                edges: vec![],
                entry_node_id: node.id,
                exit_node_id: node.id,
            }
        } else {
            info!(
                node_id = %node.id,
                strategy = ?node.strategy,
                "No strategy registered, using passthrough"
            );
            ExecutionSubgraph {
                nodes: vec![node.clone()],
                edges: vec![],
                entry_node_id: node.id,
                exit_node_id: node.id,
            }
        }
    }
}

pub(crate) fn strategy_ir_from_node(node: &ExecutionNode) -> StrategyIR {
    crate::compiler::strategy_expansion::strategy_ir_from_node(node)
}

pub(crate) fn execution_graph_to_subgraph(
    mut eg: ExecutionGraph,
    template: &ExecutionNode,
) -> ExecutionSubgraph {
    let entry_id = eg.nodes.first().map(|n| n.id).unwrap_or(template.id);
    let exit_id = eg.nodes.last().map(|n| n.id).unwrap_or(template.id);

    ExecutionSubgraph {
        nodes: std::mem::take(&mut eg.nodes),
        edges: std::mem::take(&mut eg.edges),
        entry_node_id: entry_id,
        exit_node_id: exit_id,
    }
}
