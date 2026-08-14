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

### Chain
Sequential pipeline of sub-strategies where each stage feeds into the next.

**Configuration:**
- `stages: Vec<Box<dyn Strategy>>` — ordered list of strategies to execute in sequence

**Behaviour:**
- Applies each sub-strategy to the input node
- Connects each stage's exit to the next stage's entry via an `ExecutionEdge`
- Returns the combined subgraph with entry = first stage entry, exit = last stage exit
- A single-stage chain is a passthrough (identical to the single sub-strategy)

**Example:** `[Single, Reflection]` produces a Generate node → Review → Gate pipeline.

### ReAct (Reasoning + Acting)
Interleaves reasoning with looped execution, mimicking the ReAct pattern.

**Configuration:**
- `max_iterations: u32` — safety limit on reasoning loops (default: 10)

**Behaviour:**
- Creates a `Loop` control node as entry point
- Creates a `LLMGenerate` node for the reasoning step
- Forward edge: Loop → Generate (initial execution)
- Loop-back edge: Generate → Loop, condition: `"loop"` (repeat reasoning)
- The scheduler handles iteration; when the LLM produces a final answer (no tool call), the loop exits via the Generate node's outgoing edges

**Subgraph:**
```
Loop (entry) → Generate (reasoning) → (loop back to Loop)
                                    → (exit to parent graph)
```

### Debate
Multiple models discuss a topic, with a judge synthesising the final answer.

**Configuration:**
- `debaters: Vec<Box<dyn Strategy>>` — list of debater strategies (e.g., two `Single` strategies)
- `judge: Box<dyn Strategy>` — strategy that synthesises the debate

**Behaviour:**
- Each debater strategy is applied independently to the input node, producing parallel subgraphs
- All debater exits are connected to the judge's entry via `ExecutionEdge`
- The judge produces the final output

**Subgraph:**
```
Debater1 ──┐
           ├──→ Judge (exit)
Debater2 ──┘
```

### Fusion (v0.5.0+)
Panel of judges that independently evaluate and produce a consensus.
