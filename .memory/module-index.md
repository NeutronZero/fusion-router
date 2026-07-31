# Module Index

> A searchable index of every significant type, trait, and component in FusionRouter.

---

## A

**AbiConstraints** — `src/abi/mod.rs` — ExecutionAbi node constraints
**AbiEdgeKind** — `src/abi/mod.rs` — ABI edge kinds (Sequential, Parallel, Conditional, Retry, Merge, Loop)
**AbiRetryPolicy** — `src/abi/mod.rs` — ABI node retry policy
**AuditLog** — `src/telemetry/audit.rs` — Structured audit logging for security events

## B

**Budget** — `src/intent/mod.rs` — NormalizedIntent budget constraints
**BroadcastEventBus** — `src/events/bus.rs` — In-memory broadcast event bus implementation
**BudgetEnvelope** — `src/resource/budget.rs` — Token/time budget enforcement
**BudgetOptimisation** — `src/compiler/passes/legacy_passes.rs` — Budget application compiler pass

## C

**CancellingStream** — `src/resource/cancelling_stream.rs` — Safe stream cancellation
**CachePolicy** — `src/abi/mod.rs` — ABI node cache policy
**CapabilityBuilder** — `crates/fusion-capability-sdk/src/builder.rs` — Builder for capability construction
**CapabilityContract** — `crates/fusion-plugin-api/src/lib.rs` — Declarative capability ABI contract
**CapabilityExecutor** (trait) — `crates/fusion-plugin-api/src/lib.rs` — Capability execution trait
**CapabilityExecutor** (module) — `src/executor/capability_executor.rs` — Unified capability dispatch
**CapabilityGraph** — `src/planner/resolver/capability/graph.rs` — Capability dependency DAG
**CapabilityHostServices** (trait) — `docs/adrs/adr-019-capability-host-interface.md` — WASM host service interface
**CapabilityId** — `crates/fusion-plugin-api/src/lib.rs` — Strongly-typed capability identifier
**CapabilityInstance** — `crates/fusion-plugin-api/src/lib.rs` — Bound runtime execution object
**CapabilityManifestBuilder** — `crates/fusion-capability-sdk/src/manifest.rs` — Plugin manifest builder
**CapabilityPlugin** (trait) — `crates/fusion-plugin-api/src/lib.rs` — Capability declaration trait
**CapabilityRegistry** — `src/capability/registry.rs` — Immutable capability registry (frozen after startup)
**CapabilityResolver** — `src/planner/resolver/capability/resolver.rs` — Resolves abstract capability references
**CapabilityTrait** — `crates/fusion-plugin-api/src/lib.rs` — Capability semantic traits (Streaming, LongContext, ...)
**CheckpointEngine** — `src/session/checkpoint.rs` — Session snapshot creation at intervals
**CircuitBreaker** — `src/providers/circuit_breaker.rs` — 3-state provider circuit breaker
**CircuitBreakingProvider** — `src/providers/circuit_breaking_provider.rs` — Provider wrapper with circuit breaking
**Compiler** (trait) — `src/compiler/mod.rs` — Core compiler interface
**CompilerPass** (trait) — `src/compiler/passes/mod.rs` — Compiler pass trait
**ConnectorResolver** — `src/scheduler/connector_resolver.rs` — Late-binding connector resolution
**ConnectorHealth** — `src/scheduler/connector_health.rs` — Connector health monitoring
**ConnectorSubscriber** — `src/scheduler/connector_subscriber.rs` — Connector event subscription
**ConstraintValidation** — `src/compiler/passes/legacy_passes.rs` — WorkflowIR structural validation pass
**Constraints** — `src/intent/mod.rs` — NormalizedIntent constraints
**ContextAssembler** — `src/context/assembler.rs` — ContextSnapshot construction
**ContextSnapshot** — `src/types/execution_context.rs` — Assembled execution context

## D

**DefaultCompiler** — `src/compiler/mod.rs` — Standard compiler implementation
**DefaultExecutor** — `src/executor/mod.rs` — Standard executor implementation
**DefaultScheduler** — `src/scheduler/default.rs` — Standard DAG scheduler
**DefaultResourceManager** — `src/resource/mod.rs` — Standard resource manager
**DistributedScheduler** — `src/scheduler/distributed.rs` — Remote worker pool scheduler
**DynamicPlanner** — `src/planner/dynamic_planner.rs` — LLM-powered WorkflowIR planner

## E

**EventBus** (trait) — `src/events/mod.rs` — Runtime event stream interface
**EriError** — `src/eri/mod.rs` — ERI error type
**EvaluationPolicy** — `src/abi/mod.rs` — ABI node evaluation policy
**EvidenceRepository** (trait) — `src/telemetry/mod.rs` — Evidence storage interface
**ExecutionAbi** — `src/abi/mod.rs` — Frozen executable workflow contract (v0.13)
**ExecutionAbiNode** — `src/abi/mod.rs` — ABI execution node contract
**ExecutionAbiResult** — `src/eri/mod.rs` — ERI execution result
**ExecutionEnvironment** — `src/target/mod.rs` — Runtime placement environments
**ExecutionError** — `crates/fusion-plugin-api/src/lib.rs` — Structured execution error
**ExecutionEventEnvelope** — `src/events/payload.rs` — Immutable runtime event envelope
**ExecutionGraph** — `src/types/execution.rs` — Compiled executable DAG
**ExecutionResult** — `crates/fusion-plugin-api/src/lib.rs` — Standardized execution output
**ExecutionRuntimeInterface** — `src/eri/mod.rs` — Runtime execution contract trait (v0.13)
**ExecutionRequest** — `src/trigger/types.rs` — Canonical execution request
**ExecutionSession** — `src/session/types.rs` — Execution identity container
**ExecutionState** — `src/eri/mod.rs` — Nine-state execution model
**ExecutionTarget** — `src/target/mod.rs` — Provider-independent placement contract (v0.13)
**Executor** (trait) — `src/executor/mod.rs` — Core executor interface

## F

**FeatureFlag** — `src/feature_gate/mod.rs` — Feature flag type
**FeatureRegistry** — `src/feature_gate/mod.rs` — Feature flag registry
**FeedbackCalibrator** — `src/telemetry/calibration.rs` — EMA-based feedback calibration
**FusionMetrics** — `src/telemetry/metrics.rs` — Core Prometheus metrics

## G

**GraphVerification** — `src/compiler/passes/legacy_passes.rs` — Final graph validation pass
**GraphVisualizer** — `src/devex/visualizer.rs` — ExecutionGraph visualization

## I

**intent_to_workflow** — `src/intent/lowering.rs` — Planner's initial lowering: NormalizedIntent → WorkflowIR
**IntentKind** — `src/intent/mod.rs` — NormalizedIntent classification
**IntentPlanner** — `src/planner/intent_planner.rs` — Entry-point planner delegating to sub-planners

## L

**LifecycleManager** — `src/lifecycle/manager.rs` — Session lifecycle orchestration

## M

**Model** (trait) — `src/providers/mod.rs` — LLM-specific behavior
**MemorySessionStore** — `src/session/store/memory.rs` — In-memory session store

## N

**NetworkConstraints** — `src/target/mod.rs` — Target egress constraints
**NodeFusion** — `src/compiler/passes/legacy_passes.rs` — Node merging optimization pass
**NormalizedIntent** — `src/intent/mod.rs` — Canonical goals and constraints (v0.13)

## O

**OllamaModel** — `src/providers/ollama_model.rs` — Ollama model adapter
**OpenRouterModel** — `src/providers/openrouter_model.rs` — OpenRouter model adapter
**OperationError** — `src/operations/mod.rs` — Operations platform error type

## P

**PassRegistry** — `src/compiler/registry/mod.rs` — Compiler pass registry
**PackageLoader** — `src/package/loader.rs` — WASM compilation and contract registration
**PackageVerifier** — `src/package/verifier.rs` — .fusionpkg structural and attestation verification
**Permission** (enum) — `crates/fusion-plugin-api/src/lib.rs` — 5 permission variants with scoping
**Planner** (trait) — `src/planner/mod.rs` — Core planner interface
**Plugin** (trait) — `crates/fusion-plugin-api/src/lib.rs` — Plugin identity trait
**PluginManager** — `src/plugin/manager.rs` — Plugin loading and registration
**PluginManifest** — `src/plugin/manifest.rs` — TOML plugin manifest
**PluginMetadata** — `crates/fusion-plugin-api/src/lib.rs` — Version compatibility metadata
**PluginScaffolder** — `src/devex/scaffold.rs` — Plugin project generator
**PolicyCompilerPass** — `src/compiler/passes/policy.rs` — Policy compilation in compiler
**PrimitiveGraph** — `src/compiler/ir/primitive_ir.rs` — Canonical lowered IR
**ProjectionDispatcher** — `src/events/projection.rs` — Event projection framework
**Provider** (trait) — `src/providers/mod.rs` — Unified LLM interface
**ProviderRegistry** — `src/providers/registry.rs` — Provider registration
**ProviderRouter** — `src/providers/router.rs` — Provider request routing

## R

**ReasoningBudget** — `src/abi/mod.rs` — ABI node reasoning budget
**ReplayEngine** — `src/session/replay.rs` — 3-mode execution replay
**RequirementsExtractor** — `src/requirements/extractor.rs` — Intent classification
**ResourceGuard** — `src/resource/guard.rs` — RAII resource cleanup
**ResourceLimits** — `src/target/mod.rs` — Target resource limits
**ResourceManager** — `src/resource/mod.rs` — Central resource tracking
**RetryFallbackInsertion** — `src/compiler/passes/legacy_passes.rs` — Retry/fallback pass
**RouterError** — `src/types/error.rs` — Centralized error type with PipelineStage
**RuntimeError** — `src/runtime/mod.rs` — Capability runtime error type
**RuntimeModuleCache** — `src/runtime/module_cache.rs` — Compiled module cache

## S

**SandboxConfig** — `src/runtime/config.rs` — Sandbox memory/fuel/timeout limits
**SandboxInstance** — `src/runtime/sandbox_instance.rs` — Ephemeral sandbox instance trait
**SandboxRuntime** — `src/runtime/sandbox_runtime.rs` — Sandbox instantiation trait
**Scheduler** (trait) — `src/scheduler/mod.rs` — Core scheduler interface
**SchedulerKind** — `src/target/mod.rs` — Preferred scheduler enumeration
**SchemaBuilder** — `crates/fusion-capability-sdk/src/schema.rs` — JSON Schema builder
**SchedulingHints** — `src/compiler/passes/legacy_passes.rs` — Scheduling annotation pass
**SecurityPolicy** — `src/abi/mod.rs` — ABI node security policy
**SecurityProfile** — `src/target/mod.rs` — Target security profile
**SessionSnapshot** — `src/session/types.rs` — Point-in-time execution state
**SessionStore** (trait) — `src/session/store/mod.rs` — Session persistence interface
**SimplePlanner** — `src/planner/simple.rs` — Default single-node planner
**SqliteEvidenceRepository** — `src/telemetry/sqlite_repo.rs` — SQLite evidence storage
**SqliteSessionStore** — `src/session/store/sqlite.rs` — SQLite session storage
**Strategy** (trait) — `src/strategies/mod.rs` — Multi-step reasoning strategy trait
**StrategyIR** — `src/compiler/ir/strategy_ir.rs` — Strategy-aware IR

## T

**Tool** (trait) — `src/tools/mod.rs` — Tool execution trait
**ToolRegistry** — `src/tools/registry.rs` — Tool registration
**TraceInspector** — `src/devex/trace_inspector.rs` — Execution trace debugger
**Transport** (trait) — `src/transport/mod.rs` — Wire protocol interface
**TelemetryHook** — `src/abi/mod.rs` — ABI node telemetry hook
**TriggerTrace** — `src/trigger/trace.rs` — Execution provenance chain

## V

**ValidationError / ValidationReport** — `crates/fusion-ir/src/validate.rs` — Three-layer validation results

## W

**WasmtimeSandboxRuntime** — `src/runtime/wasmtime_runtime.rs` — Wasmtime concrete runtime
**WORKFLOW_IR_VERSION** — `crates/fusion-ir/src/version.rs` — IR schema version (1)
**WorkflowBuilder** — `crates/fusion-ir/src/builder.rs` — Immutable builder; only public construction path
**WorkflowDefinition** — `src/workflow/mod.rs` — YAML workflow template
**WorkflowEdge / WorkflowEdgeKind** — `crates/fusion-ir/src/edge.rs` — Edge types (6 frozen kinds)
**WorkflowIR** — `crates/fusion-ir/src/workflow.rs` — Canonical immutable workflow graph (v0.13 contract 2, migrated)
**WorkflowIR (legacy)** — `src/types/mod.rs` — High-level abstract plan used by the planner/compiler pipeline (superseded by fusion-ir WorkflowIR)
**WorkflowIrError** — `crates/fusion-ir/src/error.rs` — Serialization/validation error wrapper
**WorkflowMetadata** — `crates/fusion-ir/src/workflow.rs` — Extensible provider-independent workflow metadata
**WorkflowNode / WorkflowNodeKind** — `crates/fusion-ir/src/node.rs` — Node types (9 frozen kinds)
**WorkflowPlanner** — `src/planner/workflow.rs` — Registry-first planner
**WorkflowRegistry** — `src/workflow/registry.rs` — YAML workflow loader
**WorkQueue** — `src/scheduler/work_queue.rs` — Topological DAG scheduling queue

## Z

**ZenModel** — `src/providers/zen_model.rs` — Zen API model adapter
