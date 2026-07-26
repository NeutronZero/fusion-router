# FusionRouter v0.11 Roadmap — Platform Maturity & Cloud-Native

> **Theme:** Production Platform, Multi-Node Execution, Operational Intelligence
> **Status:** Strategic Planning
> **Predecessor:** v0.10.0 (Capability Platform — Phase 7 Ecosystem Tracks)

---

## 1. Executive Vision

In **v0.8.0**, FusionRouter established the Intent-Oriented Execution Model.  
In **v0.9.0**, FusionRouter solidified the compiler pipeline (`PrimitiveGraph` → `ExecutionGraph`, deterministic lowering, optimization passes, provenance).  
In **v0.10.0**, FusionRouter delivered the Capability Platform — triggers, sessions, connectors, policy compilation, developer tooling, distributed scheduling, and production hardening.

The compiler, runtime, sessions, triggers, connectors, and SDK are now comprehensive.  
**v0.11.0 does not introduce another foundational architectural layer.** Instead, it consumes every existing API to build a production-grade, cloud-native, operationally intelligent platform.

### What v0.11 Is Not

- No new intermediate representations
- No new scheduler abstractions
- No new compiler pipelines
- No new session layers
- No new connector APIs
- No new provenance models

The kernel is mature. v0.11 validates it through implementations rather than redesigning it.

---

## 2. Epic A — Cloud-Native Distributed Runtime ⭐⭐⭐⭐⭐

The `DistributedScheduler` exists but delegates to the local scheduler. v0.11 makes distributed execution genuinely operational.

```
ExecutionGraph
        │
        ▼
DistributedScheduler
        │
        ├──── Worker A
        ├──── Worker B
        ├──── Worker C
        └──── Local Fallback
```

| Feature | Description |
|---------|-------------|
| Worker registration protocol | Workers announce themselves to the scheduler on startup |
| Heartbeats | Periodic liveness signals from workers; stale workers are evicted |
| Lease management | Workers lease node execution with TTL; leases expire and are reassigned on failure |
| Node capability advertisement | Workers advertise model availability, concurrency capacity, connector support |
| Work stealing | Idle workers pull ready nodes from overloaded workers |
| Execution migration | Stalled or failed worker executions are migrated to healthy workers |
| Worker draining | Graceful shutdown: stop accepting new work, finish in-flight, deregister |
| Scheduler failover | Standby schedulers takeover if primary fails |
| Leader election | Optional consensus-based primary election |

---

## 3. Epic B — Streaming Runtime & Metering ⭐⭐⭐⭐⭐

Carried forward from the v0.10 deferred backlog. Streaming currently bypasses the pipeline — it must become a first-class metered path.

| Feature | Description |
|---------|-------------|
| `StreamingResourceGuard` | Token-counting `Transform` stream wrapper recording incremental usage |
| SSE token metering | Count prompt + completion tokens per SSE chunk |
| Streaming cost tracking | Accumulate per-token cost during streaming |
| Streaming latency tracking | Measure TTFB (time to first byte) and inter-token latency |
| Stream cancellation | Client disconnect triggers immediate upstream provider cancellation |
| Partial replay | Replay streaming execution from checkpoint without re-executing the full stream |
| Stream checkpoints | Periodic snapshot of stream state for resumability |

---

## 4. Epic C — Planning Intelligence ⭐⭐⭐⭐

Planning is currently template-driven. v0.11 evolves planning into a measurable, adaptive subsystem.

```
Planner
    │
    ▼
Evidence → Planner Optimizer → Better WorkflowIR
```

| Feature | Description |
|---------|-------------|
| Planner ranking | Rank planner templates by historical success rate per intent |
| Strategy ranking | Rank strategies (Single, Consensus, Reflection, etc.) by cost/latency/success per workload |
| Planner confidence | Planner emits confidence scores with generated IRs |
| Adaptive strategy selection | Strategy is selected based on historical performance, not static mapping |
| Planner telemetry | Record planner decisions as structured events for offline analysis |

---

## 5. Epic D — Connector Runtime Platform ⭐⭐⭐⭐

The connector SDK exists (`Connector` trait, 6 reference connectors). v0.11 builds the operational layer around it.

| Feature | Description |
|---------|-------------|
| Connector marketplace | Plugin registry for publishing and discovering connectors |
| Connector discovery | Runtime auto-discovery of installed connectors |
| Connector health monitoring | Periodic health checks; mark connectors degraded/unhealthy |
| Connector version negotiation | Semver compatibility checks between connector and runtime |
| Connector capability search | Query connectors by capability, input schema, or permission requirements |
| Connector sandbox policies | Per-connector resource limits, allowed networks, filesystem access |

---

## 6. Epic E — Operational Intelligence ⭐⭐⭐⭐

The telemetry system already emits rich data. v0.11 makes it actionable.

```
Telemetry
    │
    ▼
Analytics → Recommendations → Operator Dashboard
```

| Feature | Description |
|---------|-------------|
| Slow graph detection | Identify execution graphs exceeding p95 latency thresholds |
| Expensive workflow detection | Flag workflows exceeding budget envelopes |
| Retry hotspot analysis | Find nodes/models/providers with elevated retry rates |
| Failing connector tracking | Aggregate connector failure rates and root causes |
| Provider cost analysis | Per-provider, per-model cost breakdowns over time windows |
| Execution bottleneck identification | Pinpoint scheduler, provider, or connector bottlenecks in the pipeline |

---

## 7. Epic F — Production Search ⭐⭐⭐

Carried forward from the v0.10 deferred backlog. Replace the mock `SearchTool` with production adapters.

| Feature | Description |
|---------|-------------|
| Tavily adapter | HTTP connector wrapping `https://api.tavily.com/search` |
| Serper adapter | HTTP connector wrapping `https://google.serper.dev/search` |
| Brave Search adapter | HTTP connector wrapping Brave Search API |
| Unified `SearchTool` trait | Common interface over all search backends |

---

## 8. Epic G — Live Configuration ⭐⭐⭐⭐

Carried forward from the v0.10 deferred backlog. Configuration currently requires process restart.

| Feature | Description |
|---------|-------------|
| `ArcSwap<AppConfig>` | Lock-free live config swapping |
| SIGHUP reload | Unix signal triggers config re-parse and swap |
| Configuration validation | Validate new config before applying; roll back on failure |
| Live provider updates | Add, remove, or reconfigure providers without restart |
| Live routing policy updates | Update model routing rules, circuit breaker thresholds at runtime |
| Connector reload | Hot-swap connector implementations |

---

## 9. Epic H — Trigger Runtime ⭐⭐⭐

ADR-031 defines triggers. v0.11 completes the production trigger runtime.

| Feature | Description |
|---------|-------------|
| Trigger persistence | Persist trigger declarations and execution history |
| Trigger history | Audit trail of all trigger activations and outcomes |
| Trigger retries | Configurable retry policy for failed trigger executions |
| Dead-letter queue | Capture permanently failed trigger invocations for manual inspection |
| Webhook signatures | HMAC signature verification for incoming webhooks |
| Cron monitoring | Track scheduled trigger drift, missed ticks, overruns |
| Trigger metrics | Per-trigger-type counters, latency histograms, error rates |

---

## 10. Epic I — Enterprise Policy Engine ⭐⭐⭐

Current policies compile correctly (`PolicyAST` → `PolicyIR` → `PolicyCompilerPass`). v0.11 makes them enterprise-ready.

| Feature | Description |
|---------|-------------|
| RBAC | Role-based access control for execution requests |
| ABAC | Attribute-based access control policies |
| Organization policies | Multi-tenant policy isolation at organization scope |
| Tenant policies | Per-tenant policy overrides |
| Policy bundles | Grouped, versioned, signed policy collections |
| Policy audit history | Immutable log of all policy changes and evaluations |

---

## 11. Epic J — Operator Control Plane ⭐⭐⭐⭐

Where FusionRouter begins to feel like Kubernetes — a control plane for managing the platform.

| Feature | Description |
|---------|-------------|
| Execution dashboard | Real-time view of in-flight and completed executions |
| Live DAG viewer | Animated topological graph of executing workflows |
| Worker management | List, inspect, drain, and decommission workers |
| Connector management | Browse installed connectors, health status, capability listings |
| Session browser | Search and inspect active and historical sessions |
| Replay browser | Browse snapshots, trigger replay in any of the 3 modes |
| Graph explorer | Navigate compiled execution graphs, examine nodes and edges |
| Provider dashboard | Per-provider latency, error rates, cost, and usage trends |
| Budget dashboard | Real-time budget consumption, alerts at configurable thresholds |

---

## 12. Epic K — Reliability Engineering ⭐⭐⭐⭐⭐

Beyond functional correctness — validate system behavior under failure.

| Feature | Description |
|---------|-------------|
| Provider outage simulation | Kill upstream provider; verify circuit breaker + fallback routing |
| Worker crash simulation | Hard-kill a worker mid-execution; verify lease expiry + migration |
| Scheduler crash simulation | Kill primary scheduler; verify failover + state recovery |
| SQLite corruption simulation | Corrupt WAL; verify recovery or graceful degradation |
| Connector failure simulation | Return errors from connectors; verify retry + dead-letter |
| Replay corruption simulation | Corrupt snapshot data; verify validation rejection |
| Automatic retry verification | Assert retry policy is honored across failure scenarios |
| Replay validation | Verify deterministic replay produces identical output |
| Checkpoint verification | Assert checkpoint/restore round-trips produce identical state |

---

## 13. Epic L — Performance Engineering ⭐⭐⭐⭐⭐

Instead of adding features, optimize the existing stack.

| Focus Area | Target |
|------------|--------|
| Zero-copy artifacts | Avoid `serde_json::Value` cloning in `ExecutionResult.outputs` |
| Lock reduction | Replace `Mutex<Connection>` with SQLite WAL + connection pool |
| Graph compilation cache | Cache `PrimitiveGraph → ExecutionGraph` derivation by hash |
| Connector pooling | Reuse HTTP/TCP connections across connector invocations |
| Async batching | Batch telemetry writes; batch budget envelope updates |
| Scheduler profiling | Identify `buffer_unordered` overhead with very large graphs (1000+ nodes) |

---

## 14. Epic Priority Matrix

| Epic | Area | Priority | Dependencies |
|------|------|----------|-------------|
| **A** | Cloud-Native Distributed Runtime | ⭐⭐⭐⭐⭐ | v0.10 DistributedScheduler |
| **B** | Streaming Runtime & Metering | ⭐⭐⭐⭐⭐ | v0.10 streaming path |
| **C** | Planning Intelligence | ⭐⭐⭐⭐ | v0.10 EvidenceRepository, FeedbackCalibrator |
| **D** | Connector Runtime Platform | ⭐⭐⭐⭐ | v0.10 Connector trait, CapabilityPlugin |
| **E** | Operational Intelligence | ⭐⭐⭐⭐ | v0.10 FusionMetrics, SqliteEvidenceRepository |
| **F** | Production Search | ⭐⭐⭐ | v0.10 connectors, HTTPRequestTool |
| **G** | Live Configuration | ⭐⭐⭐⭐ | v0.10 AppConfig, config/default.yaml |
| **H** | Trigger Runtime | ⭐⭐⭐ | v0.10 Trigger Framework (ADR-031) |
| **I** | Enterprise Policy Engine | ⭐⭐⭐ | v0.10 Policy Compilation (ADR-024) |
| **J** | Operator Control Plane | ⭐⭐⭐⭐ | Epics E (telemetry), A (workers), D (connectors) |
| **K** | Reliability Engineering | ⭐⭐⭐⭐⭐ | All existing subsystems |
| **L** | Performance Engineering | ⭐⭐⭐⭐⭐ | All existing subsystems |

---

## 15. What Is Not in v0.11

The following are explicitly deferred beyond v0.11:

- Cross-strategy aggregation primitive (requires new ADR if proven necessary)
- Hot-reload WASM plugins (requires WASM runtime with live swapping support)
- Multi-region distributed scheduling (requires Epic A to stabilize first)
- Streaming WASM plugins (requires WASM async host bridge)

---

## 16. References

- [FusionRouter v0.10.0 Architecture Specification](docs/fusionrouter_architecture_v0.10.0.md)
- [ADR-030 — Session Replay Semantics](docs/adr/ADR-030-session-replay-semantics.md)
- [ADR-031 — Trigger Request Semantics](docs/adr/ADR-031-trigger-request-semantics.md)
- [v0.10 Roadmap](docs/roadmap-v0.10.md) (predecessor, complete)
- [Architecture Debt Register](docs/architecture/architecture_debt_register.md)
