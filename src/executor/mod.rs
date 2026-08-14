use async_trait::async_trait;

pub mod capability_executor;
mod fusion_bridge;
mod node_exec;
#[cfg(test)]
mod tool_loop;

pub use fusion_bridge::{connect_tools, FusionChatProvider, FusionTool};
pub use node_exec::DefaultExecutor;

use crate::types::{ExecutionNode, NodeExecContext, NodeExecutionResult};

#[async_trait]
pub trait Executor: Send + Sync {
    async fn execute_node(
        &self,
        node: &ExecutionNode,
        ctx: &NodeExecContext,
    ) -> NodeExecutionResult;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::providers::ChatProvider;
    use crate::strategies::consensus::ConsensusStrategy;
    use crate::strategies::single::SingleStrategy;
    use crate::strategies::Strategy;
    use crate::tools::ToolRegistry;
    use crate::types::*;
    use std::collections::HashMap;
    use std::sync::Arc;
    use uuid::Uuid;

    struct MockChatProvider;

    #[async_trait]
    impl ChatProvider for MockChatProvider {
        async fn chat_completion(
            &self,
            _request: &ChatCompletionRequest,
        ) -> anyhow::Result<ChatCompletionResponse> {
            Ok(ChatCompletionResponse {
                id: "mock".into(),
                object: "chat.completion".into(),
                created: 0,
                model: "mock-model".into(),
                choices: vec![Choice {
                    index: 0,
                    message: ChatMessage {
                        role: "assistant".into(),
                        content: "mock response".into(),
                    },
                    finish_reason: "stop".into(),
                }],
                native_tool_calls: None,
                usage: Some(Usage {
                    prompt_tokens: 10,
                    completion_tokens: 20,
                    total_tokens: 30,
                }),
            })
        }

        fn name(&self) -> &str {
            "mock"
        }
    }

    struct CapturingMockProvider(Arc<std::sync::Mutex<Option<ChatCompletionRequest>>>);

    #[async_trait]
    impl ChatProvider for CapturingMockProvider {
        async fn chat_completion(
            &self,
            request: &ChatCompletionRequest,
        ) -> anyhow::Result<ChatCompletionResponse> {
            *self.0.lock().unwrap() = Some(request.clone());
            Ok(ChatCompletionResponse {
                id: "mock".into(),
                object: "chat.completion".into(),
                created: 0,
                model: request.model.clone(),
                choices: vec![Choice {
                    index: 0,
                    message: ChatMessage {
                        role: "assistant".into(),
                        content: "mock response".into(),
                    },
                    finish_reason: "stop".into(),
                }],
                native_tool_calls: None,
                usage: Some(Usage {
                    prompt_tokens: 10,
                    completion_tokens: 20,
                    total_tokens: 30,
                }),
            })
        }

        fn name(&self) -> &str {
            "capturing"
        }
    }

    fn make_llm_node(strategy: StrategyKind) -> ExecutionNode {
        ExecutionNode {
            id: Uuid::new_v4(),
            kind: ExecutionNodeKind::LLMGenerate,
            strategy,
            model: "gpt-4".to_string(),
            retry_policy: RetryPolicy {
                max_retries: 3,
                backoff_ms: 1000,
            },
            fallback: None,
            config: HashMap::new(),
            subgraph: None,
        }
    }

    fn make_llm_node_with_subgraph(strategy: StrategyKind) -> ExecutionNode {
        use fusion_compiler::strategy_expansion::expanded_subgraph;
        let mut node = make_llm_node(strategy.clone());
        node.subgraph = expanded_subgraph(&node);
        node
    }

    fn make_judge_node(strategy: StrategyKind) -> ExecutionNode {
        ExecutionNode {
            id: Uuid::new_v4(),
            kind: ExecutionNodeKind::LLMJudge,
            strategy,
            model: "gpt-4".to_string(),
            retry_policy: RetryPolicy {
                max_retries: 3,
                backoff_ms: 1000,
            },
            fallback: None,
            config: HashMap::new(),
            subgraph: None,
        }
    }

    fn make_judge_node_with_subgraph(strategy: StrategyKind) -> ExecutionNode {
        use fusion_compiler::strategy_expansion::expanded_subgraph;
        let mut node = make_judge_node(strategy.clone());
        node.subgraph = expanded_subgraph(&node);
        node
    }

    #[tokio::test]
    async fn test_debate_string_roles_lower_to_real_subgraph() {
        use fusion_compiler::strategy_expansion::expanded_subgraph;

        let node = make_llm_node(StrategyKind::Debate);
        let subgraph = expanded_subgraph(&node).expect("Debate must expand");

        assert_eq!(
            subgraph.nodes.len(),
            3,
            "Debate must produce proposer + opposer + judge"
        );
        assert!(matches!(
            subgraph.nodes[0].kind,
            ExecutionNodeKind::LLMGenerate
        ));
        assert!(matches!(
            subgraph.nodes[1].kind,
            ExecutionNodeKind::LLMGenerate
        ));
        assert!(matches!(
            subgraph.nodes.last().unwrap().kind,
            ExecutionNodeKind::LLMJudge
        ));
    }

    #[tokio::test]
    async fn test_prebuilt_consensus_subgraph_inherits_node_model() {
        use fusion_compiler::strategy_expansion::expanded_subgraph;

        let mut node = make_llm_node(StrategyKind::Consensus);
        node.model = "gpt-4-turbo".into();
        let subgraph = expanded_subgraph(&node).expect("Consensus must expand");

        assert_eq!(subgraph.nodes.len(), 4);
        assert!(
            subgraph.nodes.iter().all(|n| n.model == "gpt-4-turbo"),
            "subgraph nodes must inherit the workflow node's model, got: {:?}",
            subgraph.nodes.iter().map(|n| &n.model).collect::<Vec<_>>()
        );
    }

    #[tokio::test]
    async fn test_propagate_parent_messages_to_subnodes() {
        use super::node_exec::propagate_parent_messages;

        let mut node = make_llm_node(StrategyKind::Consensus);
        node.config.insert(
            "messages".into(),
            serde_json::json!([{ "role": "user", "content": "analyze the repo" }]),
        );

        let mut subgraph = fusion_compiler::strategy_expansion::expanded_subgraph(&node)
            .expect("Consensus must expand");
        propagate_parent_messages(&node, &mut subgraph);

        assert_eq!(subgraph.nodes.len(), 4);
        for sub_node in &subgraph.nodes {
            let messages = sub_node
                .config
                .get("messages")
                .and_then(|v| v.as_array())
                .expect("LLM sub-node must inherit parent messages");
            assert_eq!(messages.len(), 1);
            assert_eq!(messages[0]["role"], "user");
            assert_eq!(messages[0]["content"], "analyze the repo");
        }
    }

    #[tokio::test]
    async fn test_execute_node_single_strategy() {
        let provider = Arc::new(MockChatProvider);
        let mut strategies: HashMap<StrategyKind, Box<dyn Strategy + Send + Sync>> = HashMap::new();
        strategies.insert(StrategyKind::Single, Box::new(SingleStrategy));
        let executor = DefaultExecutor::new(provider, strategies);
        let node = make_llm_node(StrategyKind::Single);

        let result = executor
            .execute_node(&node, &NodeExecContext::default())
            .await;

        assert_eq!(result.state, NodeState::Succeeded);
        assert_eq!(
            result.output,
            Some(serde_json::Value::String("mock response".into()))
        );
        let usage = result.usage.expect("usage should be present");
        assert_eq!(usage.prompt_tokens, 10);
        assert_eq!(usage.completion_tokens, 20);
        assert_eq!(usage.total_tokens, 30);
    }

    #[tokio::test]
    async fn test_execute_node_strategy_fallback() {
        let provider = Arc::new(MockChatProvider);
        let strategies: HashMap<StrategyKind, Box<dyn Strategy + Send + Sync>> = HashMap::new();
        let executor = DefaultExecutor::new(provider, strategies);
        let node = make_llm_node(StrategyKind::Fusion);

        let result = executor
            .execute_node(&node, &NodeExecContext::default())
            .await;

        assert_eq!(result.state, NodeState::Succeeded);
    }

    #[tokio::test]
    async fn test_single_strategy_has_no_subgraph() {
        use fusion_compiler::strategy_expansion::expanded_subgraph;

        let node = make_llm_node(StrategyKind::Single);
        assert!(
            expanded_subgraph(&node).is_none(),
            "Single strategy must not produce a subgraph"
        );
    }

    #[tokio::test]
    async fn test_prebuilt_consensus_produces_judge_as_exit() {
        use fusion_compiler::strategy_expansion::expanded_subgraph;

        let node = make_llm_node(StrategyKind::Consensus);
        let subgraph = expanded_subgraph(&node).expect("Consensus must expand");

        assert_eq!(subgraph.nodes.len(), 4);
        assert!(matches!(
            subgraph.nodes[0].kind,
            ExecutionNodeKind::LLMGenerate
        ));
        assert!(matches!(
            subgraph.nodes.last().unwrap().kind,
            ExecutionNodeKind::LLMJudge
        ));
    }

    #[tokio::test]
    async fn test_build_request_injects_system_prompt() {
        let captured = Arc::new(std::sync::Mutex::new(None::<ChatCompletionRequest>));
        let provider = Arc::new(CapturingMockProvider(captured.clone()));
        let strategies: HashMap<StrategyKind, Box<dyn Strategy + Send + Sync>> = HashMap::new();
        let executor = DefaultExecutor::new(provider, strategies);
        let node = make_judge_node(StrategyKind::Fusion);

        let _ = executor
            .execute_node(&node, &NodeExecContext::default())
            .await;

        let request = captured.lock().unwrap().take().unwrap();
        let has_system = request.messages.iter().any(|m| m.role == "system");
        assert!(has_system, "expected a system message to be injected");
        let first_role = &request.messages[0].role;
        assert_eq!(first_role, "system", "system message should be first");
        assert!(
            request.messages[0].content.contains("judge"),
            "system prompt should reference 'judge' role"
        );
    }

    /// Captures ALL requests (not just the last) so we can verify
    /// which node received what input.
    struct CapturingAllProvider(Arc<std::sync::Mutex<Vec<ChatCompletionRequest>>>);

    #[async_trait]
    impl ChatProvider for CapturingAllProvider {
        async fn chat_completion(
            &self,
            request: &ChatCompletionRequest,
        ) -> anyhow::Result<ChatCompletionResponse> {
            let mut seen = self.0.lock().unwrap();
            seen.push(request.clone());
            let idx = seen.len();
            Ok(ChatCompletionResponse {
                id: "mock".into(),
                object: "chat.completion".into(),
                created: 0,
                model: request.model.clone(),
                choices: vec![Choice {
                    index: 0,
                    message: ChatMessage {
                        role: "assistant".into(),
                        content: format!("response-{}", idx),
                    },
                    finish_reason: "stop".into(),
                }],
                native_tool_calls: None,
                usage: Some(Usage {
                    prompt_tokens: 10,
                    completion_tokens: 20,
                    total_tokens: 30,
                }),
            })
        }

        fn name(&self) -> &str {
            "capturing-all"
        }
    }

    #[tokio::test]
    async fn test_consensus_judge_sees_member_outputs() {
        let captured = Arc::new(std::sync::Mutex::new(Vec::new()));
        let provider = Arc::new(CapturingAllProvider(captured.clone()));
        let mut strategies: HashMap<StrategyKind, Box<dyn Strategy + Send + Sync>> = HashMap::new();
        strategies.insert(
            StrategyKind::Consensus,
            Box::new(ConsensusStrategy { count: 2 }),
        );
        let executor = DefaultExecutor::new(provider, strategies);

        let mut node = make_llm_node(StrategyKind::Consensus);
        node.config.insert(
            "messages".into(),
            serde_json::json!([{"role": "user", "content": "original prompt"}]),
        );

        let result = executor
            .execute_node(&node, &NodeExecContext::default())
            .await;
        assert_eq!(result.state, NodeState::Succeeded);

        let requests = captured.lock().unwrap();
        // 2 members + 1 judge
        assert!(
            requests.len() >= 2,
            "expected at least member + judge calls"
        );

        // The judge request (last one, since judge runs last in topo order)
        let judge_request = requests.last().expect("should have judge request");
        let judge_messages = judge_request
            .messages
            .iter()
            .map(|m| (&m.role, m.content.as_str()))
            .collect::<Vec<_>>();

        // Judge should have a system prompt mentioning judging
        let has_judge_system = judge_messages
            .iter()
            .any(|(r, c)| *r == "system" && c.contains("judge"));
        assert!(has_judge_system, "judge request should have system prompt");

        // CRITICAL: judge should see member outputs in its context
        // (currently broken — judge sees only the original prompt)
        let judge_user_content = judge_messages
            .iter()
            .filter(|(r, _)| *r == "user")
            .map(|(_, c)| *c)
            .collect::<Vec<_>>();
        let sees_member_outputs = judge_user_content.iter().any(|c| c.contains("response-"));
        assert!(
            sees_member_outputs,
            "judge must see member outputs, not just the original prompt. Judge user messages: {:?}",
            judge_user_content
        );
    }

    /// Provider with configurable content and provider-native tool_calls.
    struct ToolCallProvider {
        content: String,
        tool_calls: Option<Vec<ToolCall>>,
    }

    /// Provider that emits tool calls for the first `tool_call_requests`
    /// requests, then falls back to plain text — used to verify the bounded
    /// ReAct-style tool loop in the executor.
    struct ToolLoopProvider {
        text: String,
        tool_call_requests: usize,
        tool_calls: Vec<ToolCall>,
        request_count: std::sync::atomic::AtomicUsize,
    }

    #[async_trait]
    impl ChatProvider for ToolLoopProvider {
        fn name(&self) -> &str {
            "tool-loop-provider"
        }

        async fn chat_completion(
            &self,
            _request: &ChatCompletionRequest,
        ) -> anyhow::Result<ChatCompletionResponse> {
            let n = self
                .request_count
                .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            let calls = if n < self.tool_call_requests {
                Some(self.tool_calls.clone())
            } else {
                None
            };
            Ok(ChatCompletionResponse {
                id: format!("tool-loop-{}", n),
                object: "chat.completion".into(),
                created: 0,
                model: "tool-loop-model".into(),
                choices: vec![Choice {
                    index: 0,
                    message: ChatMessage {
                        role: "assistant".into(),
                        content: self.text.clone(),
                    },
                    finish_reason: "stop".into(),
                }],
                native_tool_calls: calls,
                usage: Some(Usage {
                    prompt_tokens: 10,
                    completion_tokens: 5,
                    total_tokens: 15,
                }),
            })
        }
    }

    #[async_trait]
    impl ChatProvider for ToolCallProvider {
        fn name(&self) -> &str {
            "tool-call-provider"
        }

        async fn chat_completion(
            &self,
            _request: &ChatCompletionRequest,
        ) -> anyhow::Result<ChatCompletionResponse> {
            Ok(ChatCompletionResponse {
                id: "tool".into(),
                object: "chat.completion".into(),
                created: 0,
                model: "tool-model".into(),
                choices: vec![Choice {
                    index: 0,
                    message: ChatMessage {
                        role: "assistant".into(),
                        content: self.content.clone(),
                    },
                    finish_reason: "stop".into(),
                }],
                usage: None,
                native_tool_calls: self.tool_calls.clone(),
            })
        }
    }

    fn calculator_registry() -> Arc<ToolRegistry> {
        let mut registry = ToolRegistry::new();
        registry.register(Arc::new(crate::tools::builtin::CalculatorTool));
        registry.register(Arc::new(crate::tools::builtin::SearchTool));
        Arc::new(registry)
    }

    fn single_strategies() -> HashMap<StrategyKind, Box<dyn Strategy + Send + Sync>> {
        let mut strategies: HashMap<StrategyKind, Box<dyn Strategy + Send + Sync>> = HashMap::new();
        strategies.insert(StrategyKind::Single, Box::new(SingleStrategy));
        strategies
    }

    /// Law 7 / ADR-037: a model output containing a free-form tool JSON
    /// object is returned as TEXT and never executed.
    #[tokio::test]
    async fn law7_no_freeform_tool_parsing() {
        let tool_json = r#"{"tool": "calculator", "args": {"expression": "2+2"}}"#;
        let provider = Arc::new(ToolCallProvider {
            content: tool_json.to_string(),
            tool_calls: None,
        });
        let executor = DefaultExecutor::new(provider, single_strategies())
            .with_tool_registry(calculator_registry())
            .with_allow_auto_exec(true);
        let mut node = make_llm_node(StrategyKind::Single);
        node.config
            .insert("tool_allowlist".into(), serde_json::json!(["calculator"]));

        let result = executor
            .execute_node(&node, &NodeExecContext::default())
            .await;

        assert_eq!(result.state, NodeState::Succeeded);
        let output = result.output.expect("output must be present");
        assert_eq!(
            output,
            serde_json::Value::String(tool_json.to_string()),
            "tool-shaped JSON in content must be returned as text, never executed"
        );
        assert!(
            !output.to_string().contains("\"result\""),
            "the calculator must never have run"
        );
    }

    /// Law 7: provider-native tool_calls execute ONLY allowlisted tools.
    #[tokio::test]
    async fn law7_native_tool_calls_execute_only_allowlisted() {
        let provider = Arc::new(ToolCallProvider {
            content: String::new(),
            tool_calls: Some(vec![
                ToolCall {
                    id: "c1".into(),
                    name: "calculator".into(),
                    arguments: serde_json::json!({"expression": "2+2"}),
                },
                ToolCall {
                    id: "s1".into(),
                    name: "search".into(),
                    arguments: serde_json::json!({"query": "x"}),
                },
            ]),
        });
        let executor = DefaultExecutor::new(provider, single_strategies())
            .with_tool_registry(calculator_registry())
            .with_allow_auto_exec(true);
        let mut node = make_llm_node(StrategyKind::Single);
        node.config
            .insert("tool_allowlist".into(), serde_json::json!(["calculator"]));

        let result = executor
            .execute_node(&node, &NodeExecContext::default())
            .await;

        assert_eq!(result.state, NodeState::Succeeded);
        let output = result.output.expect("tool call results must be produced");
        let calls = output["tool_calls"].as_array().expect("tool_calls array");
        assert_eq!(calls.len(), 2);
        let calc = &calls[0];
        assert_eq!(calc["tool"], "calculator");
        assert_eq!(calc["executed"], true);
        assert_eq!(calc["result"]["result"], 4.0, "calculator must run 2+2");
        let search = &calls[1];
        assert_eq!(search["tool"], "search");
        assert_eq!(search["executed"], false, "search is outside the allowlist");
        assert!(
            search["reason"]
                .as_str()
                .unwrap_or("")
                .contains("allowlist"),
            "non-allowlisted call must explain why it was not executed"
        );
    }

    /// Law 7: with auto-execution disabled (default), native tool_calls are
    /// never executed even when the request names an allowlist.
    #[tokio::test]
    async fn law7_native_tool_calls_not_executed_when_auto_exec_disabled() {
        let provider = Arc::new(ToolCallProvider {
            content: String::new(),
            tool_calls: Some(vec![ToolCall {
                id: "c1".into(),
                name: "calculator".into(),
                arguments: serde_json::json!({"expression": "2+2"}),
            }]),
        });
        let executor = DefaultExecutor::new(provider, single_strategies())
            .with_tool_registry(calculator_registry());
        let mut node = make_llm_node(StrategyKind::Single);
        node.config
            .insert("tool_allowlist".into(), serde_json::json!(["calculator"]));

        let result = executor
            .execute_node(&node, &NodeExecContext::default())
            .await;

        assert_eq!(result.state, NodeState::Succeeded);
        let output = result.output.expect("tool call results must be produced");
        assert_eq!(
            output["tool_calls"][0]["executed"], false,
            "auto-exec disabled must never execute a tool"
        );
        assert!(
            !output.to_string().contains("\"result\""),
            "the calculator must never have run"
        );
    }

    /// Law 7: an empty per-request allowlist blocks all tool execution
    /// (fail closed), even with auto-exec enabled.
    #[tokio::test]
    async fn law7_empty_allowlist_blocks_all_tools() {
        let provider = Arc::new(ToolCallProvider {
            content: String::new(),
            tool_calls: Some(vec![ToolCall {
                id: "c1".into(),
                name: "calculator".into(),
                arguments: serde_json::json!({"expression": "2+2"}),
            }]),
        });
        let executor = DefaultExecutor::new(provider, single_strategies())
            .with_tool_registry(calculator_registry())
            .with_allow_auto_exec(true);

        let node = make_llm_node(StrategyKind::Single);

        let result = executor
            .execute_node(&node, &NodeExecContext::default())
            .await;
        assert_eq!(result.state, NodeState::Succeeded);
        let output = result.output.expect("tool call results must be produced");
        assert_eq!(
            output["tool_calls"][0]["executed"], false,
            "absent allowlist must block all tool execution"
        );
    }

    /// Law 7: when auto-exec is enabled with an allowlist, tool definitions
    /// are advertised to the provider; otherwise the request carries none.
    #[tokio::test]
    async fn law7_tool_definitions_only_sent_with_allowlist() {
        let captured = Arc::new(std::sync::Mutex::new(None::<ChatCompletionRequest>));
        let provider = Arc::new(CapturingMockProvider(captured.clone()));
        let executor = DefaultExecutor::new(provider.clone(), single_strategies())
            .with_tool_registry(calculator_registry())
            .with_allow_auto_exec(true);
        let node = make_llm_node(StrategyKind::Single);

        let _ = executor
            .execute_node(&node, &NodeExecContext::default())
            .await;
        let request = captured.lock().unwrap().take().unwrap();
        assert!(
            request.tools.is_none(),
            "no allowlist in request means no tool definitions may be advertised"
        );

        let mut node = make_llm_node(StrategyKind::Single);
        node.config
            .insert("tool_allowlist".into(), serde_json::json!(["calculator"]));
        let _ = executor
            .execute_node(&node, &NodeExecContext::default())
            .await;
        let request = captured.lock().unwrap().take().unwrap();
        let tools = request
            .tools
            .expect("allowlist must advertise tool definitions");
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0].name, "calculator");
        assert!(tools[0].parameters.is_some(), "schema must be advertised");

        let executor_disabled = DefaultExecutor::new(provider, single_strategies())
            .with_tool_registry(calculator_registry());
        let mut node = make_llm_node(StrategyKind::Single);
        node.config
            .insert("tool_allowlist".into(), serde_json::json!(["calculator"]));
        let _ = executor_disabled
            .execute_node(&node, &NodeExecContext::default())
            .await;
        let request = captured.lock().unwrap().take().unwrap();
        assert!(
            request.tools.is_none(),
            "auto-exec disabled must not advertise tool definitions"
        );
    }

    /// The bounded tool loop re-prompts the model after executing native tool
    /// calls, then final output is the model's text once it stops calling
    /// tools.
    #[tokio::test]
    async fn test_tool_loop_re_prompts_until_model_emits_text() {
        let provider = Arc::new(ToolLoopProvider {
            text: "final review text".into(),
            tool_call_requests: 2,
            tool_calls: vec![ToolCall {
                id: "lr".into(),
                name: "file_read".into(),
                arguments: serde_json::json!({"path": "src/executor/mod.rs"}),
            }],
            request_count: std::sync::atomic::AtomicUsize::new(0),
        });
        let mut registry = ToolRegistry::new();
        registry.register(Arc::new(crate::tools::builtin::FileReadTool::new(
            ".".into(),
        )));
        let executor = DefaultExecutor::new(provider, single_strategies())
            .with_tool_registry(Arc::new(registry))
            .with_allow_auto_exec(true);
        let mut node = make_llm_node(StrategyKind::Single);
        node.config
            .insert("tool_allowlist".into(), serde_json::json!(["file_read"]));

        let result = executor
            .execute_node(&node, &NodeExecContext::default())
            .await;

        assert_eq!(result.state, NodeState::Succeeded);
        assert_eq!(
            result.output,
            Some(serde_json::Value::String("final review text".into())),
            "after tool rounds the model's final text must be the output"
        );
        let usage = result.usage.expect("usage accumulated across rounds");
        assert_eq!(
            usage.total_tokens, 45,
            "3 provider calls (2 tool + 1 text) x 15 tokens"
        );
    }

    /// The tool loop must terminate even when the model never stops calling
    /// tools — the round budget caps it.
    #[tokio::test]
    async fn test_tool_loop_honors_round_budget() {
        let provider = Arc::new(ToolLoopProvider {
            text: String::new(),
            tool_call_requests: usize::MAX,
            tool_calls: vec![ToolCall {
                id: "c".into(),
                name: "calculator".into(),
                arguments: serde_json::json!({"expression": "1+1"}),
            }],
            request_count: std::sync::atomic::AtomicUsize::new(0),
        });
        let executor = DefaultExecutor::new(provider, single_strategies())
            .with_tool_registry(calculator_registry())
            .with_allow_auto_exec(true);
        let mut node = make_llm_node(StrategyKind::Single);
        node.config
            .insert("tool_allowlist".into(), serde_json::json!(["calculator"]));
        node.config
            .insert("max_tool_rounds".into(), serde_json::json!(3));

        let result = executor
            .execute_node(&node, &NodeExecContext::default())
            .await;

        assert_eq!(result.state, NodeState::Succeeded);
        assert!(
            result.output.is_some(),
            "budget-exhausted loop must still surface the last tool results"
        );
        assert_eq!(result.output.unwrap()["tool_calls"][0]["executed"], true);
    }

    #[cfg(feature = "semantic-cache")]
    mod cache_tests {
        use super::*;
        use crate::cache::embeddings::Embedder;
        use crate::cache::SemanticCache;

        /// Deterministic per-text embeddings: identical keys embed identically
        /// (cosine 1.0), different keys embed differently.
        struct DeterministicEmbedder;

        #[async_trait]
        impl Embedder for DeterministicEmbedder {
            async fn embed(&self, text: &str) -> Result<Vec<f32>, String> {
                let mut v = vec![0.0f32; 64];
                for (i, b) in text.bytes().enumerate() {
                    v[i % 64] += b as f32;
                }
                let norm = v.iter().map(|x| x * x).sum::<f32>().sqrt();
                if norm > 0.0 {
                    for x in &mut v {
                        *x /= norm;
                    }
                }
                Ok(v)
            }
        }

        struct CountingProvider(Arc<std::sync::atomic::AtomicUsize>);

        #[async_trait]
        impl ChatProvider for CountingProvider {
            async fn chat_completion(
                &self,
                request: &ChatCompletionRequest,
            ) -> anyhow::Result<ChatCompletionResponse> {
                self.0.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                Ok(ChatCompletionResponse {
                    id: "count".into(),
                    object: "chat.completion".into(),
                    created: 0,
                    model: request.model.clone(),
                    choices: vec![Choice {
                        index: 0,
                        message: ChatMessage {
                            role: "assistant".into(),
                            content: "judge verdict".into(),
                        },
                        finish_reason: "stop".into(),
                    }],
                    native_tool_calls: None,
                    usage: Some(Usage {
                        prompt_tokens: 1,
                        completion_tokens: 1,
                        total_tokens: 2,
                    }),
                })
            }

            fn name(&self) -> &str {
                "counting"
            }
        }

        /// A cache hit on one consensus member must satisfy only that member:
        /// the remaining members and the judge still execute, and the judge's
        /// output becomes the strategy result (regression for the early
        /// `return` that used to abort the whole subgraph).
        #[tokio::test]
        async fn test_cache_hit_continues_remaining_subgraph() {
            let counter = Arc::new(std::sync::atomic::AtomicUsize::new(0));
            let provider = Arc::new(CountingProvider(counter.clone()));
            let mut strategies: HashMap<StrategyKind, Box<dyn Strategy + Send + Sync>> =
                HashMap::new();
            strategies.insert(
                StrategyKind::Consensus,
                Box::new(ConsensusStrategy::default()),
            );
            let executor = DefaultExecutor::new(provider, strategies);

            let mut node = make_llm_node(StrategyKind::Consensus);
            node.config.insert(
                "messages".into(),
                serde_json::json!([{ "role": "user", "content": "hello cache test" }]),
            );

            // Reconstruct the exact request the first member will produce and
            // pre-populate the cache for it.
            let member_request = ChatCompletionRequest {
                model: node.model.clone(),
                messages: vec![ChatMessage {
                    role: "user".into(),
                    content: "hello cache test".into(),
                }],
                stream: false,
                temperature: None,
                max_tokens: None,
                tools: None,
                files: None,
                execution: None,
                output: None,
                strategy: None,
            };
            let cache_key = DefaultExecutor::cache_key(&member_request);

            let cache = Arc::new(SemanticCache::new(
                Arc::new(DeterministicEmbedder),
                0.99,
                100,
                64,
            ));
            cache
                .put(
                    &cache_key,
                    serde_json::json!({ "content": "cached member" }),
                )
                .await;
            let executor = executor.with_cache(cache);

            let result = executor
                .execute_node(&node, &NodeExecContext::default())
                .await;

            assert_eq!(result.state, NodeState::Succeeded);
            assert_eq!(
                result.output,
                Some(serde_json::Value::String("judge verdict".into())),
                "a cached member must not become the whole strategy's output"
            );
            assert_eq!(
                counter.load(std::sync::atomic::Ordering::SeqCst),
                1,
                "only the judge should hit the provider (all members cached)"
            );
        }
    }

    #[tokio::test]
    async fn test_execute_node_preserves_accumulated_usage_on_failure() {
        struct FailingProvider {
            calls: std::sync::atomic::AtomicUsize,
        }

        #[async_trait]
        impl ChatProvider for FailingProvider {
            fn name(&self) -> &str {
                "failing"
            }
            async fn chat_completion(
                &self,
                _req: &ChatCompletionRequest,
            ) -> anyhow::Result<ChatCompletionResponse> {
                let n = self.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                if n == 0 {
                    Ok(ChatCompletionResponse {
                        id: "ok".into(),
                        object: "chat.completion".into(),
                        created: 0,
                        model: "mock".into(),
                        choices: vec![Choice {
                            index: 0,
                            message: ChatMessage {
                                role: "assistant".into(),
                                content: "ok".into(),
                            },
                            finish_reason: "stop".into(),
                        }],
                        native_tool_calls: None,
                        usage: Some(Usage {
                            prompt_tokens: 10,
                            completion_tokens: 20,
                            total_tokens: 30,
                        }),
                    })
                } else {
                    anyhow::bail!("provider simulated failure")
                }
            }
        }

        let provider = Arc::new(FailingProvider {
            calls: std::sync::atomic::AtomicUsize::new(0),
        });
        let mut strategies: HashMap<StrategyKind, Box<dyn Strategy + Send + Sync>> = HashMap::new();
        strategies.insert(
            StrategyKind::Consensus,
            Box::new(crate::strategies::consensus::ConsensusStrategy::default()),
        );
        let executor = DefaultExecutor::new(provider, strategies);
        let node = make_llm_node(StrategyKind::Consensus);

        let result = executor
            .execute_node(&node, &NodeExecContext::default())
            .await;

        assert!(matches!(result.state, NodeState::Failed(_)));
        let usage = result.usage.expect("accumulated usage from successful first stage must be preserved on second stage failure");
        assert_eq!(usage.prompt_tokens, 10);
        assert_eq!(usage.completion_tokens, 20);
        assert_eq!(usage.total_tokens, 30);
    }

    // -----------------------------------------------------------------------
    // Phase 6.4: plain Single leaves delegate to fusion_runtime::ProviderExecutor
    // -----------------------------------------------------------------------

    fn make_single_leaf(config: HashMap<&str, serde_json::Value>) -> ExecutionNode {
        ExecutionNode {
            id: Uuid::new_v4(),
            kind: ExecutionNodeKind::LLMGenerate,
            strategy: StrategyKind::Single,
            model: "gpt-4".to_string(),
            retry_policy: RetryPolicy {
                max_retries: 0,
                backoff_ms: 0,
            },
            fallback: None,
            config: config
                .into_iter()
                .map(|(k, v)| (k.to_string(), v))
                .collect(),
            subgraph: None,
        }
    }

    /// Provider that fails the first `fail_first` requests, then succeeds.
    struct SequenceProvider {
        calls: std::sync::atomic::AtomicUsize,
        fail_first: usize,
        fail_models: Vec<String>,
        content: String,
        recorded_models: std::sync::Mutex<Vec<String>>,
    }

    #[async_trait]
    impl ChatProvider for SequenceProvider {
        fn name(&self) -> &str {
            "sequence"
        }

        async fn chat_completion(
            &self,
            request: &ChatCompletionRequest,
        ) -> anyhow::Result<ChatCompletionResponse> {
            self.recorded_models
                .lock()
                .unwrap()
                .push(request.model.clone());
            let n = self.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            let model_fails = self.fail_models.iter().any(|m| m == &request.model);
            if model_fails || n < self.fail_first {
                anyhow::bail!("provider simulated failure");
            }
            Ok(ChatCompletionResponse {
                id: "seq".into(),
                object: "chat.completion".into(),
                created: 0,
                model: request.model.clone(),
                choices: vec![Choice {
                    index: 0,
                    message: ChatMessage {
                        role: "assistant".into(),
                        content: self.content.clone(),
                    },
                    finish_reason: "stop".into(),
                }],
                native_tool_calls: None,
                usage: Some(Usage {
                    prompt_tokens: 10,
                    completion_tokens: 20,
                    total_tokens: 30,
                }),
            })
        }
    }

    #[tokio::test]
    async fn test_single_leaf_delegates_to_crates_executor_with_string_output() {
        let provider = Arc::new(SequenceProvider {
            calls: std::sync::atomic::AtomicUsize::new(0),
            fail_first: 0,
            fail_models: vec![],
            content: "crates response".into(),
            recorded_models: std::sync::Mutex::new(Vec::new()),
        });
        let executor = DefaultExecutor::new(provider.clone(), HashMap::new());
        let node = make_single_leaf(HashMap::new());

        let result = executor
            .execute_node(&node, &NodeExecContext::default())
            .await;

        assert_eq!(result.state, NodeState::Succeeded);
        assert_eq!(
            result.output,
            Some(serde_json::Value::String("crates response".into())),
            "leaf LLM output must keep the src String contract"
        );
        assert_eq!(provider.calls.load(std::sync::atomic::Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn test_single_leaf_retries_inside_crates_executor() {
        let provider = Arc::new(SequenceProvider {
            calls: std::sync::atomic::AtomicUsize::new(0),
            fail_first: 1,
            fail_models: vec![],
            content: "recovered".into(),
            recorded_models: std::sync::Mutex::new(Vec::new()),
        });
        let executor = DefaultExecutor::new(provider.clone(), HashMap::new());
        let mut node = make_single_leaf(HashMap::new());
        node.retry_policy = RetryPolicy {
            max_retries: 2,
            backoff_ms: 0,
        };

        let result = executor
            .execute_node(&node, &NodeExecContext::default())
            .await;

        assert_eq!(result.state, NodeState::Succeeded);
        assert_eq!(
            provider.calls.load(std::sync::atomic::Ordering::SeqCst),
            2,
            "initial attempt + 1 retry inside ProviderExecutor"
        );
    }

    #[tokio::test]
    async fn test_single_leaf_fallback_inside_crates_executor() {
        let provider = Arc::new(SequenceProvider {
            calls: std::sync::atomic::AtomicUsize::new(0),
            fail_first: 0,
            fail_models: vec!["gpt-4".to_string()],
            content: "fallback answer".into(),
            recorded_models: std::sync::Mutex::new(Vec::new()),
        });
        let executor = DefaultExecutor::new(provider.clone(), HashMap::new());
        let mut node = make_single_leaf(HashMap::new());
        node.fallback = Some(FallbackConfig {
            model: "fallback-model".into(),
            provider: "fb".into(),
        });

        let result = executor
            .execute_node(&node, &NodeExecContext::default())
            .await;

        assert_eq!(result.state, NodeState::Succeeded);
        let models = provider.recorded_models.lock().unwrap().clone();
        assert_eq!(
            models.len(),
            2,
            "primary fails once, fallback model attempted"
        );
        assert_eq!(models[1], "fallback-model");
    }

    #[tokio::test]
    async fn test_control_leaf_node_output_is_none() {
        let provider = Arc::new(SequenceProvider {
            calls: std::sync::atomic::AtomicUsize::new(0),
            fail_first: 0,
            fail_models: vec![],
            content: "unused".into(),
            recorded_models: std::sync::Mutex::new(Vec::new()),
        });
        let executor = DefaultExecutor::new(provider.clone(), HashMap::new());
        let mut node = make_single_leaf(HashMap::new());
        node.kind = ExecutionNodeKind::Gate;

        let result = executor
            .execute_node(&node, &NodeExecContext::default())
            .await;

        assert_eq!(result.state, NodeState::Succeeded);
        assert_eq!(
            result.output, None,
            "control nodes must keep the src contract of no output"
        );
        assert_eq!(provider.calls.load(std::sync::atomic::Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn test_legacy_path_retries_then_succeeds() {
        // A tool allowlist routes execution to the legacy Law 7 path, whose
        // retry loop moved in from the crates adapter (Phase 6.4).
        let provider = Arc::new(SequenceProvider {
            calls: std::sync::atomic::AtomicUsize::new(0),
            fail_first: 2,
            fail_models: vec![],
            content: "legacy recovered".into(),
            recorded_models: std::sync::Mutex::new(Vec::new()),
        });
        let executor = DefaultExecutor::new(provider.clone(), HashMap::new());
        let mut node = make_single_leaf(HashMap::from([(
            "tool_allowlist".into(),
            serde_json::json!(["calculator"]),
        )]));
        node.retry_policy = RetryPolicy {
            max_retries: 2,
            backoff_ms: 0,
        };

        let result = executor
            .execute_node(&node, &NodeExecContext::default())
            .await;

        assert_eq!(result.state, NodeState::Succeeded);
        assert_eq!(
            provider.calls.load(std::sync::atomic::Ordering::SeqCst),
            3,
            "2 failures + 1 success via the legacy retry loop"
        );
    }

    #[tokio::test]
    async fn test_legacy_path_fallback_after_exhausted_retries() {
        let provider = Arc::new(SequenceProvider {
            calls: std::sync::atomic::AtomicUsize::new(0),
            fail_first: 0,
            fail_models: vec!["gpt-4".to_string(), "backup-model".to_string()],
            content: "never".into(),
            recorded_models: std::sync::Mutex::new(Vec::new()),
        });
        let executor = DefaultExecutor::new(provider.clone(), HashMap::new());
        let mut node = make_single_leaf(HashMap::from([(
            "tool_allowlist".into(),
            serde_json::json!(["calculator"]),
        )]));
        node.retry_policy = RetryPolicy {
            max_retries: 1,
            backoff_ms: 0,
        };
        node.fallback = Some(FallbackConfig {
            model: "backup-model".into(),
            provider: "backup".into(),
        });

        let result = executor
            .execute_node(&node, &NodeExecContext::default())
            .await;

        assert!(matches!(
            result.state,
            NodeState::Failed(reason) if reason.starts_with("Fallback failed:")
        ));
        let models = provider.recorded_models.lock().unwrap().clone();
        assert_eq!(models.len(), 3, "2 primary attempts + 1 fallback");
        assert_eq!(models[2], "backup-model");
    }

    #[tokio::test]
    async fn test_legacy_path_never_retries_cancellation_marker() {
        struct CancelledProvider(std::sync::atomic::AtomicUsize);
        #[async_trait]
        impl ChatProvider for CancelledProvider {
            fn name(&self) -> &str {
                "cancelled"
            }
            async fn chat_completion(
                &self,
                _request: &ChatCompletionRequest,
            ) -> anyhow::Result<ChatCompletionResponse> {
                self.0.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                Err(anyhow::anyhow!("Cancelled by client"))
            }
        }

        let provider = Arc::new(CancelledProvider(std::sync::atomic::AtomicUsize::new(0)));
        let executor = DefaultExecutor::new(provider.clone(), HashMap::new());
        let mut node = make_single_leaf(HashMap::from([(
            "tool_allowlist".into(),
            serde_json::json!(["calculator"]),
        )]));
        node.retry_policy = RetryPolicy {
            max_retries: 5,
            backoff_ms: 0,
        };

        let result = executor
            .execute_node(&node, &NodeExecContext::default())
            .await;

        assert!(matches!(
            result.state,
            NodeState::Failed(reason) if reason.contains("Cancelled by client")
        ));
        assert_eq!(
            provider.0.load(std::sync::atomic::Ordering::SeqCst),
            1,
            "cancellation must never be retried"
        );
    }

    #[test]
    fn test_routing_split_single_vs_legacy() {
        let provider = Arc::new(SequenceProvider {
            calls: std::sync::atomic::AtomicUsize::new(0),
            fail_first: 0,
            fail_models: vec![],
            content: "ok".into(),
            recorded_models: std::sync::Mutex::new(Vec::new()),
        });
        let executor = DefaultExecutor::new(provider, HashMap::new());

        // 1. Plain Single leaf -> delegates to crates runtime
        let plain_single = make_single_leaf(HashMap::new());
        assert!(
            executor.delegate_to_crates(&plain_single),
            "plain single leaf must delegate to crates runtime"
        );

        // 2. Node with tool allowlist -> stays on host legacy path
        let tool_node = make_single_leaf(HashMap::from([(
            "tool_allowlist".into(),
            serde_json::json!(["search"]),
        )]));
        assert!(
            !executor.delegate_to_crates(&tool_node),
            "tool-enabled node must stay on host legacy path"
        );

        // 3. Multi-model strategy -> stays on host legacy path
        let mut consensus_node = make_single_leaf(HashMap::new());
        consensus_node.strategy = StrategyKind::Consensus;
        assert!(
            !executor.delegate_to_crates(&consensus_node),
            "consensus strategy must stay on host legacy path"
        );

        // 4. Subgraph node -> stays on host legacy path
        let mut subgraph_node = make_single_leaf(HashMap::new());
        subgraph_node.subgraph = Some(crate::types::ExecutionSubgraph {
            nodes: vec![],
            edges: vec![],
            entry_node_id: Uuid::new_v4(),
            exit_node_id: Uuid::new_v4(),
        });
        assert!(
            !executor.delegate_to_crates(&subgraph_node),
            "subgraph node must stay on host legacy path"
        );
    }
}
