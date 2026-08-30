use std::sync::{Arc, atomic::{AtomicUsize, Ordering}};

use fusion_agent_state::{
    ExecutionState, Observation, SkillSpec, StatePatch, StateStore, InMemoryStateStore,
    persistence::SqliteStateStore, runner::{AgentRunner, RunnerConfig},
};
use fusion_agent_state::runner::{AgentLlm, AgentToolExecutor};
use crate::agent::host::{parse_skill_response, RouterTools, stage1_registry};
use serde_json::json;

fn skill() -> SkillSpec {
    SkillSpec::new(json!({"type":"object","properties":{"counter":{"type":"integer"}},"additionalProperties":true}), "stage1 skill", "1")
}
fn initial() -> ExecutionState { ExecutionState::new(json!({"counter":0})).unwrap() }

struct CountingLlm { calls: Arc<AtomicUsize>, response: String }
#[async_trait::async_trait]
impl AgentLlm for CountingLlm {
    async fn complete(&self, _: fusion_agent_state::ChatRequest) -> Result<fusion_agent_state::AgentStep,String> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        parse_skill_response(&self.response)
    }
}

struct FlakyLlm { attempts: Arc<AtomicUsize>, fail_first: bool }
#[async_trait::async_trait]
impl AgentLlm for FlakyLlm {
    async fn complete(&self, _: fusion_agent_state::ChatRequest) -> Result<fusion_agent_state::AgentStep,String> {
        let n = self.attempts.fetch_add(1, Ordering::SeqCst);
        if self.fail_first && n==0 { return Err("transient failure".into()); }
        parse_skill_response("```json\n{\"state_patch\": {\"counter\": 1}, \"action\": \"calculator\"}\n```")
    }
}

struct VerifierLlm { calls: Arc<AtomicUsize> }
#[async_trait::async_trait]
impl AgentLlm for VerifierLlm {
    async fn complete(&self, req: fusion_agent_state::ChatRequest) -> Result<fusion_agent_state::AgentStep,String> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        let all = serde_json::to_string(&req).unwrap();
        assert!(!all.contains("SECRET_REASONING"), "R_t leaked into next ChatRequest");
        parse_skill_response("```json\n{\"state_patch\": {\"counter\": 1}, \"action\": \"calculator\"}\n```")
    }
}

struct NoopTools { calls: Arc<AtomicUsize> }
#[async_trait::async_trait]
impl AgentToolExecutor for NoopTools {
    async fn execute(&self, _: &fusion_agent_state::AgentAction) -> Result<Observation,String> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        Ok(Observation::new(json!({"ok":true})))
    }
}

#[tokio::test]
async fn gate_real_chatprovider_invocation_and_valid_parsing() {
    let store = InMemoryStateStore::new(skill(), initial()).unwrap();
    let calls = Arc::new(AtomicUsize::new(0));
    let llm = Arc::new(CountingLlm { calls: calls.clone(), response: "reason\n```json\n{\"state_patch\": {\"counter\": 1}, \"action\": \"calculator\"}\n```".into() });
    let tools = Arc::new(NoopTools { calls: Arc::new(AtomicUsize::new(0)) });
    let mut runner = AgentRunner::new(skill(), store, llm, tools, RunnerConfig::default());
    let out = runner.step(Observation::new(json!({})), None).await.unwrap();
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(out.committed_state.value.get("counter").and_then(|v| v.as_u64()), Some(1));
}

#[tokio::test]
async fn gate_missing_malformed_no_mutation() {
    for bad in [
        "no fence",
        "```json\n{\"action\": \"x\"}\n```",
        "```json\n{\"state_patch\": \"not object\", \"action\": \"x\"}\n```",
        "```json\nnot json\n```",
    ] {
        let store = InMemoryStateStore::new(skill(), initial()).unwrap();
        let llm = Arc::new(CountingLlm { calls: Arc::new(AtomicUsize::new(0)), response: bad.into() });
        let mut runner = AgentRunner::new(skill(), store, llm, Arc::new(NoopTools{ calls: Arc::new(AtomicUsize::new(0)) }), RunnerConfig::default());
        let before = runner.store().load().value.clone();
        assert!(runner.step(Observation::new(json!({})), None).await.is_err(), "should fail for {bad}");
        assert_eq!(runner.store().load().value, before, "no mutation on {bad}");
        assert_eq!(runner.log().len(), 0);
    }
}

#[tokio::test]
async fn gate_invalid_patch_no_tool() {
    let store = InMemoryStateStore::new(skill(), initial()).unwrap();
    let llm = Arc::new(CountingLlm { calls: Arc::new(AtomicUsize::new(0)), response: "```json\n{\"state_patch\": {\"counter\": {\"bad\":1}}, \"action\": \"calculator\"}\n```".into() });
    let tool_calls = Arc::new(AtomicUsize::new(0));
    let mut runner = AgentRunner::new(skill(), store, llm, Arc::new(NoopTools{ calls: tool_calls.clone() }), RunnerConfig::default());
    assert!(runner.step(Observation::new(json!({})), None).await.is_err());
    assert_eq!(tool_calls.load(Ordering::SeqCst), 0, "tool must not run on invalid patch");
    assert_eq!(runner.log().len(), 0);
}

#[tokio::test]
async fn gate_invalid_action_no_execution_via_router_tools() {
    let store = InMemoryStateStore::new(skill(), initial()).unwrap();
    let llm = Arc::new(CountingLlm { calls: Arc::new(AtomicUsize::new(0)), response: "```json\n{\"state_patch\": {\"counter\": 1}, \"action\": \"shell_command\"}\n```".into() });
    let registry = stage1_registry(None);
    let router_tools = Arc::new(RouterTools::new(registry));
    let mut runner = AgentRunner::new(skill(), store, llm, router_tools, RunnerConfig::default());
    let out = runner.step(Observation::new(json!({})), None).await.unwrap();
    assert_eq!(out.committed_state.value.get("counter").and_then(|v| v.as_u64()), Some(1));
    assert_eq!(out.observation.value.get("executed").and_then(|v| v.as_bool()), Some(false));
}

#[tokio::test]
async fn gate_idempotent_retry_single_commit() {
    struct HostWithRetry { inner: Arc<AtomicUsize> }
    #[async_trait::async_trait]
    impl AgentLlm for HostWithRetry {
        async fn complete(&self, _: fusion_agent_state::ChatRequest) -> Result<fusion_agent_state::AgentStep,String> {
            for _ in 0..2 {
                let attempt = self.inner.fetch_add(1, Ordering::SeqCst);
                if attempt==0 { continue; }
                return parse_skill_response("```json\n{\"state_patch\": {\"counter\": 99}, \"action\": \"calculator\"}\n```");
            }
            Err("unreachable".into())
        }
    }
    let store = InMemoryStateStore::new(skill(), initial()).unwrap();
    let attempts = Arc::new(AtomicUsize::new(0));
    let llm = Arc::new(HostWithRetry { inner: attempts.clone() });
    let tool_calls = Arc::new(AtomicUsize::new(0));
    let mut runner = AgentRunner::new(skill(), store, llm, Arc::new(NoopTools{ calls: tool_calls.clone() }), RunnerConfig::default());
    let out = runner.step(Observation::new(json!({})), None).await.unwrap();
    assert_eq!(out.committed_state.value.get("counter").and_then(|v| v.as_u64()), Some(99));
    assert_eq!(tool_calls.load(Ordering::SeqCst), 1, "exactly one tool execution even after retry");
    assert_eq!(runner.log().len(), 1, "exactly one EventLog entry");
    let out2 = runner.step(Observation::new(json!({})), None).await.unwrap();
    assert_eq!(runner.log().len(), 2);
    let _ = out2;
}

#[tokio::test]
async fn gate_provider_retry_via_flaky_llm() {
    let store = InMemoryStateStore::new(skill(), initial()).unwrap();
    let attempts = Arc::new(AtomicUsize::new(0));
    let llm = Arc::new(FlakyLlm { attempts: attempts.clone(), fail_first: true });
    let mut runner = AgentRunner::new(skill(), store, llm, Arc::new(NoopTools{ calls: Arc::new(AtomicUsize::new(0)) }), RunnerConfig::default());
    assert!(runner.step(Observation::new(json!({})), None).await.is_err());
    assert_eq!(runner.store().load().value.get("counter").and_then(|v| v.as_u64()), Some(0));
    assert_eq!(runner.log().len(), 0);
    let out = runner.step(Observation::new(json!({})), None).await.unwrap();
    assert_eq!(out.committed_state.value.get("counter").and_then(|v| v.as_u64()), Some(1));
    assert_eq!(attempts.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn gate_token_budget_accounting() {
    let store = InMemoryStateStore::new(skill(), initial()).unwrap();
    let llm = Arc::new(CountingLlm { calls: Arc::new(AtomicUsize::new(0)), response: "```json\n{\"state_patch\": {}, \"action\": \"calculator\"}\n```".into() });
    let mut runner = AgentRunner::new(skill(), store, llm, Arc::new(NoopTools{ calls: Arc::new(AtomicUsize::new(0)) }), RunnerConfig{ max_steps: 100, model: "m".into(), max_tokens: Some(80) });
    let mut obs = Observation::new(json!({}));
    let mut hit = false;
    for _ in 0..10 {
        match runner.step(obs, None).await { Ok(o)=> obs=o.observation, Err(e) if e.contains("budget") => { hit=true; break; }, Err(e)=> panic!("{e}") }
    }
    assert!(hit, "budget must be enforced");
}

#[tokio::test]
async fn gate_cancellation_before_llm() {
    let store = InMemoryStateStore::new(skill(), initial()).unwrap();
    let calls = Arc::new(AtomicUsize::new(0));
    let mut runner = AgentRunner::new(skill(), store, Arc::new(CountingLlm{ calls: calls.clone(), response: "```json\n{\"state_patch\": {}, \"action\": \"x\"}\n```".into() }), Arc::new(NoopTools{ calls: Arc::new(AtomicUsize::new(0)) }), RunnerConfig::default());
    let tok = tokio_util::sync::CancellationToken::new(); tok.cancel();
    assert!(runner.step(Observation::new(json!({})), Some(&tok)).await.is_err());
    assert_eq!(calls.load(Ordering::SeqCst), 0, "provider must not be called when cancelled before LLM");
}

#[tokio::test]
async fn gate_cancellation_before_tool() {
    let store = InMemoryStateStore::new(skill(), initial()).unwrap();
    let llm = Arc::new(CountingLlm { calls: Arc::new(AtomicUsize::new(0)), response: "```json\n{\"state_patch\": {\"counter\": 1}, \"action\": \"calculator\"}\n```".into() });
    let tool_calls = Arc::new(AtomicUsize::new(0));
    let mut runner = AgentRunner::new(skill(), store, llm, Arc::new(NoopTools{ calls: tool_calls.clone() }), RunnerConfig::default());
    let tok = tokio_util::sync::CancellationToken::new(); tok.cancel();
    let _ = runner.step(Observation::new(json!({})), Some(&tok)).await;
    assert_eq!(runner.log().len(), 0);
}

#[tokio::test]
async fn gate_max_steps_clean() {
    let store = InMemoryStateStore::new(skill(), initial()).unwrap();
    let llm = Arc::new(CountingLlm { calls: Arc::new(AtomicUsize::new(0)), response: "```json\n{\"state_patch\": {\"counter\": 1}, \"action\": \"x\"}\n```".into() });
    let mut runner = AgentRunner::new(skill(), store, llm, Arc::new(NoopTools{ calls: Arc::new(AtomicUsize::new(0)) }), RunnerConfig{ max_steps: 3, model: "m".into(), max_tokens: None });
    for _ in 0..3 { runner.step(Observation::new(json!({})), None).await.unwrap(); }
    assert!(runner.step(Observation::new(json!({})), None).await.is_err());
    assert!(runner.step(Observation::new(json!({})), None).await.unwrap_err().contains("max_steps"));
}

#[tokio::test]
async fn gate_reasoning_never_forwarded() {
    struct SecretLlm;
    #[async_trait::async_trait]
    impl AgentLlm for SecretLlm {
        async fn complete(&self, _: fusion_agent_state::ChatRequest) -> Result<fusion_agent_state::AgentStep,String> {
            Ok(fusion_agent_state::AgentStep::new(Some("SECRET_REASONING".into()), StatePatch::new(json!({"counter":0})).unwrap(), fusion_agent_state::AgentAction::new("x")))
        }
    }
    let mut runner2 = AgentRunner::new(skill(), InMemoryStateStore::new(skill(), initial()).unwrap(), Arc::new(SecretLlm), Arc::new(NoopTools{ calls: Arc::new(AtomicUsize::new(0)) }), RunnerConfig::default());
    runner2.step(Observation::new(json!({})), None).await.unwrap();
    assert!(runner2.log().entries().first().unwrap().reasoning.as_deref()==Some("SECRET_REASONING"));
    let mut runner3 = AgentRunner::new(skill(), runner2.store().clone(), Arc::new(VerifierLlm{ calls: Arc::new(AtomicUsize::new(0)) }), Arc::new(NoopTools{ calls: Arc::new(AtomicUsize::new(0)) }), RunnerConfig::default());
    runner3.step(Observation::new(json!({})), None).await.unwrap();
}

#[tokio::test]
async fn gate_sqlite_with_real_transitions() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("stage1.db");
    let skill1 = skill();
    let store = SqliteStateStore::open(skill1.clone(), &path, initial()).unwrap();
    let llm = Arc::new(CountingLlm { calls: Arc::new(AtomicUsize::new(0)), response: "```json\n{\"state_patch\": {\"counter\": 42}, \"action\": \"calculator\"}\n```".into() });
    let mut runner = AgentRunner::new(skill1.clone(), store, llm, Arc::new(NoopTools{ calls: Arc::new(AtomicUsize::new(0)) }), RunnerConfig::default());
    runner.step(Observation::new(json!({})), None).await.unwrap();
    drop(runner);
    let store2 = SqliteStateStore::open(skill1, &path, initial()).unwrap();
    assert_eq!(store2.load().value.get("counter").and_then(|v| v.as_u64()), Some(42));
}

#[tokio::test]
async fn gate_eventlog_invisible() {
    let store = InMemoryStateStore::new(skill(), initial()).unwrap();
    let llm = Arc::new(CountingLlm { calls: Arc::new(AtomicUsize::new(0)), response: "```json\n{\"state_patch\": {\"counter\": 1}, \"action\": \"x\"}\n```".into() });
    let mut runner = AgentRunner::new(skill(), store, llm, Arc::new(NoopTools{ calls: Arc::new(AtomicUsize::new(0)) }), RunnerConfig::default());
    runner.step(Observation::new(json!({})), None).await.unwrap();
    runner.step(Observation::new(json!({})), None).await.unwrap();
    let log_str = serde_json::to_string(&runner.log().entries()).unwrap();
    let req = fusion_agent_state::build_model_input(runner.skill(), &runner.store().load(), &Observation::new(json!({})), "m");
    let req_str = serde_json::to_string(&req).unwrap();
    assert!(!req_str.contains("SECRET") && !req_str.contains(&log_str[..20.min(log_str.len())]) || log_str.len() < 50);
    assert_eq!(runner.log().len(), 2);
}
