# FusionRouter Architecture Handbook

> **Frozen Specifications:** AF-003 Platform Architecture & AF-004 Platform Contract Freeze (`v1`).

## Core Architecture Principles

1. **Compiler as Heart:** Every request traverses `Planner -> Compiler -> Scheduler -> Runtime -> Telemetry`. Zero bypass paths.
2. **3-Tier Cargo Workspace:** Strict downward dependency hierarchy (`Foundation -> Engine -> Platform -> Applications`).
3. **9-Pass Optimization Pipeline:** `Validation -> Capability Resolution -> Constraint Solver -> Constant Folding -> Dead Node Elimination -> Node Fusion -> Retry Injection -> Fallback Injection -> Scheduling Hints`.
4. **Execution Intelligence & Replay:** Portable `.fusion` `ExecutionBundle` export/import and 3-mode deterministic replay.
