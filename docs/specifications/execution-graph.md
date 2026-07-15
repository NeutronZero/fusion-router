# Execution Graph Specification

The Execution Graph is a compiled, executable form of the Workflow IR.

## Structure

- `graph_id`: UUID
- `nodes`: List of execution-ready nodes with resolved models, retry policies, fallbacks
- `edges`: Directed dependency edges
- `metadata`: Estimated cost, tokens, depth, node count

## Node Properties

- `id`: UUID
- `kind`: LLMGenerate, LLMReview, LLMJudge, Transform, Gate, Aggregate
- `strategy`: How this node is expanded during execution
- `model`: Resolved model name
- `retry_policy`: Max retries and backoff
- `fallback`: Optional fallback model/provider
- `config`: Provider-specific configuration
