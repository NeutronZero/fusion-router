use async_trait::async_trait;

pub mod capability_executor;
mod fusion_bridge;
mod node_exec;
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
    //! Host executor tests.
    //!
    //! Production executor semantics live in `fusion-runtime`. These tests
    //! cover only the host adapter boundary and two regression cases that
    //! have no runtime equivalent yet (cache subgraph, usage-on-failure).

    use super::*;
    use crate::providers::ChatProvider;
    use crate::strategies::consensus::ConsensusStrategy;
    use crate::strategies::Strategy;
    use crate::types::*;
    use std::collections::HashMap;
    use std::sync::Arc;
    use uuid::Uuid;

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

    // -----------------------------------------------------------------------
    // Cache subgraph regression — no runtime equivalent yet.
    // -----------------------------------------------------------------------

    mod cache_tests {
        use super::*;
        use crate::cache::embeddings::Embedder;
        use crate::cache::SemanticCache;

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
            let cache_key = format!(
                "{}:{}",
                member_request.model,
                serde_json::to_string(&member_request.messages).unwrap_or_default()
            );

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

    // -----------------------------------------------------------------------
    // Usage-on-failure regression — no runtime equivalent yet.
    // -----------------------------------------------------------------------

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
            Box::new(ConsensusStrategy::default()),
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
}
