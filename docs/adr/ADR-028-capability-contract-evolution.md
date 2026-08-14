# ADR-028: Capability Contract Evolution & Semantic Versioning

- **Status**: Proposed
- **Date**: July 2026
- **Context**: FusionRouter v0.10.0 Ecosystem Compatibility & Contract Governance
- **Deciders**: FusionRouter Core Architecture Team

---

## Context

Once third-party capability plugins exist in the ecosystem, capabilities will evolve over time. Changes to `CapabilityContract` schemas, input requirements, permission scopes, or side-effect declarations must not break existing workflows or the Planner's resolution logic.

---

## Decisions

### 1. Semantic Versioning for Contracts

Every `CapabilityContract` adheres to Semantic Versioning (`MAJOR.MINOR.PATCH`):
- **MAJOR**: Breaking schema changes (removing an input field, changing data types, introducing non-optional inputs).
- **MINOR**: Backward-compatible additive changes (adding optional fields, expanding capability capabilities).
- **PATCH**: Backward-compatible metadata or bug fixes (doc updates, performance latency adjustments).

### 2. Capability Aliasing & Deprecation Policy

To support seamless upgrades without breaking existing workflow graphs:
- **Capability Aliases**: A new capability version can declare an `alias_for` field linking to legacy capability identifiers.
- **Deprecation Grace Period**: Deprecated capability contracts emit diagnostic warnings during capability resolution but remain executable for at least one minor release cycle before retirement.

### 3. Feature Flags & Compatibility Fallbacks

Planners can query capability contracts for feature flags (`supports_streaming`, `supports_cancellation`, `supports_batching`). If a target connector lacks an optional capability feature, the Capability Resolver automatically selects a compatible fallback or degrades gracefully.

---

## Consequences

- Third-party capability plugins can evolve independently without breaking core system stability.
- Backward compatibility is enforced at resolution time via formal contract semver policies.
