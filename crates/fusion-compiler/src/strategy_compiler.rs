use fusion_core::PlatformError;
use fusion_types::*;

pub trait CustomStrategyCompiler: Send + Sync {
    fn compile_custom(&self, node: &ExecutionNode) -> Result<ExecutionSubgraph, PlatformError>;
}

pub struct StrategyCompiler {
    pub custom_compiler: Option<Box<dyn CustomStrategyCompiler>>,
}

impl Default for StrategyCompiler {
    fn default() -> Self {
        Self::new(None)
    }
}

impl StrategyCompiler {
    pub fn new(custom_compiler: Option<Box<dyn CustomStrategyCompiler>>) -> Self {
        Self { custom_compiler }
    }

    pub fn compile_strategy(&self, node: &ExecutionNode) -> Result<Option<ExecutionSubgraph>, PlatformError> {
        match &node.strategy {
            StrategyKind::Single => Ok(None),
            StrategyKind::Consensus => Ok(Some(crate::strategy_expansion::expand_consensus(node))),
            StrategyKind::Reflection => Ok(Some(crate::strategy_expansion::expand_reflection(node))),
            StrategyKind::Chain => Ok(Some(crate::strategy_expansion::expand_chain(node))),
            StrategyKind::Debate => Ok(Some(crate::strategy_expansion::expand_debate(node))),
            StrategyKind::ReAct => Ok(Some(crate::strategy_expansion::expand_react(node))),
            StrategyKind::Fusion => Ok(Some(crate::strategy_expansion::expand_fusion(node))),
            StrategyKind::Custom(custom_name) => {
                if let Some(custom) = &self.custom_compiler {
                    custom.compile_custom(node).map(Some)
                } else {
                    Ok(Some(crate::strategy_expansion::expand_custom(node, custom_name)))
                }
            }
        }
    }
}
