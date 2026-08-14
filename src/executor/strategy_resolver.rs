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

/// Converts an `ExecutionNode` into a `StrategyIR` for runtime strategy lowering.
/// This is the inlined version of the former `crate::compiler::strategy_expansion::strategy_ir_from_node`.
pub(crate) fn strategy_ir_from_node(node: &ExecutionNode) -> StrategyIR {
    match &node.strategy {
        crate::types::StrategyKind::Single => StrategyIR::Single,
        crate::types::StrategyKind::Consensus => {
            let count = node
                .config
                .get("count")
                .and_then(|v| v.as_u64())
                .unwrap_or(3) as u32;
            let members = node
                .config
                .get("members")
                .and_then(|v| v.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|v| v.as_str().map(|s| s.to_string()))
                        .collect()
                })
                .unwrap_or_default();
            StrategyIR::Consensus { count, members }
        }
        crate::types::StrategyKind::Reflection => {
            let max_cycles = node
                .config
                .get("max_cycles")
                .and_then(|v| v.as_u64())
                .unwrap_or(3) as u32;
            StrategyIR::Reflection { max_cycles }
        }
        crate::types::StrategyKind::Chain => {
            let stages = node
                .config
                .get("stages")
                .and_then(|v| v.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|v| {
                            v.as_str().and_then(|s| match s {
                                "Single" => Some(StrategyIR::Single),
                                "Reflection" => Some(StrategyIR::Reflection { max_cycles: 3 }),
                                "Consensus" => Some(StrategyIR::Consensus {
                                    count: 3,
                                    members: vec![],
                                }),
                                _ => None,
                            })
                        })
                        .collect()
                })
                .unwrap_or_else(|| vec![StrategyIR::Single]);
            StrategyIR::Chain { stages }
        }
        crate::types::StrategyKind::Debate => {
            let roles = node
                .config
                .get("roles")
                .and_then(|v| v.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|v| {
                            if let (Some(name), Some(model), Some(stance)) = (
                                v.get("name").and_then(|n| n.as_str()),
                                v.get("model").and_then(|m| m.as_str()),
                                v.get("stance").and_then(|s| s.as_str()),
                            ) {
                                return Some(crate::compiler::ir::DebateRole {
                                    name: name.to_string(),
                                    model: model.to_string(),
                                    stance: stance.to_string(),
                                });
                            }
                            v.as_str().map(|s| crate::compiler::ir::DebateRole {
                                name: s.to_string(),
                                model: node.model.clone(),
                                stance: s.to_string(),
                            })
                        })
                        .collect()
                })
                .unwrap_or_default();
            StrategyIR::Debate { roles }
        }
        crate::types::StrategyKind::ReAct => {
            let max_iterations = node
                .config
                .get("max_tool_rounds")
                .and_then(|v| v.as_u64())
                .unwrap_or(10) as u32;
            StrategyIR::ReAct { max_iterations }
        }
        crate::types::StrategyKind::Fusion => StrategyIR::Chain {
            stages: vec![
                StrategyIR::Single,
                StrategyIR::Consensus {
                    count: 3,
                    members: vec![],
                },
            ],
        },
        crate::types::StrategyKind::Custom(name) => {
            let config = node
                .config
                .get("custom_config")
                .cloned()
                .unwrap_or(serde_json::Value::Null);
            StrategyIR::Custom {
                name: name.clone(),
                config,
            }
        }
    }
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
