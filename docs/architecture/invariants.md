# Architectural Invariants

1. **All LLM interactions go through the Provider trait.** No subsystem calls an LLM API directly.
2. **The Compiler is a pipeline of pure passes.** Each pass takes an IR and returns a transformed IR or an error.
3. **The Planner produces a WorkflowIR, never an ExecutionGraph.** Compilation is a separate concern.
4. **Every ExecutionNode has exactly one Strategy.** The Strategy trait determines how it is expanded.
5. **Scheduling is topology-driven.** A node executes when all its dependency nodes have succeeded.
6. **The ResourceManager is the sole authority on budget.** No other component makes quota decisions.
7. **Telemetry is passive.** It observes and records but never alters execution.
8. **Evidence is derived from telemetry.** The EvidenceRepository aggregates raw records into snapshots.
9. **All config is external.** No hardcoded models, providers, or policies.
10. **Context is immutable once assembled.** No component modifies the ContextSnapshot after creation.
11. **Requirements are a heuristic, not a guarantee.** They guide but never constrain execution.
12. **Every public API is OpenAI-compatible.** /v1/chat/completions is the primary interface.
13. **Streaming is first-class.** All providers must support both streaming and non-streaming modes.
14. **Errors are typed.** Every fallible operation returns a structured error type, not a string.
