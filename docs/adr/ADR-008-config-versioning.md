# ADR-008: Configuration Versioning & Rollback Strategy

## Status
Accepted (AF-003 Frozen)

## Context
Modifying routing policies, budgets, or model targets at runtime must never risk breaking system operations without a recovery path.

## Decision
Implement snapshot-based configuration versioning (`ConfigVersion`) in `fusion-config` and `fusion-infrastructure`. Every mutation creates an immutable version record, enabling instant 1-click rollbacks.

## Alternatives Considered
- In-place mutation of static configuration files: Rejected due to inability to audit changes or perform instantaneous rollbacks.

## Consequences
- Guarantees Law 14 (Configuration Versioning).
- Full auditability of system configuration changes.
