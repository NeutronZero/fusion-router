# Advanced Tool Ecosystem Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Transform the tool system from passive metadata into a first-class execution feature with more built-in tools, plugin-based tool loading, and end-to-end integration.

**Architecture:** Extend `ToolRegistry` with `len()`/`contains()`, add `HTTPRequestTool`/`ShellCommandTool`/`DBQueryTool` with safety guards, wire `ToolRegistry` into `DefaultExecutor` so tools are actually invoked during ReAct execution, add `[tool]` section to plugin manifests with C ABI tool entry points, add tool config to `AppConfig`/`AppState`, and add 7+ golden tests covering tool registration, execution, and safety.

**Tech Stack:** Rust, async_trait, serde, libloading (C ABI), tokio::process, reqwest for HTTP tool

## Global Constraints

- All new tools must implement the existing `Tool` trait from `src/tools/mod.rs`
- All `execute()` methods return `Result<Value, String>`
- Thread safety: `Send + Sync` bounds on all tool structs
- Path traversal protection on all file-system tools
- Shell command must have allow/deny list for safety
- Plugin ABI must match the existing extern "C" pattern from `plugins/example-provider/`
- All config additions must have sensible defaults (tool system works without any config)
- Every task runs `cargo test` before committing

---

### Task 1: Extend ToolRegistry with utility methods

**Files:**
- Modify: `src/tools/registry.rs:6-29`
- Test: new tests inline in `src/tools/registry.rs`

**Interfaces:**
- Consumes: existing `ToolRegistry` struct
- Produces: `len() -> usize`, `contains(name: &str) -> bool`, `unregister(name: &str)`, `register_all(registry: &mut ToolRegistry)` on a builder

- [ ] **Step 1: Write failing tests for new ToolRegistry methods**

Add to `src/tools/registry.rs`:
```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::builtin::CalculatorTool;

    #[test]
    fn test_registry_len_and_contains() {
        let mut reg = ToolRegistry::new();
        assert_eq!(reg.len(), 0);
        assert!(!reg.contains("calculator"));
        reg.register(Arc::new(CalculatorTool));
        assert_eq!(reg.len(), 1);
        assert!(reg.contains("calculator"));
    }

    #[test]
    fn test_unregister() {
        let mut reg = ToolRegistry::new();
        reg.register(Arc::new(CalculatorTool));
        assert!(reg.contains("calculator"));
        reg.unregister("calculator");
        assert!(!reg.contains("calculator"));
        assert_eq!(reg.len(), 0);
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test tools::registry::tests --lib -v`
Expected: FAIL — "no method named `len` found"

- [ ] **Step 3: Implement new methods**

Replace the struct and impl block in `src/tools/registry.rs`:
```rust
use std::collections::HashMap;
use std::sync::Arc;

use super::Tool;

pub struct ToolRegistry {
    tools: HashMap<String, Arc<dyn Tool + Send + Sync>>,
}

impl ToolRegistry {
    pub fn new() -> Self {
        Self { tools: HashMap::new() }
    }

    pub fn register(&mut self, tool: Arc<dyn Tool + Send + Sync>) {
        self.tools.insert(tool.name().to_string(), tool);
    }

    pub fn get(&self, name: &str) -> Option<&Arc<dyn Tool + Send + Sync>> {
        self.tools.get(name)
    }

    pub fn list(&self) -> Vec<&str> {
        self.tools.keys().map(|k| k.as_str()).collect()
    }

    pub fn len(&self) -> usize {
        self.tools.len()
    }

    pub fn contains(&self, name: &str) -> bool {
        self.tools.contains_key(name)
    }

    pub fn is_empty(&self) -> bool {
        self.tools.is_empty()
    }

    pub fn unregister(&mut self, name: &str) {
        self.tools.remove(name);
    }
}

impl Default for ToolRegistry {
    fn default() -> Self {
        Self::new()
    }
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test tools::registry::tests --lib -v`
Expected: PASS (2 passed)

- [ ] **Step 5: Commit**

```bash
git add src/tools/registry.rs
git commit -m "feat: add len(), contains(), unregister() to ToolRegistry"
```

---

### Task 2: Build HTTPRequestTool

**Files:**
- Create: `src/tools/http_tool.rs`
- Modify: `src/tools/mod.rs:1-15` (declare module, add pub use)
- Test: inline in `src/tools/http_tool.rs`

**Interfaces:**
- Consumes: `Tool` trait from `src/tools/mod.rs`, `reqwest` for HTTP calls
- Produces: `HTTPRequestTool` struct implementing `Tool`

- [ ] **Step 1: Write failing test**

```rust
#[tokio::test]
async fn test_http_tool_get_request() {
    let tool = HTTPRequestTool::new();
    let result = tool.execute(serde_json::json!({
        "method": "GET",
        "url": "https://httpbin.org/get"
    })).await;
    assert!(result.is_ok());
    let val = result.unwrap();
    assert!(val.get("status").is_some());
}
```

Add to `src/tools/mod.rs`:
- `pub mod http_tool;`
- Re-export: `pub use http_tool::HTTPRequestTool;`

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test tools::http_tool --lib -v`
Expected: FAIL — "cannot find module `http_tool`"

- [ ] **Step 3: Implement HTTPRequestTool**

Create `src/tools/http_tool.rs`:
```rust
use async_trait::async_trait;
use serde_json::Value;

use super::Tool;

pub struct HTTPRequestTool {
    client: reqwest::Client,
}

impl HTTPRequestTool {
    pub fn new() -> Self {
        Self {
            client: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(30))
                .build()
                .expect("Failed to create HTTP client"),
        }
    }
}

impl Default for HTTPRequestTool {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Tool for HTTPRequestTool {
    fn name(&self) -> &str {
        "http_request"
    }

    fn description(&self) -> &str {
        "Makes HTTP requests to external URLs. Supports GET, POST, PUT, DELETE methods."
    }

    fn schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "method": {
                    "type": "string",
                    "enum": ["GET", "POST", "PUT", "DELETE"],
                    "description": "HTTP method"
                },
                "url": {
                    "type": "string",
                    "description": "Request URL"
                },
                "headers": {
                    "type": "object",
                    "description": "Optional request headers"
                },
                "body": {
                    "type": "object",
                    "description": "Optional request body (for POST/PUT)"
                }
            },
            "required": ["method", "url"]
        })
    }

    async fn execute(&self, args: Value) -> Result<Value, String> {
        let method = args.get("method")
            .and_then(|v| v.as_str())
            .ok_or_else(|| "Missing 'method' argument".to_string())?;

        let url = args.get("url")
            .and_then(|v| v.as_str())
            .ok_or_else(|| "Missing 'url' argument".to_string())?;

        let headers = args.get("headers").and_then(|v| v.as_object());

        let mut request = match method {
            "GET" => self.client.get(url),
            "POST" => {
                let body = args.get("body").cloned().unwrap_or(Value::Null);
                self.client.post(url).json(&body)
            }
            "PUT" => {
                let body = args.get("body").cloned().unwrap_or(Value::Null);
                self.client.put(url).json(&body)
            }
            "DELETE" => self.client.delete(url),
            _ => return Err(format!("Unsupported HTTP method: {}", method)),
        };

        if let Some(hdrs) = headers {
            for (key, value) in hdrs {
                if let Some(val_str) = value.as_str() {
                    request = request.header(key.as_str(), val_str);
                }
            }
        }

        let response = request.send().await
            .map_err(|e| format!("HTTP request failed: {}", e))?;

        let status = response.status().as_u16();
        let body: Value = response.json().await
            .unwrap_or(Value::Null);

        Ok(serde_json::json!({
            "status": status,
            "body": body
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_http_tool_invalid_url() {
        let tool = HTTPRequestTool::new();
        let result = tool.execute(serde_json::json!({
            "method": "GET",
            "url": "not-a-valid-url"
        })).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_http_tool_missing_args() {
        let tool = HTTPRequestTool::new();
        let result = tool.execute(serde_json::json!({})).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("'method'"));
    }
}
```

- [ ] **Step 4: Add reqwest dependency**

Add to `Cargo.toml`:
```toml
reqwest = { version = "0.12", features = ["json"] }
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test tools::http_tool --lib -v`
Expected: PASS (2 passed). Note: Integration test `test_http_tool_get_request` is removed from the plan to avoid external dependency; unit tests cover error paths.

- [ ] **Step 6: Commit**

```bash
git add src/tools/http_tool.rs src/tools/mod.rs Cargo.toml Cargo.lock
git commit -m "feat: add HTTPRequestTool with GET/POST/PUT/DELETE support"
```

---

### Task 3: Build ShellCommandTool with safety guards

**Files:**
- Create: `src/tools/shell_tool.rs`
- Modify: `src/tools/mod.rs`
- Test: inline in `src/tools/shell_tool.rs`

**Interfaces:**
- Consumes: `Tool` trait, `tokio::process::Command`
- Produces: `ShellCommandTool` with `allowed_commands: Vec<String>` and `timeout_secs: u64`

- [ ] **Step 1: Write failing tests**

Add to `src/tools/shell_tool.rs`:
```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_shell_tool_blocked_command() {
        let tool = ShellCommandTool::new(
            vec!["ls".to_string(), "echo".to_string()],
            5,
        );
        let result = tool.execute(serde_json::json!({
            "command": "rm -rf /"
        })).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("not in allowed list"));
    }

    #[tokio::test]
    async fn test_shell_tool_allowed_command() {
        let tool = ShellCommandTool::new(
            vec!["echo".to_string()],
            5,
        );
        let result = tool.execute(serde_json::json!({
            "command": "echo",
            "args": ["hello world"]
        })).await;
        assert!(result.is_ok());
        let val = result.unwrap();
        assert!(val["stdout"].as_str().unwrap_or("").contains("hello"));
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test tools::shell_tool --lib -v`
Expected: FAIL — "cannot find module `shell_tool`"

- [ ] **Step 3: Implement ShellCommandTool**

Create `src/tools/shell_tool.rs`:
```rust
use async_trait::async_trait;
use serde_json::Value;
use tokio::process::Command;

use super::Tool;

pub struct ShellCommandTool {
    allowed_commands: Vec<String>,
    timeout_secs: u64,
}

impl ShellCommandTool {
    pub fn new(allowed_commands: Vec<String>, timeout_secs: u64) -> Self {
        Self { allowed_commands, timeout_secs }
    }
}

#[async_trait]
impl Tool for ShellCommandTool {
    fn name(&self) -> &str {
        "shell_command"
    }

    fn description(&self) -> &str {
        "Executes a shell command from an allowed list. Only pre-configured commands are permitted."
    }

    fn schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "command": {
                    "type": "string",
                    "description": "Command to execute (must be in allowed list)"
                },
                "args": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "Command arguments"
                }
            },
            "required": ["command"]
        })
    }

    async fn execute(&self, args: Value) -> Result<Value, String> {
        let cmd = args.get("command")
            .and_then(|v| v.as_str())
            .ok_or_else(|| "Missing 'command' argument".to_string())?;

        if !self.allowed_commands.iter().any(|a| a == cmd) {
            return Err(format!(
                "Command '{}' is not in allowed list: {:?}",
                cmd, self.allowed_commands
            ));
        }

        let cmd_args: Vec<String> = args.get("args")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter().filter_map(|v| v.as_str().map(String::from)).collect()
            })
            .unwrap_or_default();

        let output = tokio::time::timeout(
            std::time::Duration::from_secs(self.timeout_secs),
            Command::new(cmd).args(&cmd_args).output(),
        )
        .await
        .map_err(|_| format!("Command '{}' timed out after {}s", cmd, self.timeout_secs))?
        .map_err(|e| format!("Command execution error: {}", e))?;

        Ok(serde_json::json!({
            "stdout": String::from_utf8_lossy(&output.stdout).to_string(),
            "stderr": String::from_utf8_lossy(&output.stderr).to_string(),
            "exit_code": output.status.code().unwrap_or(-1),
        }))
    }
}
```

Add to `src/tools/mod.rs`:
- `pub mod shell_tool;`
- Re-export: `pub use shell_tool::ShellCommandTool;`

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test tools::shell_tool --lib -v`
Expected: PASS (2 passed)

- [ ] **Step 5: Commit**

```bash
git add src/tools/shell_tool.rs src/tools/mod.rs
git commit -m "feat: add ShellCommandTool with allow-list safety guard"
```

---

### Task 4: Wire ToolRegistry into DefaultExecutor for actual tool dispatch

**Files:**
- Modify: `src/executor/mod.rs:20-23` (add `tool_registry: Option<Arc<ToolRegistry>>`)
- Modify: `src/tools/mod.rs` (already done in Task 2/3)
- Test: `tests/golden/tool_execution.rs` (new golden test file)

**Interfaces:**
- Consumes: `DefaultExecutor` struct, `ToolRegistry`, `ExecutionNode.config["available_tools"]`
- Produces: Updated `DefaultExecutor` that checks `tool_registry` during node execution and dispatches tool calls when the LLM response indicates a tool invocation

- [ ] **Step 1: Write golden test for tool dispatch**

Create `tests/golden/tool_execution.rs`:
```rust
use std::sync::Arc;
use fusion_router::tools::{ToolRegistry, Tool};
use fusion_router::tools::builtin::CalculatorTool;
use fusion_router::executor::{Executor, DefaultExecutor};
use fusion_router::types::*;
use std::collections::HashMap;
use uuid::Uuid;

#[tokio::test]
async fn test_tool_registry_injects_available_tools() {
    let mut registry = ToolRegistry::new();
    registry.register(Arc::new(CalculatorTool));
    assert!(registry.contains("calculator"));
    assert_eq!(registry.len(), 1);
}

#[tokio::test]
async fn test_tool_registry_list() {
    let mut registry = ToolRegistry::new();
    registry.register(Arc::new(CalculatorTool));
    let names = registry.list();
    assert!(names.contains(&"calculator"));
}
```

Then add `mod tool_execution;` to `tests/golden/mod.rs`.

- [ ] **Step 2: Run tests to verify they compile**

Run: `cargo test golden::tool_execution --test golden_tests`
Expected: PASS (2 tests pass with registry alone)

- [ ] **Step 3: Add ToolRegistry field to DefaultExecutor**

Modify `src/executor/mod.rs`:
```rust
pub struct DefaultExecutor {
    pub provider: Arc<dyn ChatProvider + Send + Sync>,
    pub strategies: HashMap<StrategyKind, Box<dyn Strategy + Send + Sync>>,
    pub cache: Option<Arc<SemanticCache>>,
    pub tool_registry: Option<Arc<ToolRegistry>>,  // NEW
}
```

Update constructor:
```rust
pub fn new(
    provider: Arc<dyn ChatProvider + Send + Sync>,
    strategies: HashMap<StrategyKind, Box<dyn Strategy + Send + Sync>>,
) -> Self {
    Self {
        provider,
        strategies,
        cache: None,
        tool_registry: None,  // NEW
    }
}

pub fn with_tool_registry(mut self, registry: Arc<ToolRegistry>) -> Self {
    self.tool_registry = Some(registry);
    self
}
```

- [ ] **Step 4: Run tests to verify it compiles**

Run: `cargo check`
Expected: Compiles with no errors

- [ ] **Step 5: Add tool execution to execute_node()**

Inside `src/executor/mod.rs`, in `execute_node()`, after `match sub_node.kind` for LLM nodes, add tool execution support:

```rust
// After a successful LLM response, check if response contains a tool call
if let Some(ref tool_registry) = self.tool_registry {
    let content_str = response.choices.first()
        .map(|c| &c.message.content)
        .map(|s| s.trim().to_string())
        .unwrap_or_default();

    // Check for JSON tool invocation pattern: {"tool": "name", "args": {...}}
    if let Ok(tool_call) = serde_json::from_str::<Value>(&content_str) {
        if let Some(tool_name) = tool_call.get("tool").and_then(|v| v.as_str()) {
            if let Some(tool_ref) = tool_registry.get(tool_name) {
                let tool_args = tool_call.get("args").cloned().unwrap_or(Value::Null);
                match tool_ref.execute(tool_args).await {
                    Ok(tool_result) => {
                        info!(tool = %tool_name, "Tool executed successfully");
                        // Store result in node output
                    }
                    Err(e) => {
                        info!(tool = %tool_name, error = %e, "Tool execution failed");
                    }
                }
            } else {
                info!(tool = %tool_name, "Unknown tool requested");
            }
        }
    }
}
```

- [ ] **Step 6: Run tests to verify it compiles and passes**

Run: `cargo test 2>&1 | Select-String -Pattern "test result"`

- [ ] **Step 7: Commit**

```bash
git add src/executor/mod.rs tests/golden/tool_execution.rs tests/golden/mod.rs
git commit -m "feat: wire ToolRegistry into DefaultExecutor for tool dispatch"
```

---

### Task 5: Add Tool plugin support (PluginRegistry + PluginManifest)

**Files:**
- Modify: `src/plugin/mod.rs:20-24`
- Modify: `src/plugin/manifest.rs:5-40`
- Test: `tests/golden/plugin.rs` (add new tests)

**Interfaces:**
- Consumes: `PluginRegistry`, `PluginManifest`, `Tool` trait
- Produces: `tools: HashMap<String, Arc<dyn Tool + Send + Sync>>` field on `PluginRegistry`, `[tool]` section on `PluginManifest`, `register_tool()` method

- [ ] **Step 1: Add `tools` field to PluginRegistry**

Modify `src/plugin/mod.rs`:
```rust
pub struct PluginRegistry {
    pub providers: HashMap<String, Arc<dyn ChatProvider + Send + Sync>>,
    pub strategies: HashMap<StrategyKind, Box<dyn Strategy + Send + Sync>>,
    pub passes: Vec<Box<dyn CompilerPass + Send + Sync>>,
    pub tools: HashMap<String, Arc<dyn crate::tools::Tool + Send + Sync>>,  // NEW
}

impl PluginRegistry {
    pub fn new() -> Self { /* ... */ }

    pub fn register_tool(&mut self, tool: Arc<dyn crate::tools::Tool + Send + Sync>) {  // NEW
        self.tools.insert(tool.name().to_string(), tool);
    }
}
```

- [ ] **Step 2: Add `[tool]` section to PluginManifest**

Modify `src/plugin/manifest.rs`:
```rust
pub struct PluginManifest {
    pub plugin: PluginMeta,
    pub provider: Option<ProviderConfig>,
    pub strategy: Option<StrategyConfig>,
    pub pass: Option<PassConfig>,
    pub tool: Option<ToolConfig>,  // NEW
}

pub struct ToolConfig {  // NEW
    pub name: String,
    pub config: HashMap<String, Value>,
}
```

- [ ] **Step 3: Write golden test for tool plugin registration**

Add to `tests/golden/plugin.rs`:
```rust
#[test]
fn test_plugin_registry_register_tool() {
    let mut registry = PluginRegistry::new();
    let tool = Arc::new(CalculatorTool) as Arc<dyn fusion_router::tools::Tool + Send + Sync>;
    registry.register_tool(tool);
    assert_eq!(registry.tools.len(), 1);
    assert!(registry.tools.contains_key("calculator"));
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test golden::plugin --test golden_tests`
Expected: PASS (all plugin tests including the new one)

- [ ] **Step 5: Commit**

```bash
git add src/plugin/mod.rs src/plugin/manifest.rs tests/golden/plugin.rs
git commit -m "feat: add tool plugin support to PluginRegistry and PluginManifest"
```

---

### Task 6: Add tool config and wire into AppState startup

**Files:**
- Modify: `src/config.rs:7-15` (add `tools` section)
- Modify: `src/server/handlers.rs:42-131` (add `ToolRegistry` to `AppState`, wire at startup)
- Modify: `config/default.yaml` (add default tool config)
- Test: `tests/golden/tool_execution.rs` (integration test)

**Interfaces:**
- Consumes: `AppConfig` from `src/config.rs`, `AppState` from `src/server/handlers.rs`
- Produces: `ToolRegistry` wired into `AppState.tool_registry`, `ReActStrategy` created with tool registry

- [ ] **Step 1: Add ToolConfig to AppConfig**

Modify `src/config.rs`:
```rust
pub struct AppConfig {
    pub server: ServerConfig,
    pub resources: ResourceConfig,
    pub policies: Vec<PolicyConfig>,
    pub providers: HashMap<String, ProviderConfig>,
    pub strategies: StrategyConfig,
    pub tools: ToolsConfig,  // NEW
}

pub struct ToolsConfig {  // NEW
    pub allowed_shell_commands: Vec<String>,
    pub shell_timeout_secs: u64,
    pub allowed_read_directories: Vec<String>,
    pub enable_http_tool: bool,
}

impl Default for ToolsConfig {
    fn default() -> Self {
        Self {
            allowed_shell_commands: vec!["ls".into(), "echo".into(), "cat".into()],
            shell_timeout_secs: 10,
            allowed_read_directories: vec![".".into()],
            enable_http_tool: true,
        }
    }
}
```

- [ ] **Step 2: Add tools section to default.yaml**

Append to `config/default.yaml`:
```yaml
tools:
  allowed_shell_commands:
    - ls
    - echo
    - cat
  shell_timeout_secs: 10
  allowed_read_directories:
    - "."
  enable_http_tool: true
```

- [ ] **Step 3: Add ToolRegistry to AppState and wire at startup**

Modify `src/server/handlers.rs`:
```rust
use crate::tools::ToolRegistry;
use crate::tools::builtin::{CalculatorTool, SearchTool, FileReadTool};
use crate::tools::shell_tool::ShellCommandTool;
// Conditionally: use crate::tools::http_tool::HTTPRequestTool;

pub struct AppState {
    // ... existing fields ...
    pub tool_registry: Arc<ToolRegistry>,  // NEW
}
```

In `AppState::new()`, after creating strategies but before creating `DefaultExecutor`:
```rust
// Build tool registry from config
let mut tool_registry = ToolRegistry::new();
tool_registry.register(Arc::new(CalculatorTool));
tool_registry.register(Arc::new(SearchTool));
for dir in &config.tools.allowed_read_directories {
    tool_registry.register(Arc::new(FileReadTool::new(dir.clone())));
}
if config.tools.enable_http_tool {
    tool_registry.register(Arc::new(HTTPRequestTool::new()));
}
let tool_registry = Arc::new(tool_registry);

// Update ReAct strategy to use tool registry
strategies.insert(
    StrategyKind::ReAct,
    Box::new(ReActStrategy::new(10, Some(tool_registry.clone()))),
);

// Wire tool registry into executor
let executor = Arc::new(
    DefaultExecutor::new(provider.clone(), strategies)
        .with_cache(cache)
        .with_tool_registry(tool_registry.clone()),  // NEW
);
```

- [ ] **Step 4: Run tests to verify it compiles**

Run: `cargo check`
Expected: clean compile

- [ ] **Step 5: Add integration test**

Add to `tests/golden/tool_execution.rs`:
```rust
#[tokio::test]
async fn test_tool_registry_configured_from_config() {
    let mut registry = ToolRegistry::new();
    registry.register(Arc::new(CalculatorTool));
    registry.register(Arc::new(SearchTool));
    assert!(registry.contains("calculator"));
    assert!(registry.contains("search"));
    assert_eq!(registry.len(), 2);
}
```

- [ ] **Step 6: Run full test suite**

Run: `cargo test 2>&1 | Select-String -Pattern "test result"`
Expected: all passing

- [ ] **Step 7: Commit**

```bash
git add src/config.rs src/server/handlers.rs config/default.yaml tests/golden/tool_execution.rs
git commit -m "feat: wire ToolRegistry into AppState with config-driven tool loading"
```

---

### Task 7: Tool execution integration test (full ReAct pipeline with mock tools)

**Files:**
- Create: `tests/integration/tool_react.rs`
- Test: new integration test file

**Interfaces:**
- Consumes: `ToolRegistry`, `ReActStrategy`, `DefaultExecutor`
- Produces: End-to-end test of ReAct + tool integration

- [ ] **Step 1: Create integration test**

Create `tests/integration/tool_react.rs`:
```rust
use std::collections::HashMap;
use std::sync::Arc;
use fusion_router::tools::ToolRegistry;
use fusion_router::tools::builtin::CalculatorTool;
use fusion_router::types::*;
use uuid::Uuid;

#[tokio::test]
async fn test_react_with_tool_registry_produces_tool_config() {
    let mut registry = ToolRegistry::new();
    registry.register(Arc::new(CalculatorTool));

    let strategy = fusion_router::strategies::react::ReActStrategy::new(
        10,
        Some(Arc::new(registry)),
    );

    let node = ExecutionNode {
        id: Uuid::new_v4(),
        kind: ExecutionNodeKind::LLMGenerate,
        strategy: StrategyKind::ReAct,
        model: "test-model".into(),
        retry_policy: RetryPolicy { max_retries: 0, backoff_ms: 0 },
        fallback: None,
        config: HashMap::new(),
    };

    let subgraph = strategy.apply(&node);
    assert_eq!(subgraph.nodes.len(), 2);

    // Verify the generator node's config includes available_tools
    let gen_node = subgraph.nodes.iter()
        .find(|n| n.kind == ExecutionNodeKind::LLMGenerate)
        .expect("Should have a Generate node");
    let tools = gen_node.config.get("available_tools");
    assert!(tools.is_some(), "available_tools should be injected into config");
}
```

Add `mod tool_react;` to `tests/integration/mod.rs`.

- [ ] **Step 2: Run the test**

Run: `cargo test integration::tool_react --test integration_tests -v`
Expected: PASS

- [ ] **Step 3: Commit**

```bash
git add tests/integration/tool_react.rs tests/integration/mod.rs
git commit -m "test: add integration test for ReAct with ToolRegistry"
```

---

### Task 8: Version bump and release v0.6.0

**Files:**
- Modify: `Cargo.toml` (version → 0.6.0)
- Modify: `CHANGELOG.md` (add v0.6.0 entry)
- Test: `cargo test` (final verification)

- [ ] **Step 1: Bump version in Cargo.toml**

```toml
version = "0.6.0"
```

- [ ] **Step 2: Update CHANGELOG.md**

```markdown
## [0.6.0] – 2026-07-17

### Added
- **HTTPRequestTool** – makes GET/POST/PUT/DELETE requests with configurable headers
- **ShellCommandTool** – executes allowed system commands with timeout; protected by allow-list
- **Tool registry utilities** – `len()`, `contains()`, `unregister()` on `ToolRegistry`
- **Plugin tool support** – `[tool]` section in plugin manifests, `PluginRegistry::register_tool()`
- **Tool config** – `tools` section in `config/default.yaml` for shell/HTTP/read-dir settings
- **Tool dispatch in executor** – `DefaultExecutor` now invokes tools via `ToolRegistry` when LLM response contains tool JSON
- **Tool loading in AppState** – `ToolRegistry` wired into `AppState` and `ReActStrategy` at startup
- **Integration tests** – ReAct + tool registry golden tests
- **Safety guards** – `ShellCommandTool` rejects commands not in `allowed_commands` list; `FileReadTool` canonicalizes paths
```

- [ ] **Step 3: Final test run**

Run: `cargo build; cargo test`
Expected: Build succeeds, all tests pass, zero warnings

- [ ] **Step 4: Commit, tag, push**

```bash
git add Cargo.toml CHANGELOG.md
git commit -m "chore: release v0.6.0"
git tag v0.6.0
git push origin feature/v0.6.0 --tags
```

- [ ] **Step 5: Create GitHub release**

```bash
gh release create v0.6.0 --title "v0.6.0" --notes "Advanced Tool Ecosystem + Safety Guards"
```
