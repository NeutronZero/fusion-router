# Contributing

## Engineering Guardrails

1. **Trait-driven design** — Define traits before implementations. All major subsystems expose a trait.
2. **No direct dependencies** — All subsystems communicate through traits and message types, never through direct coupling.
3. **Pure compiler passes** — Compiler passes are pure functions: `(IR) -> Result<IR, Error>`. No side effects.
4. **ADRs before code** — Write an Architecture Decision Record before implementing any major subsystem.
5. **Serialization tests** — All IR and graph types implement `Serialize`/`Deserialize` and are tested for round-trip stability.
6. **Golden tests** — Compiler outputs are tested against stored golden files.
7. **Deterministic compilation** — Given the same IR and config, the compiler must always produce the same graph.
