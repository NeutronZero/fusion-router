# Implementation Roadmap

| Phase | Name | Goal |
|-------|------|------|
| 0 | Foundation | Repository structure, Cargo.toml, base HTTP server, minimal provider adapter |
| 1 | Context & Requirements | Data types, context assembler, requirements extractor |
| 2 | Planner & Compiler | WorkflowIR, ExecutionGraph, planner, compiler passes, golden tests |
| 3 | Scheduler & Execution State | Work queue, state machine, retries, fallbacks |
| 4 | Resource Manager | Quota tracking, budget optimization |
| 5 | Strategies | Single, Consensus, Reflection |
| 6 | Provider Abstraction | Provider/Model/Transport split, normalization layer |
| 7 | Telemetry & Evidence | Tracing, SQLite evidence repository, evidence-informed planning |
| 8 | DAG Support | Branching, conditionals, loops, split/join |
| 9 | Advanced Strategies | Chain, Debate, Fusion, plugin system |
