# ADR-024: Declarative Policy Compilation & Node Metadata Annotations

- **Status**: Proposed
- **Date**: July 2026
- **Context**: FusionRouter v0.10.0 Security & Governance Subsystem
- **Deciders**: FusionRouter Core Architecture Team

---

## Context

Imperative runtime safety checks (e.g. `if command.is_dangerous() { approve() }`) mix governance with execution code. In v0.10.0, security, approval, budget limits, retries, and timeouts become declarative policies compiled directly into graph transformations.

---

## Decisions

### 1. Policy Compiler Pass

Declarative policies (defined via YAML/JSON or code) are compiled into a `PolicyIR` by the `PolicyCompiler`.

During graph compilation, the compiler executes a dedicated `PolicyCompilerPass`. If a `PrimitiveGraph` node invokes a capability bound to security or approval policies (e.g., `shell.exec`, `delete.file`, `payment.transfer`), the pass automatically rewrites the graph by prepending an `ApprovalNode` or `PolicyGuardNode`.

```text
Policy Declarations (YAML) ──► PolicyCompiler ──► PolicyIR
                                                      │
PrimitiveGraph ──► PolicyCompilerPass (Pass 1) ───────┼──► Transformed PrimitiveGraph
                                                      │    (With ApprovalNodes inserted)
```

### 2. Node Metadata Annotations

Instead of creating distinct wrapper structs for every runtime policy, runtime policies are attached as structured `NodeMetadata` annotations on graph nodes:

```rust
pub struct NodeMetadata {
    pub retry: Option<RetryPolicy>,
    pub timeout: Option<TimeoutPolicy>,
    pub approval: Option<ApprovalPolicy>,
    pub budget: Option<BudgetEnvelopePolicy>,
    pub concurrency: Option<ConcurrencyPolicy>,
    pub security_guards: Vec<SecurityGuardId>,
}
```

The compiler annotates nodes during compilation; the Scheduler inspects `NodeMetadata` at runtime to enforce retries, timeouts, and quota limits.

---

## Consequences

- Governance rules are fully auditable and deterministic.
- Security policies are enforced at the graph level prior to execution, preventing unvetted tool execution.
