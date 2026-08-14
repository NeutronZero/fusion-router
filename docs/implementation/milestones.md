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

## Phase 8-9 — Advanced DAG & Strategies
- DAG compilation works
- Golden tests for branching/looping pass

## Phase 10-13 — LTS Foundation & Governance (v0.14.0)
- 3-Tier Cargo Workspace refactoring completed (`Foundation -> Engine -> Platform -> Applications`)
- 17 Architecture Laws and 11 Architectural Invariants enforced (AF-003, AF-004, AF-005)
- Platform Health Engine & Recovery Engine operational
- Portable `.fusion` bundle export/import with 3-mode deterministic replay
- Certified Performance SLOs (Planner <10ms, Compiler <20ms, Scheduler <5ms, Runtime <10ms, Replay <20ms)

## Phase 14 — Multi-Model Ensemble Review (v0.14.5)
- ADR-038 multi-model ensemble review CLI implemented
- 7 review findings fixed, 9 refuted with quoted empirical evidence
- Codebase test suite fully green with zero warnings

## Phase 15 — Distributed Architecture (v0.15)
- Worker Protocol v1 and RemoteWorkerPool integration
- Multi-node capability federation
- High-availability state migration & replay attestation
