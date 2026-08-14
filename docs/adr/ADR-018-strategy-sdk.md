# ADR-018: Strategy SDK — Primitive IR, Lowering Contracts, and Extensible Orchestration

- **Status**: Proposed
- **Date**: July 2026
- **Context**: FusionRouter v0.9 Architectural Roadmap
- **Deciders**: FusionRouter Engineering Team

---

## Context

FusionRouter currently defines strategies as runtime node-expansion functions:

```rust
pub trait Strategy {
    fn apply(&self, node: &ExecutionNode) -> ExecutionSubgraph;
}
```

Each strategy (Single, Consensus, Reflection, Debate, ReAct, Chain, Fusion) hard-codes its subgraph topology at the `ExecutionNode` level. This couples strategy logic to the runtime scheduler. Adding a new strategy requires implementing `Strategy::apply` and registering it — but the scheduler must still interpret the resulting subgraph without knowing the strategy's semantic intent.

Several pressures now converge toward a different architecture:

1. **The structured-debate exercise** revealed that Debate, Consensus, Council, and Reflection are compositions of a small set of reusable primitives (FanOut, Barrier, Reducer, FeedbackLoop, ConditionalBranch). Each primitives is a scheduling concern; each strategy is a lowering concern.

2. **Deterministic replay** requires that the same strategy intent always produces the same execution graph. The current `Strategy::apply` runs at execution time and produces `ExecutionSubgraph` directly, making graph-level caching and prior validation difficult.

3. **Plugin-extensible orchestration** requires that third-party plugins can register new strategies without modifying the runtime. The current approach forces plugins to either implement `Strategy::apply` (which requires runtime awareness) or bypass the strategy system entirely.

4. **Compiler-oriented architecture** (ADR-003) already establishes a multi-pass compiler for `WorkflowIR -> ExecutionGraph`. Strategies are currently an exception to this model — they expand post-compilation. Moving strategy resolution into the compiler pipeline eliminates that gap.

---

## Decision

We introduce a **Strategy SDK** with a two-layer intermediate representation, a deterministic lowering pass per strategy, and typed artifact contracts between nodes.

### Layer 1: Strategy IR

Strategy IR is the planner's output. It speaks in domain-level strategy names with strategy-specific configuration:

```rust
pub enum StrategyIR {
    Single,
    Consensus { count: u32 },
    Reflection { max_cycles: u32 },
    Debate { roles: Vec<DebateRole> },
    ReAct { max_iterations: u32 },
    Chain { stages: Vec<StrategyIR> },
    Custom { name: String, config: serde_json::Value },
}
```

The planner emits `StrategyIR`. It does not produce `ExecutionGraph` directly.

### Layer 2: Primitive IR

Primitive IR is a small set of reusable scheduling primitives. It is the **only** IR the runtime scheduler understands:

```rust
pub enum PrimitiveNode {
    FanOut { count: u32 },
    Barrier { min_completion: f32, timeout: Duration, on_failure: BarrierFailurePolicy },
    Reducer { mode: ReducerMode },
    FeedbackLoop { max_iterations: u32 },
    ConditionalBranch { condition: String },
}

pub enum ReducerMode {
    Debate,
    Consensus,
    Majority,
    WeightedVote,
    Merge,
    Score,
}
```

The runtime scheduler implements only these primitives. It has no knowledge of Debate, Consensus, Council, or Reflection.

### Compiler Pipeline (Revised)

```
Strategy IR
   │
   ▼
Strategy Validation      ← strategy-level invariants (e.g., N ≥ 2 for Debate)
   │
   ▼
Lower                    ← per-strategy: StrategyIR → PrimitiveGraph
   │
   ▼
Primitive IR
   │
   ▼
Graph Validation         ← primitive-level invariants (acyclic, one reducer, etc.)
   │
   ▼
Optimization             ← specialize (barrier elimination, fan-out collapsing, etc.)
   │
   ▼
Execution Graph          ← lowered to runtime nodes with model bindings
```

### Strategy Trait (Revised)

Each strategy implements a **lowering** interface, not an execution interface:

```rust
pub trait Strategy: Send + Sync {
    fn descriptor(&self) -> StrategyDescriptor;
    fn lower(&self, ir: &StrategyIR, ctx: &CompilationContext) -> Result<PrimitiveGraph, CompilerError>;
}

pub struct StrategyDescriptor {
    pub name: &'static str,
    pub parallelism: Parallelism,
    pub requires_barrier: bool,
    pub supports_streaming: StreamingMode,
    pub retry_policy: RetryPolicy,
    pub expected_outputs: Vec<ArtifactKind>,
}

pub enum Parallelism { Sequential, Fixed(u32), Unlimited }
pub enum StreamingMode { None, IncrementalArtifacts, IncrementalReduction, Full }
```

### Artifact Contract

Nodes communicate via typed, versioned artifacts:

```rust
pub enum ArtifactKind { Debate, Consensus, Reflection }

pub trait Artifact: Send + Sync {
    fn version(&self) -> u16;
    fn kind(&self) -> ArtifactKind;
}
```

Each strategy defines its own artifact struct implementing `Artifact`. The reducer node is generic — its mode determines how it interprets `Vec<Box<dyn Artifact>>`.

### Debate Artifact Example

```rust
pub const DEBATE_ARTIFACT_VERSION: u16 = 1;

pub struct DebateArtifact {
    pub version: u16,
    pub role: RoleId,
    pub stance: Stance,
    pub claims: Vec<Claim>,
    pub tradeoffs: Vec<Tradeoff>,
    pub citations: Vec<Citation>,
    pub confidence: ConfidenceMetrics,
    pub unknowns: Vec<String>,
    pub assumptions: Vec<String>,
    pub risks: Vec<String>,
}

pub struct Claim {
    pub id: ClaimId,          // e.g., "C1", "C2"
    pub text: String,
}

pub struct Tradeoff {
    pub dimension: String,
    pub cost: String,
    pub benefit: String,
}

pub struct ConfidenceMetrics {
    pub claim_support: f32,
    pub citation_quality: f32,
    pub reasoning_consistency: f32,
    pub overall: f32,
}
```

### Deterministic Lowering

The lowering pass **MUST** be deterministic:

```
Same Strategy IR + Same CompilationContext
  → Always produces byte-identical PrimitiveGraph
```

This is enforced by:
- Pure lowering functions (no I/O, no randomness)
- Stable node IDs derived from strategy configuration
- Serialization of PrimitiveGraph at every stage for testability

### Lowering: Debate → Primitives

The Debate strategy lowerer produces:

```
FanOut(N) → Barrier(min_completion, timeout, on_failure) → Reducer(mode=Debate)
```

This is a pure function of `Debate { roles }` configuration. No runtime state consulted.

### Example: Lowering Debate with N=2

Input:
```rust
StrategyIR::Debate {
    roles: vec![
        DebateRole { name: "Defender", model: "claude-opus-4", stance: Defend },
        DebateRole { name: "Critic", model: "gpt-4o", stance: Critique },
    ],
}
```

Output (PrimitiveGraph):
```yaml
nodes:
  - id: fanout_1
    kind: FanOut
    config: { count: 2 }

  - id: debater_1
    kind: LLMGenerate
    model: claude-opus-4
    artifact: Debate
    role: Defender

  - id: debater_2
    kind: LLMGenerate
    model: gpt-4o
    artifact: Debate
    role: Critic

  - id: barrier_1
    kind: Barrier
    config: { min_completion: 1.0, timeout: 60s, on_failure: continue }

  - id: reducer_1
    kind: Reducer
    config: { mode: Debate, model: claude-opus-4 }

edges:
  - from: fanout_1, to: debater_1
  - from: fanout_1, to: debater_2
  - from: debater_1, to: barrier_1
  - from: debater_2, to: barrier_1
  - from: barrier_1, to: reducer_1
```

### Optimization Legality

Optimizations applied to PrimitiveIR **MUST** preserve:

1. **Node semantics** — every primitive node's observable behavior is unchanged
2. **Dependency ordering** — no edge added or removed that changes execution order constraints
3. **Artifact ABI** — artifact types and versions between producer and consumer remain compatible
4. **Observable outputs** — the reduction artifact is semantically identical

### Validation Phases

**Strategy Validation** (pre-lowering):
- Required roles present
- Model assignments valid
- Configuration within bounds (e.g., `N ≥ 2` for Debate)

**Graph Validation** (post-lowering):
- Exactly one Reducer for Debate
- No debater depends on another debater
- Graph is acyclic
- All artifact types match between producers and consumers

**Execution Validation** (post-optimization):
- All nodes have model bindings
- No orphaned nodes
- Barrier predecessor count matches FanOut count

---

## Consequences

### Positive

1. **Runtime stays small** — The scheduler implements 5 primitives (FanOut, Barrier, Reducer, FeedbackLoop, ConditionalBranch), not N strategies. New strategies add lowering passes only.

2. **Plugin-extensible** — Third-party plugins register `Strategy` implementations. The runtime never changes. The plugin ecosystem is bounded by the Primitive IR, not by scheduler internals.

3. **Deterministic replay** — Byte-identical PrimitiveGraph from same IR + config. Enables graph hashing, caching, and regression testing across releases.

4. **Cross-strategy optimization** — The optimization pass can specialize any PrimitiveGraph: barrier elimination when N=1, fan-out collapsing for nested debates, concurrency tuning per provider.

5. **Observability** — PrimitiveGraph is serializable at every compiler stage. Graph diffing between releases, visualization, and execution simulation all come for free.

6. **Forward compatibility** — Strategy IR can evolve independently of Primitive IR. Old strategies produce old Strategy IR; lowering adapts to current primitives.

7. **Provenance** — The Artifact trait with versioned contracts ensures cross-release compatibility. Reducer records exactly which artifact versions it consumed.

### Negative

1. **Migration cost** — All existing strategies (Single, Consensus, Reflection, Debate, ReAct, Chain, Fusion) must be reimplemented as lowering passes. The old `Strategy::apply` trait is removed.

2. **Two-layer IR complexity** — Developers must understand Strategy IR and Primitive IR. Debugging across the lowering boundary requires both layers.

3. **Lowering pass overhead** — Every request now runs through a lowering pass. For simple strategies (Single), this is pure overhead. Optimization passes can shortcut: Single lowers to a single LLMGenerate node, bypassing all primitives.

### Replay & Evolution Policy

- Breaking changes to `Artifact` version numbers require new artifact structs (e.g., `DebateArtifactV2`). The reducer records which version it consumed.
- Breaking changes to `PrimitiveIR` require a new PrimitiveGraph version. Old execution logs remain replayable via the previous version's lowering pass.
- Strategy configuration is part of the deterministic lowering key. Changing strategy configuration changes the graph hash. This is intended.

---

## Migration Path

### Phase 1: Primitive IR and Lowering Trait

Add `PrimitiveIR` types alongside existing `ExecutionGraph`. Implement the `Strategy` trait revision with `descriptor()` and `lower()`. The existing `Strategy::apply` is deprecated but still supported for backward compatibility.

### Phase 2: Strategy-by-Strategy Migration

Each strategy is migrated independently:

1. **Single** → Trivial: lowers to single `LLMGenerate` node, no primitives needed
2. **Consensus** → `FanOut(N) → Barrier → Reducer(mode=consensus)`
3. **Reflection** → `LLMGenerate → LLMReview → ConditionalBranch → (FeedbackLoop | exit)`
4. **Debate** → `FanOut(N) → Barrier → Reducer(mode=debate)`
5. **ReAct** → `FeedbackLoop(max_iterations)`
6. **Chain** → Sequential `PrimitiveGraph` concatenation
7. **Fusion** → `FanOut(N) → Barrier → Reducer(mode=score)`

Each migration is a self-contained ADR review.

### Phase 3: Runtime Cleanup

Remove `Strategy::apply`. The scheduler only knows primitives. All strategy logic lives in lowering passes.

---

## Related Documents

- ADR-003: Compiler — establishes the compiler pass pipeline that this ADR extends
- ADR-017: Execution Runtime ABI — establishes ExecutionResult as the runtime contract
- `docs/specifications/strategy-api.md` — current Strategy trait specification (to be superseded)
- `docs/specifications/execution-graph.md` — current ExecutionGraph specification (to be extended for PrimitiveIR)
- `~/.agents/skills/structured-debate/SKILL.md` — structured debate specification that informed this ADR

---

## Unresolved Questions

1. **Should primitives be parameterized by artifact type at the IR level?** Currently the Reducer knows its mode (Debate, Consensus, etc.) but not the concrete artifact struct. This could be resolved by making `PrimitiveNode` generic over artifact type, or by keeping artifact dispatch at the lowering level.

2. **How should streaming interact with primitives?** A FanOut node with streaming debaters produces incremental artifacts. The Barrier must decide whether to wait for all increments or deliver partial results to the Reducer. This is deferred to a follow-up ADR on streaming semantics.

3. **Should the optimization pass be pluggable?** Like compiler passes, optimization passes could be registered by plugins. This would allow provider-specific optimizations (e.g., collapsing fan-outs for providers with native batch APIs). Deferred until provider-aware optimization is motivated by performance data.
