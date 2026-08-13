//! Compile-time strategy expansion (Phase 3.5).
//!
//! After `lower_to_graph`, non-`Single` strategy nodes get a prebuilt
//! `ExecutionSubgraph` attached so the Phase 4 runtime subgraph path becomes
//! the production path for Consensus (MVP) without pulling the full `src`
//! strategy registry.
//!
//! The expansion is pure and deterministic: child UUIDs are derived with
//! `Uuid::new_v5` from the parent node id, role, and index, so two compiles
//! of the same node yield identical subgraphs.
//!
//! Scope: only `StrategyKind::Consensus` expands; all other kinds pass
//! through (`subgraph = None`) with a warning.

use fusion_types::*;

/// Returns the prebuilt subgraph for a strategy node, or `None` for
/// passthrough (Single) and unexpanded kinds.
pub fn expanded_subgraph(node: &ExecutionNode) -> Option<ExecutionSubgraph> {
    match node.strategy {
        StrategyKind::Single => None,
        StrategyKind::Consensus => Some(expand_consensus(node)),
        _ => {
            tracing::warn!(
                node_id = %node.id,
                strategy = ?node.strategy,
                "strategy expansion not implemented at compile time; using passthrough"
            );
            None
        }
    }
}

/// Deterministic child id: `v5(namespace, "{parent}:{role}:{index}")`.
fn child_id(parent: uuid::Uuid, role: &str, index: usize) -> uuid::Uuid {
    uuid::Uuid::new_v5(
        &uuid::Uuid::NAMESPACE_URL,
        format!("{}:{}:{}", parent, role, index).as_bytes(),
    )
}

/// Expands a Consensus node into `count` × `LLMGenerate` fan-out feeding one
/// `LLMJudge` (exit). Models come from `config["members"]` when present,
/// otherwise `node.model` repeated `count` times. Children inherit the
/// parent's retry policy, fallback, and base config; nested subgraphs are
/// always `None`.
fn expand_consensus(node: &ExecutionNode) -> ExecutionSubgraph {
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
    fn unexpanded_strategies_pass_through() {
        for kind in [StrategyKind::Debate, StrategyKind::Chain, StrategyKind::Reflection] {
            let node = make_node(kind.clone());
            assert!(expanded_subgraph(&node).is_none(), "{kind:?} must pass through");
        }
    }

    #[test]
    fn consensus_default_shape() {
        let node = make_node(StrategyKind::Consensus);
        let sg = expanded_subgraph(&node).expect("consensus expands");
        assert_eq!(sg.nodes.len(), 4, "default count=3 generates + 1 judge");
        assert_eq!(sg.edges.len(), 3);
        assert_eq!(sg.nodes[0].kind, ExecutionNodeKind::LLMGenerate);
        assert_eq!(sg.nodes.last().unwrap().kind, ExecutionNodeKind::LLMJudge);
        assert_eq!(sg.entry_node_id, sg.nodes[0].id);
        assert_eq!(sg.exit_node_id, sg.nodes[3].id);
        assert!(sg.nodes.iter().all(|n| n.subgraph.is_none()));
        // Every generate feeds the judge
        for gen in &sg.nodes[..3] {
            assert!(sg.edges.iter().any(|e| e.from == gen.id && e.to == sg.exit_node_id));
        }
    }

    #[test]
    fn consensus_member_models_applied() {
        let mut node = make_node(StrategyKind::Consensus);
        node.config.insert("members".into(), serde_json::json!(["m-a", "m-b", "m-c"]));
        let sg = expanded_subgraph(&node).expect("consensus expands");
        let models: Vec<String> = sg.nodes[..3].iter().map(|n| n.model.clone()).collect();
        assert_eq!(models, vec!["m-a", "m-b", "m-c"]);
        assert_eq!(sg.nodes.last().unwrap().model, "parent-model", "judge uses parent model");
    }

    #[test]
    fn consensus_empty_members_repeats_node_model() {
        let node = make_node(StrategyKind::Consensus);
        let sg = expanded_subgraph(&node).expect("consensus expands");
        assert!(sg.nodes[..3].iter().all(|n| n.model == "parent-model"));
    }

    #[test]
    fn consensus_count_from_config() {
        let mut node = make_node(StrategyKind::Consensus);
        node.config.insert("count".into(), serde_json::json!(2));
        let sg = expanded_subgraph(&node).expect("consensus expands");
        assert_eq!(sg.nodes.len(), 3, "count=2 generates + 1 judge");
        assert_eq!(sg.edges.len(), 2);
    }

    #[test]
    fn consensus_ids_deterministic_across_compiles() {
        let node = make_node(StrategyKind::Consensus);
        let a = expanded_subgraph(&node).unwrap();
        let b = expanded_subgraph(&node).unwrap();
        assert_eq!(a.nodes.len(), b.nodes.len());
        for (na, nb) in a.nodes.iter().zip(b.nodes.iter()) {
            assert_eq!(na.id, nb.id, "same parent node must yield same child ids");
        }
        // Different parents produce different ids
        let other = make_node(StrategyKind::Consensus);
        let c = expanded_subgraph(&other).unwrap();
        assert_ne!(a.nodes[0].id, c.nodes[0].id);
    }

    #[test]
    fn consensus_children_copy_retry_and_fallback() {
        let node = make_node(StrategyKind::Consensus);
        let sg = expanded_subgraph(&node).unwrap();
        for child in &sg.nodes {
            assert_eq!(child.retry_policy.max_retries, 1);
            assert_eq!(child.retry_policy.backoff_ms, 50);
            assert_eq!(child.fallback.as_ref().unwrap().model, "fb");
        }
    }

    // -----------------------------------------------------------------------
    // Phase 3.5.7: compile → runtime E2E
    // -----------------------------------------------------------------------

    struct RecordingProvider {
        calls: std::sync::Mutex<Vec<fusion_runtime::ChatRequest>>,
    }

    #[async_trait::async_trait]
    impl fusion_runtime::ChatProvider for RecordingProvider {
        async fn chat_completion(
            &self,
            request: &fusion_runtime::ChatRequest,
        ) -> Result<fusion_runtime::ChatResponse, String> {
            self.calls.lock().unwrap().push(request.clone());
            Ok(fusion_runtime::ChatResponse {
                content: format!("runner response for model {}", request.model),
                usage: Some(fusion_types::Usage {
                    prompt_tokens: 50,
                    completion_tokens: 25,
                    total_tokens: 75,
                }),
                tool_calls: vec![],
            })
        }
    }

    #[tokio::test]
    async fn test_consensus_ir_compiles_and_runs_on_runtime() {
        let mut config = HashMap::new();
        config.insert("count".into(), serde_json::json!(3));
        config.insert("members".into(), serde_json::json!(["m1", "m2", "m3"]));
        let ir = WorkflowIR {
            plan_id: uuid::Uuid::new_v4(),
            nodes: vec![IRNode {
                id: uuid::Uuid::new_v4(),
                kind: IRNodeKind::Generate,
                strategy: StrategyKind::Consensus,
                model: Some("orchestrator".into()),
                config,
            }],
            edges: vec![],
            metadata: IRMetadata {
                policy_applied: vec![],
                estimated_cost: 0.01,
                estimated_tokens: 100,
            },
        };

        let engine = crate::CompilerEngine::new();
        let (_report, graph) = engine
            .compile_and_lower("consensus e2e", &ir)
            .await
            .expect("compile_and_lower");

        assert_eq!(graph.nodes.len(), 1);
        let sg = graph.nodes[0].subgraph.as_ref().expect("consensus must attach subgraph");
        assert_eq!(sg.nodes.len(), 4);

        let provider = std::sync::Arc::new(RecordingProvider {
            calls: std::sync::Mutex::new(Vec::new()),
        });
        let runtime = fusion_runtime::RuntimeEngine::new(provider.clone() as std::sync::Arc<dyn fusion_runtime::ChatProvider>);
        let outcome = runtime.run(std::sync::Arc::new(graph)).await.expect("run");
        assert!(outcome.success, "expanded graph must run successfully");

        let calls = provider.calls.lock().unwrap();
        assert_eq!(calls.len(), 4, "spy must see N generates + 1 judge = 4 calls");
        let models: Vec<&str> = calls.iter().map(|c| c.model.as_str()).collect();
        assert_eq!(models, vec!["m1", "m2", "m3", "orchestrator"]);

        // The judge must see the generate outputs as parent context
        let judge_call = calls.last().unwrap();
        let joined: String = judge_call.messages.iter().map(|m| m.content.clone()).collect();
        assert!(
            joined.contains("runner response for model m1"),
            "judge must see generate output m1, got: {joined}"
        );
        assert!(
            joined.contains("runner response for model m3"),
            "judge must see generate output m3, got: {joined}"
        );
    }
}