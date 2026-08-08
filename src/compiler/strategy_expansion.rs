//! Compile-time strategy expansion.
//!
//! Strategy lowering (`Strategy::lower` → `PrimitiveGraph` →
//! `to_execution_graph` → `ExecutionSubgraph`) happens here, inside the
//! compiler, and the resulting subgraph is attached to the node as
//! `ExecutionNode.subgraph`. The executor executes the prebuilt subgraph
//! directly and no longer lowers strategies on the live path.
//!
//! The lowering is pure and deterministic: `to_execution_graph` derives node
//! UUIDs from the primitive-graph hash, so identical inputs produce identical
//! subgraphs (compiler determinism invariant preserved).

use std::sync::Arc;
use std::sync::OnceLock;

use crate::compiler::context::CompilationContext;
use crate::compiler::ir::strategy_ir::{DebateRole, StrategyIR};
use crate::compiler::registry::StrategyRegistry;
use crate::strategies::chain::ChainStrategy;
use crate::strategies::consensus::ConsensusStrategy;
use crate::strategies::fusion::FusionStrategy;
use crate::strategies::react::ReActStrategy;
use crate::strategies::reflection::ReflectionStrategy;
use crate::strategies::single::SingleStrategy;
use crate::types::{ExecutionNode, ExecutionSubgraph, StrategyKind};

/// Maps a `StrategyKind` to its registry key (lowercased descriptor name).
pub(crate) fn strategy_name(kind: &StrategyKind) -> &str {
    match kind {
        StrategyKind::Single => "single",
        StrategyKind::Consensus => "consensus",
        StrategyKind::Debate => "debate",
        StrategyKind::Reflection => "reflection",
        StrategyKind::ReAct => "react",
        StrategyKind::Chain => "chain",
        StrategyKind::Fusion => "fusion",
        StrategyKind::Custom(name) => name.as_str(),
    }
}

/// Registry of the built-in strategies, shared across compilations.
fn default_strategy_registry() -> &'static StrategyRegistry {
    static REGISTRY: OnceLock<StrategyRegistry> = OnceLock::new();
    REGISTRY.get_or_init(|| {
        let mut registry = StrategyRegistry::new();
        registry.register(Arc::new(SingleStrategy));
        registry.register(Arc::new(ConsensusStrategy::default()));
        registry.register(Arc::new(crate::strategies::debate::DebateStrategy {
            debaters: vec![Box::new(SingleStrategy), Box::new(SingleStrategy)],
            judge: Box::new(SingleStrategy),
        }));
        registry.register(Arc::new(ReflectionStrategy::default()));
        registry.register(Arc::new(ReActStrategy::default()));
        registry.register(Arc::new(ChainStrategy {
            stages: vec![Box::new(SingleStrategy)],
        }));
        registry.register(Arc::new(FusionStrategy::new(vec![Box::new(SingleStrategy)])));
        registry
    })
}

/// Rebuilds the `StrategyIR` declared by a node (config-driven parameters).
pub(crate) fn strategy_ir_from_node(node: &ExecutionNode) -> StrategyIR {
    match node.strategy {
        StrategyKind::Single => StrategyIR::Single,
        StrategyKind::Consensus => StrategyIR::Consensus {
            count: node.config.get("count").and_then(|v| v.as_u64()).unwrap_or(3) as u32,
            members: node
                .config
                .get("members")
                .and_then(|v| {
                    v.as_array().map(|items| {
                        items
                            .iter()
                            .filter_map(|item| item.as_str().map(|s| s.to_string()))
                            .collect()
                    })
                })
                .unwrap_or_default(),
        },
        StrategyKind::Debate => StrategyIR::Debate {
            roles: node
                .config
                .get("roles")
                .and_then(|v| {
                    v.as_array().map(|items| {
                        items
                            .iter()
                            .map(|item| {
                                serde_json::from_value::<DebateRole>(item.clone()).or_else(|_| {
                                    item.as_str().map(|s| DebateRole {
                                        name: s.to_string(),
                                        model: node.model.clone(),
                                        stance: s.to_string(),
                                    })
                                    .ok_or(serde_json::Error::io(std::io::Error::new(
                                        std::io::ErrorKind::InvalidData,
                                        "roles must be role objects or strings",
                                    )))
                                })
                            })
                            .filter_map(Result::ok)
                            .collect()
                    })
                })
                .unwrap_or_default(),
        },
        StrategyKind::Reflection => StrategyIR::Reflection {
            max_cycles: node.config.get("max_reflection_cycles")
                .and_then(|v| v.as_u64()).unwrap_or(3) as u32,
        },
        StrategyKind::ReAct => StrategyIR::ReAct {
            max_iterations: node.config.get("max_iterations")
                .and_then(|v| v.as_u64()).unwrap_or(10) as u32,
        },
        StrategyKind::Chain => StrategyIR::Chain {
            stages: node.config.get("stages")
                .and_then(|v| serde_json::from_value(v.clone()).ok())
                .unwrap_or_default(),
        },
        StrategyKind::Fusion => StrategyIR::Custom {
            name: "fusion".into(),
            config: serde_json::json!({}),
        },
        StrategyKind::Custom(ref name) => StrategyIR::Custom {
            name: name.clone(),
            config: node.config.get("config").cloned().unwrap_or(serde_json::json!({})),
        },
    }
}

fn execution_graph_to_subgraph(eg: &crate::types::ExecutionGraph, template: &ExecutionNode) -> ExecutionSubgraph {
    let entry_id = eg.nodes.first().map(|n| n.id).unwrap_or(template.id);
    let exit_id = eg.nodes.last().map(|n| n.id).unwrap_or(template.id);

    ExecutionSubgraph {
        nodes: eg.nodes.clone(),
        edges: eg.edges.clone(),
        entry_node_id: entry_id,
        exit_node_id: exit_id,
    }
}

/// Lowers the node's strategy into a prebuilt `ExecutionSubgraph`, if any.
///
/// Returns `None` for passthrough nodes (Single strategy), unregistered
/// strategy kinds (e.g. Custom WASM strategies not available at compile
/// time), and when lowering fails — the node then executes as itself.
pub(crate) fn expanded_subgraph(node: &ExecutionNode) -> Option<ExecutionSubgraph> {
    if node.strategy == StrategyKind::Single {
        return None;
    }

    let registry = default_strategy_registry();
    let strategy = match registry.get(strategy_name(&node.strategy)) {
        Ok(s) => s,
        Err(e) => {
            tracing::warn!(
                node_id = %node.id,
                strategy = ?node.strategy,
                error = %e,
                "strategy not registered at compile time; using passthrough"
            );
            return None;
        }
    };

    let mut ctx = CompilationContext::new();
    if !node.model.is_empty() {
        ctx.available_models.push(node.model.clone());
    }

    let ir = strategy_ir_from_node(node);
    match strategy.lower(&ir, &ctx) {
        Ok(pg) => {
            let eg = pg.to_execution_graph(
                node.strategy.clone(),
                &node.retry_policy,
                &node.fallback,
                &node.config,
            );
            Some(execution_graph_to_subgraph(&eg, node))
        }
        Err(e) => {
            tracing::warn!(
                node_id = %node.id,
                strategy = ?node.strategy,
                error = %e,
                "strategy lowering failed at compile time; using passthrough"
            );
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{ExecutionNodeKind, RetryPolicy};
    use std::collections::HashMap;
    use uuid::Uuid;

    fn make_node(strategy: StrategyKind) -> ExecutionNode {
        ExecutionNode {
            id: Uuid::new_v4(),
            kind: ExecutionNodeKind::LLMGenerate,
            strategy,
            model: "m".into(),
            retry_policy: RetryPolicy { max_retries: 0, backoff_ms: 0 },
            fallback: None,
            config: HashMap::new(),
            subgraph: None,
        }
    }

    #[test]
    fn registry_contains_all_builtin_strategies() {
        assert_eq!(default_strategy_registry().strategy_count(), 7);
    }

    #[test]
    fn single_strategy_has_no_subgraph() {
        let node = make_node(StrategyKind::Single);
        assert!(expanded_subgraph(&node).is_none());
    }

    #[test]
    fn consensus_expands_to_subgraph_at_compile_time() {
        let node = make_node(StrategyKind::Consensus);
        let subgraph = expanded_subgraph(&node).expect("consensus expands");
        assert_eq!(subgraph.nodes.len(), 4);
        assert_eq!(subgraph.nodes[0].kind, ExecutionNodeKind::LLMGenerate);
        assert_eq!(subgraph.nodes.last().unwrap().kind, ExecutionNodeKind::LLMJudge);
        assert!(subgraph.nodes.iter().all(|n| n.subgraph.is_none()));
    }

    #[test]
    fn debate_expands_to_subgraph() {
        let mut node = make_node(StrategyKind::Debate);
        node.config.insert(
            "roles".into(),
            serde_json::json!(["Defender", "Critic"]),
        );
        let subgraph = expanded_subgraph(&node).expect("debate expands");
        assert_eq!(subgraph.nodes.len(), 3);
        assert!(subgraph.nodes.iter().all(|n| n.subgraph.is_none()));
    }

    #[test]
    fn expansion_is_deterministic() {
        let a = make_node(StrategyKind::Consensus);
        let b = make_node(StrategyKind::Consensus);
        let sa = expanded_subgraph(&a).unwrap();
        let sb = expanded_subgraph(&b).unwrap();
        assert_eq!(sa.nodes.len(), sb.nodes.len());
        assert_eq!(sa.nodes[0].id, sb.nodes[0].id);
    }

    #[test]
    fn consensus_node_members_flow_into_ir() {
        let mut node = make_node(StrategyKind::Consensus);
        node.config.insert("count".into(), serde_json::json!(3));
        node.config.insert(
            "members".into(),
            serde_json::json!(["zen/model-a", "openrouter/model-b", "openrouter/model-c"]),
        );
        let ir = strategy_ir_from_node(&node);
        match ir {
            StrategyIR::Consensus { count, members } => {
                assert_eq!(count, 3);
                assert_eq!(members, vec!["zen/model-a", "openrouter/model-b", "openrouter/model-c"]);
            }
            _ => panic!("expected Consensus IR"),
        }
    }

    #[test]
    fn consensus_without_members_defaults_to_empty_ir() {
        let node = make_node(StrategyKind::Consensus);
        let ir = strategy_ir_from_node(&node);
        match ir {
            StrategyIR::Consensus { count, members } => {
                assert_eq!(count, 3);
                assert!(members.is_empty());
            }
            _ => panic!("expected Consensus IR"),
        }
    }

    #[test]
    fn consensus_expansion_carries_per_member_models() {
        let mut node = make_node(StrategyKind::Consensus);
        node.config.insert(
            "members".into(),
            serde_json::json!(["zen/model-a", "openrouter/model-b", "openrouter/model-c"]),
        );
        let subgraph = expanded_subgraph(&node).expect("consensus expands");
        let models: Vec<String> = subgraph
            .nodes
            .iter()
            .filter(|n| n.kind == ExecutionNodeKind::LLMGenerate)
            .map(|n| n.model.clone())
            .collect();
        assert_eq!(models, vec!["zen/model-a", "openrouter/model-b", "openrouter/model-c"]);
    }
}
