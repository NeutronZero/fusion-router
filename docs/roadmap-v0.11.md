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

### Long-Term Release Model

This roadmap naturally suggests a progression toward a stable public platform:

| Version | Focus |
|---------|-------|
| v0.8 | Execution model |
| v0.9 | Compiler |
| v0.10 | Capability platform |
| **v0.11** | **Platform maturity** |
| v0.12 | Cloud-native operations |
| v1.0 | Stable public platform |

The transition from v0.8–v0.10 built the architectural foundation. v0.11 begins the shift from architecture-building to operational excellence, culminating in a v1.0 that can be depended on for production workloads.

---

## 2. Platform Invariants — v1.0 Release Gates

As v0.11 operationalizes the kernel, a small set of **platform invariants** must be tracked across every release. These are the contracts that turn v0.x into v1.0 — they must hold before a stable release is warranted.

| Invariant | Description |
|-----------|-------------|
| Public SDK compatibility | Public API changes require explicit, versioned migration paths. No silent breakage. |
| Replay compatibility | Snapshots written by version N must replay correctly on version N+1 (at minimum the two most recent minor versions). |
| Session migration | Session state must migrate safely across version upgrades without data loss. |
| Deterministic compilation | Identical input → identical `ExecutionGraph` output. The compiler is a pure function of its inputs. |
| Stable execution semantics | Execution of identical graphs with identical inputs produces identical results (modulo non-deterministic provider responses, which are captured in provenance). |
| Connector conformance | All certified connectors pass the conformance suite on every supported runtime version. |
| Policy determinism | Policy evaluation produces identical results for identical requests and identical policy sets. |
| Upgrade safety | Upgrade from the prior minor version must succeed with zero data loss. Rollback must be possible within one minor version. |

These invariants are release gates, not features. They must be verified as part of every release pipeline. Violations are release-blocking.

---

## 3. Implementation Stages

The epics are sequenced by dependency rather than priority. Each stage must be substantially complete before the next begins.

```
Stage 1 ─── Stage 2 ─── Stage 3 ─── Stage 4 ─── Stage 5
Foundation   Dist.       Intel.       UX          Enterprise
             Runtime
  G            A           C            J           F
  B                       E                        H
  D                                                I
  K
  M
```

### Stage 1 — Platform Foundation

These epics build the operational infrastructure everything else depends on:

| Epic | Rationale |
|------|-----------|
| **G** Live Configuration | No system should require restarts during development or production |
| **B** Streaming Runtime & Metering | Streaming is the most-used execution path; it must be metered and observable |
| **D** Connector Runtime Platform | Connector health and discovery are prerequisites for distributed execution |
| **K** Reliability Engineering | Chaos testing validates that all foundation epics work under failure |
| **M** Compatibility & Release Engineering | Versioned releases require upgrade/replay/downgrade guarantees |

### Stage 2 — Distributed Runtime

| Epic | Rationale |
|------|-----------|
| **A** Cloud-Native Distributed Runtime | Only after config reload, connector health, and reliability testing are stable. Distributed execution is much easier to debug when the operational infrastructure already exists. |

### Stage 3 — Intelligence

| Epic | Rationale |
|------|-----------|
| **C** Planning Intelligence | Learning depends on high-quality telemetry from Stages 1–2 |
| **E** Operational Intelligence | Analytics and recommendations require a stable runtime to observe |

### Stage 4 — Platform UX

| Epic | Rationale |
|------|-----------|
| **J** Operator Control Plane | Dashboards built before mature telemetry typically need major redesign later. Stage 3 ensures the data is ready. |

### Stage 5 — Enterprise

| Epic | Rationale |
|------|-----------|
| **F** Production Search | Product-facing capability, not a platform foundation |
| **H** Trigger Runtime | Product-facing capability, not a platform foundation |
| **I** Enterprise Policy Engine | Product-facing capability, not a platform foundation |

---

## 3. Implementation Stages Detail

### Stage 1 — Platform Foundation

#### Epic G — Live Configuration ⭐⭐⭐⭐

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

#### Epic B — Streaming Runtime & Metering ⭐⭐⭐⭐⭐

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

#### Epic D — Connector Runtime Platform ⭐⭐⭐⭐

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

#### Epic K — Reliability Engineering ⭐⭐⭐⭐⭐

Beyond functional correctness — validate system behavior under failure, and establish measurable recovery properties.

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
| Recovery Time Objective (RTO) | Measure and enforce time-to-recovery targets per failure mode |
| Mean time to recover (MTTR) | Track recovery duration across failure scenarios |
| Automatic rollback | Revert to last known-good state on failed upgrade or config apply |
| State convergence | Assert that after any failure+recovery, system state converges to expected steady state |
| SLO verification | Convert chaos tests into measurable SLOs rather than pass/fail assertions |

---

#### Epic M — Compatibility & Release Engineering ⭐⭐⭐⭐⭐

Once you begin shipping versions, compatibility becomes one of the most valuable engineering investments.

```
v0.10         v0.11
   │            │
   ▼            ▼
 Upgrade ──→ Replay ──→ Resume ──→ Success
```

Ensuring that every version upgrade is safe, reversible, and verifiable.

| Feature | Description |
|---------|-------------|
| API compatibility tests | Automated assertion that public API signatures remain backward-compatible |
| Plugin compatibility matrix | Document which plugin versions are compatible with which runtime versions |
| Connector compatibility matrix | Document which connector versions work with which runtime versions |
| Replay compatibility tests | Verify that v0.10 snapshots replay correctly on v0.11 runtime |
| Session format migration | Automated migration of session state across version boundaries |
| Upgrade tests | Green/blue upgrade validation: install new version, verify all paths, roll back on failure |
| Downgrade behavior | Assert that downgrading to a prior version recovers correctly without data loss |
| SemVer validation | Automated enforcement of semver rules on public API changes |
| Feature-flag gating | Gate new features behind flags so operators can incrementally roll out |

---

#### SDK Validation Suite

Beyond the PluginScaffolder introduced in v0.10, add certification tooling for the ecosystem.

| Feature | Description |
|---------|-------------|
| Connector certification | `cargo fusion certify-connector` — verify ABI compatibility, metadata, capabilities, version constraints, documentation completeness, benchmark results |
| Strategy certification | `cargo fusion certify-strategy` — verify strategy contract compliance, determinism, and performance |
| Plugin certification | `cargo fusion certify-plugin` — verify plugin ABI, metadata, and runtime compatibility |
| Conformance suite | Reusable test harness that plugin/connector/strategy authors run before publishing |

---

### Stage 2 — Distributed Runtime

#### Epic A — Cloud-Native Distributed Runtime ⭐⭐⭐⭐⭐

The `DistributedScheduler` exists but delegates to the local scheduler. v0.11 makes distributed execution genuinely operational.

```
Scheduler
    │
    ▼
DistributionStrategy
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
| **Execution affinity** | Co-locate related nodes (e.g. Generate → Review → Judge) on the same worker to avoid large artifact transfers dominating runtime. Workers advertise locality preferences; scheduler respects them within balancing constraints. |

---

### Stage 3 — Intelligence

#### Epic C — Planning Intelligence ⭐⭐⭐⭐

Planning is currently template-driven. v0.11 evolves planning into a measurable, adaptive subsystem.

**Architectural invariant:** The planner must learn without changing compiler behavior. The boundary is:

```
Planner
    │
    ▼
WorkflowIR
    │
    ▼
Compiler  (deterministic, never modified by planner feedback)
```

Learning belongs before compilation. The compiler remains deterministic — planner feedback may change *which* template is selected or *which* strategy is chosen, but never how the compiler lowers a given WorkflowIR.

| Feature | Description |
|---------|-------------|
| Planner ranking | Rank planner templates by historical success rate per intent |
| Strategy ranking | Rank strategies (Single, Consensus, Reflection, etc.) by cost/latency/success per workload |
| Planner confidence | Planner emits confidence scores with generated IRs |
| Adaptive strategy selection | Strategy is selected based on historical performance, not static mapping |
| Planner telemetry | Record planner decisions as structured events for offline analysis |
| **Recommendation ingestion** | Consume recommendations from Operational Intelligence (see Epic E) — e.g. "Consensus is 40% slower for Debug requests with no accuracy improvement" → adjust strategy selection |

---

#### Epic E — Operational Intelligence ⭐⭐⭐⭐

The telemetry system already emits rich data. v0.11 makes it actionable — and turns it into a competitive differentiator through recommendations that feed back into Planning.

```
Telemetry
    │
    ▼
Analytics → Recommendations
                │
                ├──→ Operator Dashboard (visibility)
                └──→ Planning Intelligence (automated optimization)
```

| Feature | Description |
|---------|-------------|
| Slow graph detection | Identify execution graphs exceeding p95 latency thresholds |
| Expensive workflow detection | Flag workflows exceeding budget envelopes |
| Retry hotspot analysis | Find nodes/models/providers with elevated retry rates |
| Failing connector tracking | Aggregate connector failure rates and root causes |
| Provider cost analysis | Per-provider, per-model cost breakdowns over time windows |
| Execution bottleneck identification | Pinpoint scheduler, provider, or connector bottlenecks in the pipeline |
| **Actionable recommendations** | Emit concrete suggestions: "Consensus strategy is 40% slower for Debug requests with no accuracy improvement" or "Reflection performs better than Debate for Architecture prompts" |
| **Feedback channel** | Recommendations are consumable by Planning Intelligence (Epic C) as structured evidence for adaptive strategy and template selection |

---

### Stage 4 — Platform UX

#### Epic J — Operator Control Plane ⭐⭐⭐⭐

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

### Stage 5 — Enterprise

#### Epic F — Production Search ⭐⭐⭐

Carried forward from the v0.10 deferred backlog. Replace the mock `SearchTool` with production adapters.

| Feature | Description |
|---------|-------------|
| Tavily adapter | HTTP connector wrapping `https://api.tavily.com/search` |
| Serper adapter | HTTP connector wrapping `https://google.serper.dev/search` |
| Brave Search adapter | HTTP connector wrapping Brave Search API |
| Unified `SearchTool` trait | Common interface over all search backends |

---

#### Epic H — Trigger Runtime ⭐⭐⭐

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

#### Epic I — Enterprise Policy Engine ⭐⭐⭐

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

## 4. Cross-Cutting: Performance Engineering

#### Epic L — Performance Engineering ⭐⭐⭐⭐⭐

Optimize the existing stack. This epic runs across all stages — each stage includes performance validation, and dedicated optimization sprints address systemic issues discovered during earlier stages.

| Focus Area | Target |
|------------|--------|
| Zero-copy artifacts | Avoid `serde_json::Value` cloning in `ExecutionResult.outputs` |
| Lock reduction | Replace `Mutex<Connection>` with SQLite WAL + connection pool |
| Graph compilation cache | Cache `PrimitiveGraph → ExecutionGraph` derivation by hash |
| Connector pooling | Reuse HTTP/TCP connections across connector invocations |
| Async batching | Batch telemetry writes; batch budget envelope updates |
| Scheduler profiling | Identify `buffer_unordered` overhead with very large graphs (1000+ nodes) |
| **Memory profiling** | Track allocation patterns, identify hot allocations and fragmentation |
| **Allocation heatmaps** | Per-component memory pressure visualization to guide targeted optimization |
| **Cache effectiveness** | Measure hit rates for compilation cache, connector pools, and session caches |
| **Connector latency histograms** | Per-connector, per-operation latency distributions under load |

---

## 5. Epic Summary

| Epic | Area | Stage | Dependencies |
|------|------|-------|-------------|
| **G** | Live Configuration | 1 — Foundation | v0.10 AppConfig, config/default.yaml |
| **B** | Streaming Runtime & Metering | 1 — Foundation | v0.10 streaming path |
| **D** | Connector Runtime Platform | 1 — Foundation | v0.10 Connector trait, CapabilityPlugin |
| **K** | Reliability Engineering | 1 — Foundation | All existing subsystems |
| **M** | Compatibility & Release Engineering | 1 — Foundation | All existing subsystems |
| **A** | Cloud-Native Distributed Runtime | 2 — Distributed | Epics G, D, K |
| **C** | Planning Intelligence | 3 — Intelligence | v0.10 EvidenceRepository, FeedbackCalibrator; Epic E |
| **E** | Operational Intelligence | 3 — Intelligence | v0.10 FusionMetrics, SqliteEvidenceRepository |
| **J** | Operator Control Plane | 4 — UX | Epics E (telemetry), A (workers), D (connectors) |
| **F** | Production Search | 5 — Enterprise | v0.10 connectors, HTTPRequestTool |
| **H** | Trigger Runtime | 5 — Enterprise | v0.10 Trigger Framework (ADR-031) |
| **I** | Enterprise Policy Engine | 5 — Enterprise | v0.10 Policy Compilation (ADR-024) |
| **L** | Performance Engineering | Cross-cutting | All existing subsystems |

---

## 6. What Is Not in v0.11

The following are explicitly deferred beyond v0.11:

- Cross-strategy aggregation primitive (requires new ADR if proven necessary)
- Hot-reload WASM plugins (requires WASM runtime with live swapping support)
- Multi-region distributed scheduling (requires Epic A to stabilize first)
- Streaming WASM plugins (requires WASM async host bridge)

---

## 7. References

- [FusionRouter v0.10.0 Architecture Specification](docs/fusionrouter_architecture_v0.10.0.md)
- [Release Gate Specification](docs/release/release_gate_spec.md) — executable release criteria
- [ADR-027 — Architecture Conformance Testing](docs/adr/ADR-027-architecture-conformance-testing.md)
- [ADR-030 — Session Replay Semantics](docs/adr/ADR-030-session-replay-semantics.md)
- [ADR-031 — Trigger Request Semantics](docs/adr/ADR-031-trigger-request-semantics.md)
- [v0.10 Roadmap](docs/roadmap-v0.10.md) (predecessor, complete)
- [Architecture Debt Register](docs/architecture/architecture_debt_register.md)
