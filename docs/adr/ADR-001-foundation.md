# ADR-001: Foundation Architecture

## Status
Accepted

## Context
FusionRouter needs a modular, extensible architecture for routing LLM requests through a configurable pipeline.

## Decision
1. **HTTP-first**: The primary interface is an OpenAI-compatible HTTP API (`/v1/chat/completions`).
2. **Trait-driven**: All major subsystems expose traits, enabling test doubles and alternative implementations.
3. **Shared type system**: All subsystem communication uses types defined in `src/types/`.
4. **Async-native**: Built on tokio + axum for non-blocking I/O.
5. **Provider abstraction**: Providers implement a `Provider` trait, isolating LLM API details.

## Consequences
- Easy to test with mock providers.
- New providers can be added without changing core logic.
- Type system evolves in one place.
