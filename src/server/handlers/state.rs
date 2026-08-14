use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use crate::compiler::DefaultCompiler;
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
use crate::workflow::WorkflowRegistry;
use crate::capability::CapabilityRegistry;

#[derive(Clone)]
pub struct AppState {
    pub context_assembler: Arc<DefaultContextAssembler>,
    pub requirements_extractor: Arc<DefaultRequirementsExtractor>,
    pub planner: Arc<dyn Planner + Send + Sync>,
    pub compiler: Arc<DefaultCompiler>,
    pub scheduler: Arc<DefaultScheduler>,
    pub executor: Arc<DefaultExecutor>,
    pub resource_manager: Arc<DefaultResourceManager>,
    pub evidence_repository: Arc<dyn EvidenceRepository + Send + Sync>,
    pub provider: Arc<dyn ChatProvider + Send + Sync>,
    pub config_manager: Arc<ConfigManager>,
    pub workflow_registry: Arc<WorkflowRegistry>,
    pub tool_registry: Arc<ToolRegistry>,
    pub connector_resolver: Arc<ConnectorResolver>,
    pub policy_registry: Arc<crate::policy::PolicyRegistry>,
    pub capability_registry: Arc<dyn CapabilityRegistry>,
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

        let mut workflow_registry = WorkflowRegistry::new();
        let _ = workflow_registry.load_dir("workflows");
        let workflow_registry = Arc::new(workflow_registry);

        let capability_catalog =
            crate::providers::capability_catalog::CapabilityCatalog::from_config(&config);
        let planner: Arc<dyn Planner + Send + Sync> = Arc::new(
            crate::planner::IntentPlanner::with_capability_catalog(
                config.model_catalog.clone(),
                capability_catalog,
            ),
        );

        let resource_manager = Arc::new(resource_manager);

        let compiler = Arc::new(crate::compiler::build_compiler(
            config.model_catalog.clone(),
            resource_manager.clone(),
            None,
        ));

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
        tool_registry.register(Arc::new(ShellCommandTool::new(
            config.tools.allowed_shell_commands.clone(),
            config.tools.shell_timeout_secs,
            config.tools.allowed_read_directories.clone(),
            config.tools.allow_unrestricted_args,
        )));
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
            config.resources.max_concurrent_nodes as usize,
        ));

        let config_manager = Arc::new(ConfigManager::new(config_path, config, vec![]));
        let policy_registry = Arc::new(crate::policy::PolicyRegistry::new());

        Self {
            context_assembler,
            requirements_extractor,
            planner,
            compiler,
            scheduler,
            executor,
            resource_manager,
            evidence_repository,
            provider,
            config_manager,
            workflow_registry,
            tool_registry,
            connector_resolver,
            policy_registry,
            capability_registry: Arc::new(crate::capability::InMemoryCapabilityRegistry::new()),
        }
    }

    pub fn with_policy_registry(mut self, policy_registry: Arc<crate::policy::PolicyRegistry>) -> Self {
        self.policy_registry = policy_registry;
        self
    }

    pub fn with_capability_registry(mut self, registry: Arc<dyn CapabilityRegistry>) -> Self {
        self.capability_registry = registry;
        self
    }
}
