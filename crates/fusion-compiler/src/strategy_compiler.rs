//! Phase C: Strategy Lowering Pass.
//!
//! Validates that every non-Single strategy kind in the IR has a supported
//! expansion path. Runs as a `CompilerPass` before `lower_to_graph` to
//! guarantee that zero passthrough fallbacks survive compilation.

use fusion_types::*;
use std::collections::HashSet;

/// Delegate compiler trait for lowering custom strategies into executable subgraphs.
pub trait StrategyCompiler: Send + Sync {
    fn compile_subgraph(&self, node: &ExecutionNode, custom_name: &str) -> ExecutionSubgraph;
}

/// A default strategy compiler that emits a delegate execution subgraph.
#[derive(Debug, Default, Clone)]
pub struct DefaultCustomStrategyCompiler;

impl StrategyCompiler for DefaultCustomStrategyCompiler {
    fn compile_subgraph(&self, node: &ExecutionNode, custom_name: &str) -> ExecutionSubgraph {
        crate::strategy_expansion::expand_custom(node, custom_name)
    }
}

/// Validates and annotates strategy kinds across the IR.
///
/// This pass ensures that:
/// - All strategy kinds are one of the 8 supported variants (Single, Consensus,
///   Reflection, Chain, Debate, ReAct, Fusion, Custom)
/// - Every non-Single strategy has the required config keys
/// - No unknown strategy kinds slip through with silent degradation
///
/// The pass does not transform the IR structurally — it only rejects invalid
/// configurations at compile time rather than allowing runtime fallback.
#[derive(Default)]
pub struct StrategyLoweringPass {
    registered_custom: HashSet<String>,
    custom_compilers: std::collections::HashMap<String, std::sync::Arc<dyn StrategyCompiler>>,
}

impl StrategyLoweringPass {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_registered_custom(registered_custom: HashSet<String>) -> Self {
        Self {
            registered_custom,
            custom_compilers: std::collections::HashMap::new(),
        }
    }

    pub fn register_compiler(
        mut self,
        name: impl Into<String>,
        compiler: std::sync::Arc<dyn StrategyCompiler>,
    ) -> Self {
        let n = name.into();
        self.registered_custom.insert(n.clone());
        self.custom_compilers.insert(n, compiler);
        self
    }
}

#[async_trait::async_trait]
impl crate::CompilerPass for StrategyLoweringPass {
    fn name(&self) -> &str {
        "strategy_lowering"
    }

    async fn apply(&self, ir: WorkflowIR) -> Result<WorkflowIR, crate::CompilerError> {
        for node in &ir.nodes {
            match &node.strategy {
                StrategyKind::Single => { /* baseline — always valid */ }

                StrategyKind::Consensus => {
                    validate_consensus_config(node)?;
                }

                StrategyKind::Reflection => {
                    validate_reflection_config(node)?;
                }

                StrategyKind::Chain => {
                    validate_chain_config(node)?;
                }

                StrategyKind::Debate => {
                    validate_debate_config(node)?;
                }

                StrategyKind::ReAct => {
                    validate_react_config(node)?;
                }

                StrategyKind::Fusion => {
                    validate_fusion_config(node)?;
                }

                StrategyKind::Custom(name) => {
                    if name.is_empty() {
                        return Err(crate::CompilerError::ValidationError {
                            pass: "strategy_lowering".into(),
                            node_id: Some(node.id),
                            message: "Custom strategy must have a non-empty name".into(),
                        });
                    }
                    if !self.registered_custom.contains(name) {
                        return Err(crate::CompilerError::ValidationError {
                            pass: "strategy_lowering".into(),
                            node_id: Some(node.id),
                            message: format!("Unregistered custom strategy '{}' has no registered StrategyCompiler delegate; compilation failed closed", name),
                        });
                    }
                }
            }
        }

        Ok(ir)
    }
}

fn validate_consensus_config(node: &IRNode) -> Result<(), crate::CompilerError> {
    const MAX: u64 = crate::strategy_expansion::MAX_CONSENSUS_MEMBERS;
    if let Some(count_val) = node.config.get("count") {
        if let Some(count) = count_val.as_u64() {
            if count == 0 {
                return Err(crate::CompilerError::ValidationError {
                    pass: "strategy_lowering".into(),
                    node_id: Some(node.id),
                    message: "Consensus count must be >= 1".into(),
                });
            }
            if count > MAX {
                return Err(crate::CompilerError::ValidationError {
                    pass: "strategy_lowering".into(),
                    node_id: Some(node.id),
                    message: format!("Consensus count {count} exceeds maximum of {MAX} members"),
                });
            }
        }
    }
    if let Some(members_val) = node.config.get("members") {
        if let Some(members) = members_val.as_array() {
            if members.len() as u64 > MAX {
                return Err(crate::CompilerError::ValidationError {
                    pass: "strategy_lowering".into(),
                    node_id: Some(node.id),
                    message: format!(
                        "Consensus members list length {} exceeds maximum of {MAX} members",
                        members.len()
                    ),
                });
            }
        }
    }
    Ok(())
}

fn validate_reflection_config(node: &IRNode) -> Result<(), crate::CompilerError> {
    if let Some(cycles_val) = node.config.get("max_cycles") {
        if let Some(cycles) = cycles_val.as_u64() {
            if cycles == 0 {
                return Err(crate::CompilerError::ValidationError {
                    pass: "strategy_lowering".into(),
                    node_id: Some(node.id),
                    message: "Reflection max_cycles must be >= 1".into(),
                });
            }
        }
    }
    Ok(())
}

/// Validates Chain config in lockstep with `expand_chain` (see
/// [`crate::strategy_expansion::resolved_chain_steps`]):
/// - a `stages` array (when present) drives the pipeline length and must hold
///   `1..=MAX_CHAIN_STEPS` entries;
/// - otherwise numeric `steps` must be >= 1 (`steps: 0` is rejected instead of
///   producing an empty/dangling subgraph); oversized values are clamped by
///   expansion, not rejected.
fn validate_chain_config(node: &IRNode) -> Result<(), crate::CompilerError> {
    const MAX_STEPS: u64 = crate::strategy_expansion::MAX_CHAIN_STEPS;
    if let Some(stages_val) = node.config.get("stages") {
        let stages =
            stages_val
                .as_array()
                .ok_or_else(|| crate::CompilerError::ValidationError {
                    pass: "strategy_lowering".into(),
                    node_id: Some(node.id),
                    message: "Chain stages must be an array".into(),
                })?;
        if stages.is_empty() {
            return Err(crate::CompilerError::ValidationError {
                pass: "strategy_lowering".into(),
                node_id: Some(node.id),
                message: "Chain stages must not be empty".into(),
            });
        }
        if stages.len() as u64 > MAX_STEPS {
            return Err(crate::CompilerError::ValidationError {
                pass: "strategy_lowering".into(),
                node_id: Some(node.id),
                message: format!(
                    "Chain stages length {} exceeds maximum of {MAX_STEPS} steps",
                    stages.len()
                ),
            });
        }
        return Ok(());
    }

    if let Some(steps_val) = node.config.get("steps") {
        let steps = steps_val
            .as_u64()
            .ok_or_else(|| crate::CompilerError::ValidationError {
                pass: "strategy_lowering".into(),
                node_id: Some(node.id),
                message: "Chain steps must be a non-negative integer".into(),
            })?;
        if steps == 0 {
            return Err(crate::CompilerError::ValidationError {
                pass: "strategy_lowering".into(),
                node_id: Some(node.id),
                message:
                    "Chain steps must be >= 1; a zero-step chain would produce an empty subgraph"
                        .into(),
            });
        }
    }
    Ok(())
}

fn validate_debate_config(node: &IRNode) -> Result<(), crate::CompilerError> {
    if let Some(roles_val) = node.config.get("roles") {
        if let Some(roles) = roles_val.as_array() {
            if roles.len() < 2 {
                return Err(crate::CompilerError::ValidationError {
                    pass: "strategy_lowering".into(),
                    node_id: Some(node.id),
                    message: format!("Debate requires at least 2 roles, got {}", roles.len()),
                });
            }
        }
    }
    Ok(())
}

fn validate_react_config(node: &IRNode) -> Result<(), crate::CompilerError> {
    if let Some(rounds_val) = node.config.get("max_tool_rounds") {
        if let Some(rounds) = rounds_val.as_u64() {
            if rounds == 0 {
                return Err(crate::CompilerError::ValidationError {
                    pass: "strategy_lowering".into(),
                    node_id: Some(node.id),
                    message: "ReAct max_tool_rounds must be >= 1".into(),
                });
            }
        }
    }
    Ok(())
}

fn validate_fusion_config(_node: &IRNode) -> Result<(), crate::CompilerError> {
    // Fusion uses hardcoded expansion — no user config required.
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::CompilerPass;
    use fusion_core::NanoUSD;
    use std::collections::HashMap;

    fn ir_with_strategy(
        strategy: StrategyKind,
        config: HashMap<String, serde_json::Value>,
    ) -> WorkflowIR {
        WorkflowIR {
            plan_id: uuid::Uuid::new_v4(),
            nodes: vec![IRNode {
                id: uuid::Uuid::new_v4(),
                kind: IRNodeKind::Generate,
                strategy,
                model: Some("test-model".into()),
                config,
            }],
            edges: vec![],
            metadata: IRMetadata {
                policy_version: 0,
                policy_applied: vec![],
                estimated_cost: NanoUSD::from_nanos(10_000_000),
                estimated_tokens: 100,
            },
        }
    }

    #[tokio::test]
    async fn single_always_valid() {
        let pass = StrategyLoweringPass::new();
        let ir = ir_with_strategy(StrategyKind::Single, HashMap::new());
        assert!(pass.apply(ir).await.is_ok());
    }

    #[tokio::test]
    async fn consensus_valid() {
        let pass = StrategyLoweringPass::new();
        let mut config = HashMap::new();
        config.insert("count".into(), serde_json::json!(3));
        let ir = ir_with_strategy(StrategyKind::Consensus, config);
        assert!(pass.apply(ir).await.is_ok());
    }

    #[tokio::test]
    async fn consensus_zero_count_rejected() {
        let pass = StrategyLoweringPass::new();
        let mut config = HashMap::new();
        config.insert("count".into(), serde_json::json!(0));
        let ir = ir_with_strategy(StrategyKind::Consensus, config);
        let err = pass.apply(ir).await.unwrap_err();
        assert!(err.to_string().contains("count"));
    }

    #[tokio::test]
    async fn consensus_oversized_count_rejected() {
        let pass = StrategyLoweringPass::new();
        let max = crate::strategy_expansion::MAX_CONSENSUS_MEMBERS;
        let mut config = HashMap::new();
        config.insert("count".into(), serde_json::json!(max + 1));
        let ir = ir_with_strategy(StrategyKind::Consensus, config);
        let err = pass.apply(ir).await.unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("exceeds maximum"),
            "oversized count must fail validation with a clear error: {msg}"
        );
        // A pathological value must also be rejected, never allocated.
        let mut config = HashMap::new();
        config.insert("count".into(), serde_json::json!(u64::MAX));
        let ir = ir_with_strategy(StrategyKind::Consensus, config);
        assert!(pass.apply(ir).await.is_err());
    }

    #[tokio::test]
    async fn consensus_count_at_bound_valid() {
        let pass = StrategyLoweringPass::new();
        let max = crate::strategy_expansion::MAX_CONSENSUS_MEMBERS;
        let mut config = HashMap::new();
        config.insert("count".into(), serde_json::json!(max));
        let ir = ir_with_strategy(StrategyKind::Consensus, config);
        assert!(pass.apply(ir).await.is_ok());
    }

    #[tokio::test]
    async fn consensus_oversized_members_list_rejected() {
        let pass = StrategyLoweringPass::new();
        let max = crate::strategy_expansion::MAX_CONSENSUS_MEMBERS as usize;
        let members: Vec<&str> = (0..max + 1).map(|_| "m").collect();
        let mut config = HashMap::new();
        config.insert("members".into(), serde_json::json!(members));
        let ir = ir_with_strategy(StrategyKind::Consensus, config);
        let err = pass.apply(ir).await.unwrap_err();
        assert!(
            err.to_string().contains("members list length"),
            "oversized members list must fail validation: {err}"
        );
    }

    #[tokio::test]
    async fn reflection_valid() {
        let pass = StrategyLoweringPass::new();
        let mut config = HashMap::new();
        config.insert("max_cycles".into(), serde_json::json!(3));
        let ir = ir_with_strategy(StrategyKind::Reflection, config);
        assert!(pass.apply(ir).await.is_ok());
    }

    #[tokio::test]
    async fn reflection_zero_cycles_rejected() {
        let pass = StrategyLoweringPass::new();
        let mut config = HashMap::new();
        config.insert("max_cycles".into(), serde_json::json!(0));
        let ir = ir_with_strategy(StrategyKind::Reflection, config);
        let err = pass.apply(ir).await.unwrap_err();
        assert!(err.to_string().contains("max_cycles"));
    }

    #[tokio::test]
    async fn chain_empty_stages_rejected() {
        let pass = StrategyLoweringPass::new();
        let mut config = HashMap::new();
        config.insert("stages".into(), serde_json::json!([]));
        let ir = ir_with_strategy(StrategyKind::Chain, config);
        let err = pass.apply(ir).await.unwrap_err();
        assert!(err.to_string().contains("stages"));
    }

    #[tokio::test]
    async fn chain_non_array_stages_rejected() {
        let pass = StrategyLoweringPass::new();
        let mut config = HashMap::new();
        config.insert("stages".into(), serde_json::json!("not-an-array"));
        let ir = ir_with_strategy(StrategyKind::Chain, config);
        let err = pass.apply(ir).await.unwrap_err();
        assert!(
            err.to_string().contains("must be an array"),
            "malformed stages must be rejected: {err}"
        );
    }

    #[tokio::test]
    async fn chain_stages_array_valid_and_length_capped() {
        let pass = StrategyLoweringPass::new();

        let mut config = HashMap::new();
        config.insert(
            "stages".into(),
            serde_json::json!(["draft", "critique", "refine"]),
        );
        let ir = ir_with_strategy(StrategyKind::Chain, config);
        assert!(pass.apply(ir).await.is_ok());

        let max = crate::strategy_expansion::MAX_CHAIN_STEPS as usize;
        let stages: Vec<&str> = (0..max + 1).map(|_| "s").collect();
        let mut config = HashMap::new();
        config.insert("stages".into(), serde_json::json!(stages));
        let ir = ir_with_strategy(StrategyKind::Chain, config);
        let err = pass.apply(ir).await.unwrap_err();
        assert!(
            err.to_string().contains("exceeds maximum"),
            "stages longer than the cap must be rejected: {err}"
        );
    }

    #[tokio::test]
    async fn chain_zero_steps_rejected() {
        let pass = StrategyLoweringPass::new();
        let mut config = HashMap::new();
        config.insert("steps".into(), serde_json::json!(0));
        let ir = ir_with_strategy(StrategyKind::Chain, config);
        let err = pass.apply(ir).await.unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("steps"), "error must mention steps: {msg}");
        assert!(
            msg.contains(">= 1"),
            "zero steps must be a validation error, not an empty subgraph: {msg}"
        );
    }

    #[tokio::test]
    async fn chain_oversized_steps_accepted_then_clamped_by_expansion() {
        // Validation accepts oversized numeric steps (they are clamped
        // downstream by expand_chain) but still rejects zero.
        let pass = StrategyLoweringPass::new();
        let mut config = HashMap::new();
        config.insert("steps".into(), serde_json::json!(u64::MAX));
        let ir = ir_with_strategy(StrategyKind::Chain, config.clone());
        assert!(
            pass.apply(ir).await.is_ok(),
            "oversized numeric steps are clamped, not rejected"
        );

        let node = ExecutionNode {
            id: uuid::Uuid::new_v4(),
            kind: fusion_types::ExecutionNodeKind::LLMGenerate,
            strategy: StrategyKind::Chain,
            model: "m".into(),
            retry_policy: RetryPolicy {
                max_retries: 1,
                backoff_ms: 10,
            },
            fallback: None,
            config,
            subgraph: None,
        };
        let subgraph = crate::strategy_expansion::expand_chain(&node);
        assert_eq!(
            subgraph.nodes.len(),
            crate::strategy_expansion::MAX_CHAIN_STEPS as usize,
            "steps must clamp to the upper bound"
        );
    }

    #[tokio::test]
    async fn debate_too_few_roles_rejected() {
        let pass = StrategyLoweringPass::new();
        let mut config = HashMap::new();
        config.insert("roles".into(), serde_json::json!(["only_one"]));
        let ir = ir_with_strategy(StrategyKind::Debate, config);
        let err = pass.apply(ir).await.unwrap_err();
        assert!(err.to_string().contains("2 roles"));
    }

    #[tokio::test]
    async fn debate_two_roles_valid() {
        let pass = StrategyLoweringPass::new();
        let mut config = HashMap::new();
        config.insert(
            "roles".into(),
            serde_json::json!([
                {"name": "A", "model": "m1", "stance": "pro"},
                {"name": "B", "model": "m2", "stance": "con"}
            ]),
        );
        let ir = ir_with_strategy(StrategyKind::Debate, config);
        assert!(pass.apply(ir).await.is_ok());
    }

    #[tokio::test]
    async fn react_zero_rounds_rejected() {
        let pass = StrategyLoweringPass::new();
        let mut config = HashMap::new();
        config.insert("max_tool_rounds".into(), serde_json::json!(0));
        let ir = ir_with_strategy(StrategyKind::ReAct, config);
        let err = pass.apply(ir).await.unwrap_err();
        assert!(err.to_string().contains("max_tool_rounds"));
    }

    #[tokio::test]
    async fn fusion_always_valid() {
        let pass = StrategyLoweringPass::new();
        let ir = ir_with_strategy(StrategyKind::Fusion, HashMap::new());
        assert!(pass.apply(ir).await.is_ok());
    }

    #[tokio::test]
    async fn custom_empty_name_rejected() {
        let pass = StrategyLoweringPass::new();
        let ir = ir_with_strategy(StrategyKind::Custom(String::new()), HashMap::new());
        let err = pass.apply(ir).await.unwrap_err();
        assert!(err.to_string().contains("non-empty"));
    }

    #[tokio::test]
    async fn custom_unregistered_rejected() {
        let pass = StrategyLoweringPass::new();
        let ir = ir_with_strategy(
            StrategyKind::Custom("unregistered_strat".into()),
            HashMap::new(),
        );
        let err = pass.apply(ir).await.unwrap_err();
        assert!(err.to_string().contains("Unregistered custom strategy"));
    }

    #[tokio::test]
    async fn custom_registered_valid() {
        let mut set = HashSet::new();
        set.insert("my_strategy".into());
        let pass = StrategyLoweringPass::with_registered_custom(set);
        let ir = ir_with_strategy(StrategyKind::Custom("my_strategy".into()), HashMap::new());
        assert!(pass.apply(ir).await.is_ok());
    }
}
