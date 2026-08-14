//! Strategy IR construction from execution nodes.

use crate::compiler::ir::StrategyIR;
use crate::types::{ExecutionNode, StrategyKind};

pub fn strategy_ir_from_node(node: &ExecutionNode) -> StrategyIR {
    match &node.strategy {
        StrategyKind::Single => StrategyIR::Single,
        StrategyKind::Consensus => {
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
        StrategyKind::Reflection => {
            let max_cycles = node
                .config
                .get("max_cycles")
                .and_then(|v| v.as_u64())
                .unwrap_or(3) as u32;
            StrategyIR::Reflection { max_cycles }
        }
        StrategyKind::Chain => {
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
        StrategyKind::Debate => {
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
        StrategyKind::ReAct => {
            let max_iterations = node
                .config
                .get("max_tool_rounds")
                .and_then(|v| v.as_u64())
                .unwrap_or(10) as u32;
            StrategyIR::ReAct { max_iterations }
        }
        StrategyKind::Fusion => {
            StrategyIR::Chain {
                stages: vec![StrategyIR::Single, StrategyIR::Consensus { count: 3, members: vec![] }],
            }
        }
        StrategyKind::Custom(name) => {
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
