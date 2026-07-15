# Strategy API Specification

## Strategy Trait

```rust
pub trait Strategy {
    fn apply(&self, node: &ExecutionNode) -> ExecutionSubgraph;
}
```

A strategy takes an `ExecutionNode` and returns an `ExecutionSubgraph` (one or more nodes with edges).

## Implemented Strategies

### Single
One LLM generation node. No additional nodes.

### Consensus
- N parallel LLM generation nodes (N configurable, default 3)
- One Judge node that compares outputs
- Edges from all generators to judge

### Reflection
- One Generate node
- One Review node (quality check)
- One Gate node (conditional: pass or regen)
- Sequential edges: Generate → Review → Gate

### Chain (Phase 8+)
Sequential LLM calls where each output feeds the next.

### Debate (Phase 8+)
Multiple LLMs discuss a topic, with a judge synthesizing.

### Fusion (Phase 8+)
Panel of judges that independently evaluate and produce a consensus.
