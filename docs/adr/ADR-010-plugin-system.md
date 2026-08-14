# ADR-010: Plugin System Design

## Status
Accepted

## Context
FusionRouter needs to support third-party extensions — custom providers, execution strategies, compiler passes, and tools — without modifying core code. Early versions had all providers and strategies hard-coded. As the system grows, a plugin system enables community contributions, vendor-specific integrations, and sandboxed execution of untrusted code.

## Decision

### 1. Four Extension Points

The `PluginRegistry` maintains collections for four plugin types:
- **Providers** (`Arc<dyn ChatProvider + Send + Sync>`) — LLM API adaptors, keyed by name
- **Strategies** (`Box<dyn Strategy + Send + Sync>`) — execution strategies like Consensus, Debate, Reflection, keyed by `StrategyKind`
- **Compiler Passes** (`Box<dyn CompilerPass + Send + Sync>`) — IR transformation passes, ordered by registration
- **Tools** (`BoxedTool = Arc<dyn Tool + Send + Sync>`) — tool callable during execution, keyed by name

### 2. Manifest-Based Discovery

Plugins are authored as TOML manifest files with a `PluginManifest` schema:
- `[plugin]` — name, version, optional description, entry point path
- `[provider]` — name, model list, config
- `[strategy]` — kind, config
- `[pass]` — name, config
- `[tool]` — name, config
- `[wasm]` — optional WASM function list (feature-gated)

`PluginManifest::discover(dir)` scans a directory for `.toml` files, deserializes each, and returns successfully parsed manifests. Malformed manifests are logged as warnings and skipped.

### 3. PluginManager Lifecycle

`PluginManager` owns the registry, loaded manifests, and (optionally) the WASM runtime:
- `new()` — creates empty manager with fresh registry
- `load_manifests(dir)` — discovers and loads all plugin TOML files from a directory
- `register_provider/strategy/pass/tool` — direct registration of native plugin components
- `registry()/registry_mut()` — access to the underlying `PluginRegistry`

### 4. WASM Plugin Runtime (Feature-Gated)

When `wasm-plugins` feature is enabled:
- Plugins with a `[wasm]` section are loaded as WebAssembly modules via `wasmtime`
- `load_wasm_plugin` reads the WASM binary from the manifest's entry path
- Modules are cached by name in the manager
- `call_wasm_function` provides a typed interface for invoking exported functions
- Runtime initialized lazily on first WASM load

### 5. Registration Hooks

Plugins register themselves at startup through the manager:
- Provider plugins create an `Arc<dyn ChatProvider>` and call `manager.register_provider()`
- Strategy plugins construct the strategy and call `manager.register_strategy()`
- Compiler passes are appended to the pass pipeline
- Tools are registered by name for invocation during execution

### 6. No Hot-Reloading

Plugins are loaded once at startup and are immutable for the lifetime of the server. Dynamic re-registration is not supported — changing plugins requires a restart.

## Consequences

- The four extension points cover all major customization surfaces
- TOML manifests are human-readable and easy to author
- WASM sandboxing enables safe execution of third-party plugins
- Feature-gating WASM keeps the dependency tree lean for deployments that don't need it
- No hot-reload simplifies the runtime model at the cost of requiring restarts for plugin changes
- Malformed manifests are non-fatal — one bad plugin doesn't block others
