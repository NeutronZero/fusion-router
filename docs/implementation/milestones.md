# Milestones

## Phase 0 — Foundation
- `cargo build` succeeds
- Server starts and responds to /v1/chat/completions

## Phase 1 — Context & Requirements
- ContextAssembler produces correct snapshots
- RequirementsExtractor correctly classifies intents
- Unit tests pass

## Phase 2 — Planner & Compiler
- Planner produces valid WorkflowIR
- Compiler produces deterministic ExecutionGraph
- Golden tests pass

## Phase 3 — Scheduler
- Work queue processes nodes in dependency order
- Retry and fallback logic works
- Unit tests pass

## Phase 4 — Resource Manager
- Quotas are enforced
- Budget optimization pass downgrades models when over budget

## Phase 5 — Strategies
- Each strategy produces correct subgraphs
- Golden tests for all strategies pass

## Phase 6 — Provider Abstraction
- Zen, OpenRouter, Ollama adapters work
- Integration tests pass with mock servers

## Phase 7 — Telemetry
- Tracing spans cover all major operations
- EvidenceRepository records and aggregates correctly
- Planner uses evidence to bias model selection

## Phase 8-9
- DAG compilation works
- Golden tests for branching/looping pass
