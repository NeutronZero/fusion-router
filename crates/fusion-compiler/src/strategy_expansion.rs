//! Compile-time strategy expansion (Phase 3.5).
//!
//! After `lower_to_graph`, non-`Single` strategy nodes get a prebuilt
//! `ExecutionSubgraph` attached so the Phase 4 runtime subgraph path becomes
//! the production path for all strategies without pulling legacy host code.
//!
//! Pure and deterministic: child UUIDs are derived with `Uuid::new_v5` from the
//! parent node id, role, and index, ensuring identical output for identical inputs.

use fusion_types::*;

/// Upper bound on Consensus fan-out members.
///
/// `count` and `members` come from client config; without a cap an oversized
/// value allocates an unbounded number of child nodes (OOM DoS). Values beyond
/// this bound are rejected by the lowering validator (`strategy_compiler`) and
/// defensively clamped here for direct expansion callers that bypass it.
pub const MAX_CONSENSUS_MEMBERS: u64 = 64;

/// Chain pipeline bounds shared by the lowering validator (`validate_chain_config`)
/// and expansion (`expand_chain`) so both agree on the effective step count:
/// numeric `steps` is clamped to `1..=MAX_CHAIN_STEPS`, and a `stages` array
/// drives the step count within the same window. An empty subgraph is impossible.
pub const MIN_CHAIN_STEPS: u64 = 1;
pub const MAX_CHAIN_STEPS: u64 = 32;
pub const DEFAULT_CHAIN_STEPS: usize = 2;

/// Resolves the effective Chain step count from node config.
///
/// Precedence: length of the `stages` array > numeric `steps` > default.
/// The result is always within `MIN_CHAIN_STEPS..=MAX_CHAIN_STEPS`, so
/// `expand_chain` can never produce an empty or dangling subgraph even when
/// called directly without prior validation.
pub fn resolved_chain_steps(
    config: &std::collections::HashMap<String, serde_json::Value>,
) -> usize {
    if let Some(stages) = config.get("stages").and_then(|v| v.as_array()) {
        let len = stages.len() as u64;
        return len.clamp(MIN_CHAIN_STEPS, MAX_CHAIN_STEPS) as usize;
    }
    if let Some(steps) = config.get("steps").and_then(|v| v.as_u64()) {
        return steps.clamp(MIN_CHAIN_STEPS, MAX_CHAIN_STEPS) as usize;
    }
    DEFAULT_CHAIN_STEPS
}

/// Returns the prebuilt subgraph for a strategy node, or `None` for Single.
pub fn expanded_subgraph(node: &ExecutionNode) -> Option<ExecutionSubgraph> {
    expanded_subgraph_with_custom(node, None)
}

/// Returns the prebuilt subgraph for a strategy node, allowing custom strategy compiler delegate.
///
/// For `Custom` strategies, a delegate compiler **must** be provided. Calling
/// with `None` for a `Custom` node returns `None` — the fallback
/// `expand_custom` path is only available through `compile_custom_subgraph`.
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
            custom_compiler.map(|c| c.compile_subgraph(node, custom_name))
        }
    }
}

/// Expand a `Custom` strategy node using a registered compiler delegate.
///
/// Returns `None` if the name is empty. Panics if no compiler is provided —
/// Custom strategies must always go through a registered delegate.
pub fn compile_custom_subgraph(
    node: &ExecutionNode,
    custom_name: &str,
    compiler: &dyn crate::strategy_compiler::StrategyCompiler,
) -> ExecutionSubgraph {
    compiler.compile_subgraph(node, custom_name)
}

/// Expand all strategies (including Custom) using a map of registered compilers.
///
/// This is the structurally-mandatory entry point for `lower_to_graph`.
pub fn expanded_subgraph_with_compilers(
    node: &ExecutionNode,
    custom_compilers: &std::collections::HashMap<
        String,
        std::sync::Arc<dyn crate::strategy_compiler::StrategyCompiler>,
    >,
) -> Option<ExecutionSubgraph> {
    match &node.strategy {
        StrategyKind::Custom(custom_name) => custom_compilers.get(custom_name).map(|c| {
            let arc: &dyn crate::strategy_compiler::StrategyCompiler = c.as_ref();
            compile_custom_subgraph(node, custom_name, arc)
        }),
        _ => expanded_subgraph(node),
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
///
/// The member count is hard-capped at [`MAX_CONSENSUS_MEMBERS`] (both the
/// numeric `count` and any provided `members` array) so client config can
/// never trigger unbounded allocation.
pub fn expand_consensus(node: &ExecutionNode) -> ExecutionSubgraph {
    let count = node
        .config
        .get("count")
        .and_then(|v| v.as_u64())
        .unwrap_or(3)
        .clamp(1, MAX_CONSENSUS_MEMBERS) as usize;

    let mut members: Vec<String> = node
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
    members.truncate(MAX_CONSENSUS_MEMBERS as usize);

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
///
/// Step count semantics (shared with `validate_chain_config`): a `stages`
/// array drives the pipeline length; a numeric `steps` key is clamped to
/// `1..=MAX_CHAIN_STEPS`. The result is never empty — `entry_node_id` and
/// `exit_node_id` always reference real child nodes.
pub fn expand_chain(node: &ExecutionNode) -> ExecutionSubgraph {
    let steps = resolved_chain_steps(&node.config);

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
            ExecutionEdge {
                from: prop_id,
                to: opp_id,
                condition: None,
            },
            ExecutionEdge {
                from: opp_id,
                to: judge_id,
                condition: None,
            },
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
        edges: vec![ExecutionEdge {
            from: reason_id,
            to: tool_id,
            condition: None,
        }],
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
            ExecutionEdge {
                from: cand1_id,
                to: merger_id,
                condition: None,
            },
            ExecutionEdge {
                from: cand2_id,
                to: merger_id,
                condition: None,
            },
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
            retry_policy: RetryPolicy {
                max_retries: 1,
                backoff_ms: 50,
            },
            fallback: Some(FallbackConfig {
                model: "fb".into(),
                provider: "fb-p".into(),
            }),
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
    fn expand_consensus_huge_count_is_capped() {
        let mut node = make_node(StrategyKind::Consensus);
        node.config
            .insert("count".into(), serde_json::json!(u64::MAX));

        let sg = expanded_subgraph(&node).expect("Consensus must expand");
        // 64 (capped members) + 1 judge — never an unbounded allocation.
        assert_eq!(
            sg.nodes.len(),
            (MAX_CONSENSUS_MEMBERS as usize) + 1,
            "oversized count must clamp to MAX_CONSENSUS_MEMBERS"
        );
    }

    #[test]
    fn expand_consensus_members_list_is_truncated() {
        let mut node = make_node(StrategyKind::Consensus);
        let huge: Vec<String> = (0..10_000).map(|i| format!("model-{i}")).collect();
        node.config
            .insert("members".into(), serde_json::json!(huge));

        let sg = expanded_subgraph(&node).expect("Consensus must expand");
        assert!(
            sg.nodes.len() <= MAX_CONSENSUS_MEMBERS as usize + 1,
            "members list must be capped, got {}",
            sg.nodes.len()
        );
    }

    #[test]
    fn consensus_count_at_bound_expands_fully() {
        let mut node = make_node(StrategyKind::Consensus);
        node.config
            .insert("count".into(), serde_json::json!(MAX_CONSENSUS_MEMBERS));
        let sg = expanded_subgraph(&node).expect("Consensus must expand");
        assert_eq!(sg.nodes.len(), MAX_CONSENSUS_MEMBERS as usize + 1);
        let entry_in_nodes = sg.nodes.iter().any(|n| n.id == sg.entry_node_id);
        let exit_in_nodes = sg.nodes.iter().any(|n| n.id == sg.exit_node_id);
        assert!(
            entry_in_nodes && exit_in_nodes,
            "entry/exit must be real nodes"
        );
    }

    #[test]
    fn total_strategy_expansion_all_variants() {
        let built_in = vec![
            StrategyKind::Consensus,
            StrategyKind::Reflection,
            StrategyKind::Chain,
            StrategyKind::Debate,
            StrategyKind::ReAct,
            StrategyKind::Fusion,
        ];

        for kind in built_in {
            let node = make_node(kind.clone());
            let sg =
                expanded_subgraph(&node).unwrap_or_else(|| panic!("Strategy {kind:?} must expand"));
            assert!(
                !sg.nodes.is_empty(),
                "Expanded subgraph for {kind:?} must not be empty"
            );
        }

        // Custom requires a registered delegate — expanded_subgraph returns None.
        let custom_node = make_node(StrategyKind::Custom("my_custom".into()));
        assert!(
            expanded_subgraph(&custom_node).is_none(),
            "Custom without delegate must return None"
        );

        // With a delegate, Custom expands via expanded_subgraph_with_custom.
        struct MockCustomCompiler;
        impl crate::strategy_compiler::StrategyCompiler for MockCustomCompiler {
            fn compile_subgraph(&self, node: &ExecutionNode, _name: &str) -> ExecutionSubgraph {
                expand_custom(node, _name)
            }
        }
        let sg = expanded_subgraph_with_custom(&custom_node, Some(&MockCustomCompiler))
            .expect("Custom with delegate must expand");
        assert!(
            !sg.nodes.is_empty(),
            "Expanded subgraph for Custom must not be empty"
        );
    }

    fn assert_valid_subgraph(sg: &ExecutionSubgraph, expected_steps: usize) {
        assert_eq!(sg.nodes.len(), expected_steps);
        assert!(
            sg.nodes.iter().any(|n| n.id == sg.entry_node_id),
            "entry_node_id must reference a real node"
        );
        assert!(
            sg.nodes.iter().any(|n| n.id == sg.exit_node_id),
            "exit_node_id must reference a real node"
        );
    }

    #[test]
    fn chain_stages_array_drives_step_count() {
        let mut node = make_node(StrategyKind::Chain);
        node.config.insert(
            "stages".into(),
            serde_json::json!(["draft", "critique", "refine"]),
        );
        let sg = expanded_subgraph(&node).expect("Chain must expand");
        assert_valid_subgraph(&sg, 3);
    }

    #[test]
    fn chain_numeric_steps_clamped_to_bounds() {
        // Upper bound clamp.
        let mut node = make_node(StrategyKind::Chain);
        node.config
            .insert("steps".into(), serde_json::json!(u64::MAX));
        let sg = expanded_subgraph(&node).expect("Chain must expand");
        assert_valid_subgraph(&sg, MAX_CHAIN_STEPS as usize);

        // Exact upper bound.
        let mut node = make_node(StrategyKind::Chain);
        node.config
            .insert("steps".into(), serde_json::json!(MAX_CHAIN_STEPS));
        let sg = expanded_subgraph(&node).expect("Chain must expand");
        assert_valid_subgraph(&sg, MAX_CHAIN_STEPS as usize);

        // Zero clamps to one — an empty/dangling subgraph is impossible even
        // when expansion is invoked without prior validation.
        let mut node = make_node(StrategyKind::Chain);
        node.config.insert("steps".into(), serde_json::json!(0));
        let sg = expanded_subgraph(&node).expect("Chain must expand");
        assert_valid_subgraph(&sg, MIN_CHAIN_STEPS as usize);
    }

    #[test]
    fn chain_defaults_to_two_steps_without_config() {
        let node = make_node(StrategyKind::Chain);
        let sg = expanded_subgraph(&node).expect("Chain must expand");
        assert_valid_subgraph(&sg, DEFAULT_CHAIN_STEPS);
    }

    #[test]
    fn chain_oversized_stages_array_clamped() {
        let mut node = make_node(StrategyKind::Chain);
        let stages: Vec<String> = (0..10_000).map(|i| format!("stage-{i}")).collect();
        node.config
            .insert("stages".into(), serde_json::json!(stages));
        let sg = expanded_subgraph(&node).expect("Chain must expand");
        assert_valid_subgraph(&sg, MAX_CHAIN_STEPS as usize);
    }
}
