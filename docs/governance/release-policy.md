# Governance: Release & Versioning Policy

## Overview
FusionRouter releases follow strict Semantic Versioning tied to public contract compatibility.

## Minor Releases (0.x)
- May introduce new non-breaking features, providers, or scheduler strategies.
- Must NOT break public contracts (`WorkflowIR v1`, `Execution ABI v1`, `REST API v1`, `Plugin SDK v1`).

## Major Releases (1.0+)
- Required when introducing breaking contract changes (e.g., `v2`).
- Requires explicit migration tooling and deprecation notices.
