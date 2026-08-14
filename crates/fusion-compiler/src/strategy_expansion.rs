//! Compile-time strategy expansion (Phase 3.5).
//!
//! After `lower_to_graph`, non-`Single` strategy nodes get a prebuilt
//! `ExecutionSubgraph` attached so the Phase 4 runtime subgraph path becomes
//! the production path for all strategies without pulling legacy host code.
//!
//! Pure and deterministic: child UUIDs are derived with `Uuid::new_v5` from the
//! parent node id, role, and index, ensuring identical output for identical inputs.

use fusion_types::*;

/// Returns the prebuilt subgraph for a strategy node, or `None` for Single.
pub fn expanded_subgraph(node: &ExecutionNode) -> Option<ExecutionSubgraph> {
    expanded_subgraph_with_custom(node, None)
}

/// Returns the prebuilt subgraph for a strategy node, allowing custom strategy compiler delegate.
pub fn expanded_subgraph_with_custom(
    node: &ExecutionNode,
    custom_compiler: Option<&dyn crate::strategy_compiler::StrategyCompiler>,
) -> Option<ExecutionSubgraph> {
    match &node.strategy {
        StrategyKind::Single => None,
        StrategyKind::Consensus => Some(expand_consensus(node)),
        StrategyKind::Reflection => Some(expand_reflection(node)),
        StrategyKind::Chain => Some(expand_chain(node)),
        StrategyKind::Debate => Some(expand_debate(node)),
        StrategyKind::ReAct => Some(expand_react(node)),
        StrategyKind::Fusion => Some(expand_fusion(node)),
        StrategyKind::Custom(custom_name) => {
            if let Some(compiler) = custom_compiler {
                Some(compiler.compile_subgraph(node, custom_name))
            } else {
                Some(expand_custom(node, custom_name))
            }
        }
    }
}

/// Deterministic child id: `v5(namespace, "{parent}:{role}:{index}")`.
pub fn child_id(parent: uuid::Uuid, role: &str, index: usize) -> uuid::Uuid {
    uuid::Uuid::new_v5(
        &uuid::Uuid::NAMESPACE_URL,
        format!("{}:{}:{}", parent, role, index).as_bytes(),
    )
}

/// Expands a Consensus node into `count` × `LLMGenerate` fan-out feeding one `LLMJudge`.
pub fn expand_consensus(node: &ExecutionNode) -> ExecutionSubgraph {
    let count = node
        .config
        .get("count")
        .and_then(|v| v.as_u64())
        .unwrap_or(3)
        .max(1) as usize;

    let members: Vec<String> = node
        .config
        .get("members")
        .and_then(|v| v.as_array())
        .map(|items| {
            items
                .iter()
                .filter_map(|item| item.as_str().map(|s| s.to_string()))
                .collect()
        })
        .unwrap_or_default();

    let members = if members.is_empty() {
        vec![node.model.clone(); count]
    } else {
        members
    };

    let mut nodes: Vec<ExecutionNode> = members
        .iter()
        .enumerate()
        .map(|(i, model)| ExecutionNode {
            id: child_id(node.id, "gen", i),
            kind: ExecutionNodeKind::LLMGenerate,
            strategy: StrategyKind::Single,
            model: model.clone(),
            retry_policy: node.retry_policy.clone(),
            fallback: node.fallback.clone(),
            config: node.config.clone(),
            subgraph: None,
        })
        .collect();

    let judge = ExecutionNode {
        id: child_id(node.id, "judge", 0),
        kind: ExecutionNodeKind::LLMJudge,
        strategy: StrategyKind::Single,
        model: node.model.clone(),
        retry_policy: node.retry_policy.clone(),
        fallback: node.fallback.clone(),
        config: node.config.clone(),
        subgraph: None,
    };

    let entry_node_id = nodes.first().map(|n| n.id).unwrap_or(judge.id);
    let exit_node_id = judge.id;
    let edges: Vec<ExecutionEdge> = nodes
        .iter()
        .map(|gen| ExecutionEdge {
            from: gen.id,
            to: judge.id,
            condition: None,
        })
        .collect();
    nodes.push(judge);

    ExecutionSubgraph {
        nodes,
        edges,
        entry_node_id,
        exit_node_id,
    }
}

/// Expands Reflection strategy into generator -> reviewer loop subgraph.
pub fn expand_reflection(node: &ExecutionNode) -> ExecutionSubgraph {
    let gen_id = child_id(node.id, "generator", 0);
    let rev_id = child_id(node.id, "reviewer", 0);

    let generator = ExecutionNode {
        id: gen_id,
        kind: ExecutionNodeKind::LLMGenerate,
        strategy: StrategyKind::Single,
        model: node.model.clone(),
        retry_policy: node.retry_policy.clone(),
        fallback: node.fallback.clone(),
        config: node.config.clone(),
        subgraph: None,
    };

    let reviewer = ExecutionNode {
        id: rev_id,
        kind: ExecutionNodeKind::LLMReview,
        strategy: StrategyKind::Single,
        model: node.model.clone(),
        retry_policy: node.retry_policy.clone(),
        fallback: node.fallback.clone(),
        config: node.config.clone(),
        subgraph: None,
    };

    ExecutionSubgraph {
        nodes: vec![generator, reviewer],
        edges: vec![ExecutionEdge {
            from: gen_id,
            to: rev_id,
            condition: None,
        }],
        entry_node_id: gen_id,
        exit_node_id: rev_id,
    }
}

/// Expands Chain strategy into a sequential pipeline.
pub fn expand_chain(node: &ExecutionNode) -> ExecutionSubgraph {
    let steps = node.config.get("steps")
        .and_then(|v| v.as_u64())
        .unwrap_or(2) as usize;

    let mut nodes = Vec::with_capacity(steps);
    let mut edges = Vec::with_capacity(steps.saturating_sub(1));

    for i in 0..steps {
        let nid = child_id(node.id, "step", i);
        nodes.push(ExecutionNode {
            id: nid,
            kind: ExecutionNodeKind::LLMGenerate,
            strategy: StrategyKind::Single,
            model: node.model.clone(),
            retry_policy: node.retry_policy.clone(),
            fallback: node.fallback.clone(),
            config: node.config.clone(),
            subgraph: None,
        });
        if i > 0 {
            edges.push(ExecutionEdge {
                from: nodes[i - 1].id,
                to: nid,
                condition: None,
            });
        }
    }

    let entry_node_id = nodes.first().map(|n| n.id).unwrap_or(node.id);
    let exit_node_id = nodes.last().map(|n| n.id).unwrap_or(node.id);

    ExecutionSubgraph {
        nodes,
        edges,
        entry_node_id,
        exit_node_id,
    }
}

/// Expands Debate strategy into proposer -> opposer -> judge synthesis.
pub fn expand_debate(node: &ExecutionNode) -> ExecutionSubgraph {
    let prop_id = child_id(node.id, "proposer", 0);
    let opp_id = child_id(node.id, "opposer", 0);
    let judge_id = child_id(node.id, "judge", 0);

    let proposer = ExecutionNode {
        id: prop_id,
        kind: ExecutionNodeKind::LLMGenerate,
        strategy: StrategyKind::Single,
        model: node.model.clone(),
        retry_policy: node.retry_policy.clone(),
        fallback: node.fallback.clone(),
        config: node.config.clone(),
        subgraph: None,
    };

    let opposer = ExecutionNode {
        id: opp_id,
        kind: ExecutionNodeKind::LLMGenerate,
        strategy: StrategyKind::Single,
        model: node.model.clone(),
        retry_policy: node.retry_policy.clone(),
        fallback: node.fallback.clone(),
        config: node.config.clone(),
        subgraph: None,
    };

    let judge = ExecutionNode {
        id: judge_id,
        kind: ExecutionNodeKind::LLMJudge,
        strategy: StrategyKind::Single,
        model: node.model.clone(),
        retry_policy: node.retry_policy.clone(),
        fallback: node.fallback.clone(),
        config: node.config.clone(),
        subgraph: None,
    };

    ExecutionSubgraph {
        nodes: vec![proposer, opposer, judge],
        edges: vec![
            ExecutionEdge { from: prop_id, to: opp_id, condition: None },
            ExecutionEdge { from: opp_id, to: judge_id, condition: None },
        ],
        entry_node_id: prop_id,
        exit_node_id: judge_id,
    }
}

/// Expands ReAct strategy into reason -> tool action -> observation.
pub fn expand_react(node: &ExecutionNode) -> ExecutionSubgraph {
    let reason_id = child_id(node.id, "reason", 0);
    let tool_id = child_id(node.id, "tool", 0);

    let reasoner = ExecutionNode {
        id: reason_id,
        kind: ExecutionNodeKind::LLMGenerate,
        strategy: StrategyKind::Single,
        model: node.model.clone(),
        retry_policy: node.retry_policy.clone(),
        fallback: node.fallback.clone(),
        config: node.config.clone(),
        subgraph: None,
    };

    let tool_node = ExecutionNode {
        id: tool_id,
        kind: ExecutionNodeKind::Transform,
        strategy: StrategyKind::Single,
        model: node.model.clone(),
        retry_policy: node.retry_policy.clone(),
        fallback: node.fallback.clone(),
        config: node.config.clone(),
        subgraph: None,
    };

    ExecutionSubgraph {
        nodes: vec![reasoner, tool_node],
        edges: vec![ExecutionEdge { from: reason_id, to: tool_id, condition: None }],
        entry_node_id: reason_id,
        exit_node_id: tool_id,
    }
}

/// Expands Fusion strategy into parallel candidates feeding a merger node.
pub fn expand_fusion(node: &ExecutionNode) -> ExecutionSubgraph {
    let cand1_id = child_id(node.id, "candidate", 0);
    let cand2_id = child_id(node.id, "candidate", 1);
    let merger_id = child_id(node.id, "merger", 0);

    let cand1 = ExecutionNode {
        id: cand1_id,
        kind: ExecutionNodeKind::LLMGenerate,
        strategy: StrategyKind::Single,
        model: node.model.clone(),
        retry_policy: node.retry_policy.clone(),
        fallback: node.fallback.clone(),
        config: node.config.clone(),
        subgraph: None,
    };

    let cand2 = ExecutionNode {
        id: cand2_id,
        kind: ExecutionNodeKind::LLMGenerate,
        strategy: StrategyKind::Single,
        model: node.model.clone(),
        retry_policy: node.retry_policy.clone(),
        fallback: node.fallback.clone(),
        config: node.config.clone(),
        subgraph: None,
    };

    let merger = ExecutionNode {
        id: merger_id,
        kind: ExecutionNodeKind::LLMJudge,
        strategy: StrategyKind::Single,
        model: node.model.clone(),
        retry_policy: node.retry_policy.clone(),
        fallback: node.fallback.clone(),
        config: node.config.clone(),
        subgraph: None,
    };

    ExecutionSubgraph {
        nodes: vec![cand1, cand2, merger],
        edges: vec![
            ExecutionEdge { from: cand1_id, to: merger_id, condition: None },
            ExecutionEdge { from: cand2_id, to: merger_id, condition: None },
        ],
        entry_node_id: cand1_id,
        exit_node_id: merger_id,
    }
}

/// Expands Custom strategy into a delegate node.
pub fn expand_custom(node: &ExecutionNode, custom_name: &str) -> ExecutionSubgraph {
    let custom_id = child_id(node.id, custom_name, 0);
    let delegate = ExecutionNode {
        id: custom_id,
        kind: ExecutionNodeKind::LLMGenerate,
        strategy: StrategyKind::Single,
        model: node.model.clone(),
        retry_policy: node.retry_policy.clone(),
        fallback: node.fallback.clone(),
        config: node.config.clone(),
        subgraph: None,
    };

    ExecutionSubgraph {
        nodes: vec![delegate],
        edges: vec![],
        entry_node_id: custom_id,
        exit_node_id: custom_id,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn make_node(strategy: StrategyKind) -> ExecutionNode {
        ExecutionNode {
            id: uuid::Uuid::new_v4(),
            kind: ExecutionNodeKind::LLMGenerate,
            strategy,
            model: "parent-model".into(),
            retry_policy: RetryPolicy { max_retries: 1, backoff_ms: 50 },
            fallback: Some(FallbackConfig { model: "fb".into(), provider: "fb-p".into() }),
            config: HashMap::new(),
            subgraph: None,
        }
    }

    #[test]
    fn single_never_expands() {
        let node = make_node(StrategyKind::Single);
        assert!(expanded_subgraph(&node).is_none());
    }

    #[test]
    fn total_strategy_expansion_all_variants() {
        let variants = vec![
            StrategyKind::Consensus,
            StrategyKind::Reflection,
            StrategyKind::Chain,
            StrategyKind::Debate,
            StrategyKind::ReAct,
            StrategyKind::Fusion,
            StrategyKind::Custom("my_custom".into()),
        ];

        for kind in variants {
            let node = make_node(kind.clone());
            let sg = expanded_subgraph(&node).expect(&format!("Strategy {kind:?} must expand"));
            assert!(!sg.nodes.is_empty(), "Expanded subgraph for {kind:?} must not be empty");
        }
    }
}