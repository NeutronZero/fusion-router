# Governance: Architecture Process

## Overview
This document defines how architectural decisions and changes are governed in FusionRouter under **AF-003 Architecture Freeze**.

## Principles
1. **Charter Enforcement:** All subsystems must communicate through stable, versioned contracts. No component may bypass `fusion-compiler` or `fusion-planner`.
2. **3-Tier Dependency Hierarchy:** `Foundation -> Engine -> Platform -> Applications`. No upward crate dependencies are permitted.
3. **Conformance Testing:** All architectural laws and invariants are automatically verified by `tests/conformance.rs` in CI.
