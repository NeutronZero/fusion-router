use parking_lot::RwLock;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use crate::capability::CapabilityRegistry;
use crate::config::manager::ConfigManager;
use crate::config::AppConfig;
use crate::context::assembler::DefaultContextAssembler;
use crate::executor::DefaultExecutor;
use crate::planner::Planner;
use crate::providers::ChatProvider;
use crate::requirements::extractor::DefaultRequirementsExtractor;
use crate::resource::DefaultResourceManager;
use crate::scheduler::connector_resolver::ConnectorResolver;
use crate::scheduler::default::DefaultScheduler;
use crate::strategies::chain::ChainStrategy;
use crate::strategies::consensus::ConsensusStrategy;
use crate::strategies::debate::DebateStrategy;
use crate::strategies::fusion::FusionStrategy;
use crate::strategies::react::ReActStrategy;
use crate::strategies::reflection::ReflectionStrategy;
use crate::strategies::single::SingleStrategy;
use crate::strategies::Strategy;
use crate::telemetry::EvidenceRepository;
use crate::tools::builtin::{CalculatorTool, FileReadTool, SearchTool};
use crate::tools::{HTTPRequestTool, ShellCommandTool, ToolRegistry};
use crate::types::*;

#[derive(Clone)]
pub struct AppState {
    pub context_assembler: Arc<DefaultContextAssembler>,
    pub requirements_extractor: Arc<DefaultRequirementsExtractor>,
    pub planner: Arc<dyn Planner + Send + Sync>,
    pub scheduler: Arc<DefaultScheduler>,
    pub executor: Arc<DefaultExecutor>,
    pub resource_manager: Arc<DefaultResourceManager>,
    pub evidence_repository: Arc<dyn EvidenceRepository + Send + Sync>,
    pub provider: Arc<dyn ChatProvider + Send + Sync>,
    pub config_manager: Arc<ConfigManager>,
    pub tool_registry: Arc<ToolRegistry>,
    pub connector_resolver: Arc<ConnectorResolver>,
    pub policy_registry: Arc<crate::policy::PolicyRegistry>,
    pub capability_registry: Arc<dyn CapabilityRegistry>,
    /// Model catalog used to rebuild the compiler per request so the policy
    /// pass always reflects the live registry snapshot.
    pub model_catalog: crate::types::ModelCatalog,
    /// Concrete provider registry enabling native upstream streaming.
    pub provider_registry: Option<Arc<crate::providers::registry::ProviderRegistry>>,
    planner_capability_snapshot: Arc<RwLock<fusion_kernel::CapabilityCatalog>>,
}

impl AppState {
    pub fn new(
        provider: Arc<dyn ChatProvider + Send + Sync>,
        resource_manager: DefaultResourceManager,
        evidence_repository: Arc<dyn EvidenceRepository + Send + Sync>,
        config: AppConfig,
        config_path: PathBuf,
        connector_resolver: Arc<ConnectorResolver>,
    ) -> Self {
        let context_assembler = Arc::new(DefaultContextAssembler::new());
        let requirements_extractor = Arc::new(DefaultRequirementsExtractor);

        let intent_planner = crate::planner::IntentPlanner::new(config.model_catalog.clone());
        let planner_capability_snapshot = intent_planner.capability_snapshot.clone();
        let planner: Arc<dyn Planner + Send + Sync> = Arc::new(intent_planner);

        let resource_manager = Arc::new(resource_manager);

        let mut strategies: HashMap<StrategyKind, Box<dyn Strategy + Send + Sync>> = HashMap::new();
        strategies.insert(StrategyKind::Single, Box::new(SingleStrategy));
        strategies.insert(
            StrategyKind::Consensus,
            Box::new(ConsensusStrategy {
                count: config.strategies.consensus_count,
            }),
        );
        strategies.insert(
            StrategyKind::Reflection,
            Box::new(ReflectionStrategy::default()),
        );
        strategies.insert(
            StrategyKind::Chain,
            Box::new(ChainStrategy {
                stages: vec![
                    Box::new(SingleStrategy),
                    Box::new(ReflectionStrategy::default()),
                ],
            }),
        );

        let mut tool_registry = ToolRegistry::new();
        tool_registry.register(Arc::new(CalculatorTool));
        tool_registry.register(Arc::new(SearchTool));
        for dir in &config.tools.allowed_read_directories {
            tool_registry.register(Arc::new(FileReadTool::new(dir.clone())));
        }
        if config.tools.enable_http_tool {
            tool_registry.register(Arc::new(HTTPRequestTool::new()));
        }
        tool_registry.register(Arc::new(
            ShellCommandTool::new(
                config.tools.allowed_shell_commands.clone(),
                config.tools.shell_timeout_secs,
                config.tools.allowed_read_directories.clone(),
                config.tools.allow_unrestricted_args,
            )
            .with_path_policy(
                crate::tools::ShellPathMode::from_config(&config.tools.shell_path_mode),
                config.tools.max_staged_input_bytes,
            ),
        ));
        let tool_registry = Arc::new(tool_registry);

        strategies.insert(
            StrategyKind::ReAct,
            Box::new(ReActStrategy::new(10, Some(tool_registry.clone()))),
        );
        strategies.insert(
            StrategyKind::Debate,
            Box::new(DebateStrategy {
                debaters: vec![Box::new(SingleStrategy), Box::new(SingleStrategy)],
                judge: Box::new(SingleStrategy),
            }),
        );
        strategies.insert(
            StrategyKind::Fusion,
            Box::new(FusionStrategy::new(vec![
                Box::new(SingleStrategy) as Box<dyn Strategy>,
                Box::new(ConsensusStrategy::default()) as Box<dyn Strategy>,
            ])),
        );

        let executor = Arc::new(
            DefaultExecutor::new(provider.clone(), strategies)
                .with_tool_registry(tool_registry.clone())
                .with_allow_auto_exec(config.tools.allow_auto_exec),
        );

        let scheduler = Arc::new(DefaultScheduler::new(
            (config.resources.max_concurrent_nodes as usize).max(1),
        ));

        let config_manager = Arc::new(ConfigManager::new(config_path, config.clone(), vec![]));
        let policy_registry = Arc::new(crate::policy::PolicyRegistry::new());

        Self {
            context_assembler,
            requirements_extractor,
            planner,
            scheduler,
            executor,
            resource_manager,
            evidence_repository,
            provider,
            config_manager,
            tool_registry,
            connector_resolver,
            policy_registry,
            capability_registry: Arc::new(crate::capability::InMemoryCapabilityRegistry::new()),
            model_catalog: config.model_catalog.clone(),
            provider_registry: None,
            planner_capability_snapshot,
        }
    }

    /// Builds a compiler whose pass pipeline includes the live policy snapshot.
    ///
    /// `Some(ir)` appends the policy pass (deny ⇒ compile error, approval ⇒
    /// gate insertion); `None` (no active policies) yields the mandatory base
    /// passes only. Built per request so registry mutations take effect
    /// immediately without a restart.
    pub fn compiler_with_policies(
        &self,
        policy_ir: Option<crate::policy::ir::PolicyIR>,
    ) -> Arc<dyn crate::compiler::Compiler + Send + Sync> {
        Arc::new(crate::compiler::build_compiler(
            self.model_catalog.clone(),
            self.resource_manager.clone(),
            policy_ir,
        ))
    }

    pub fn with_policy_registry(
        mut self,
        policy_registry: Arc<crate::policy::PolicyRegistry>,
    ) -> Self {
        self.policy_registry = policy_registry;
        self
    }

    /// Attaches the concrete provider registry, enabling native upstream
    /// streaming for eligible single-node graphs.
    pub fn with_provider_registry(
        mut self,
        registry: Arc<crate::providers::registry::ProviderRegistry>,
    ) -> Self {
        self.provider_registry = Some(registry);
        self
    }

    pub fn with_capability_registry(mut self, registry: Arc<dyn CapabilityRegistry>) -> Self {
        let mut catalog = std::collections::HashMap::new();
        for contract in registry.list() {
            catalog.insert(contract.id.as_str().to_string(), Vec::new());
        }
        *self.planner_capability_snapshot.write() = fusion_kernel::CapabilityCatalog { catalog };
        self.capability_registry = registry;
        self
    }
}
