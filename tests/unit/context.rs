use fusion_router::context::assembler::{ContextAssembler, DefaultContextAssembler, estimate_tokens};
use fusion_router::types::{ChatCompletionRequest, ChatMessage};

#[tokio::test]
async fn test_context_assembler_basic() {
    let assembler = DefaultContextAssembler::new();
    let request = ChatCompletionRequest {
        model: "test".to_string(),
        messages: vec![
            ChatMessage {
                role: "user".to_string(),
                content: "Hello".to_string(),
            },
        ],
        stream: false,
        temperature: Some(0.5),
        max_tokens: Some(2048),
        tools: None,
        files: None,
    };

    let snapshot = assembler.assemble(&request).await.unwrap();
    assert_eq!(snapshot.messages.len(), 1);
    assert_eq!(snapshot.max_tokens, 2048);
    assert_eq!(snapshot.temperature, 0.5);
}

#[test]
fn test_trim_messages_under_limit() {
    let assembler = DefaultContextAssembler::new();
    let msgs = vec![
        ChatMessage { role: "system".into(), content: "Be helpful".into() },
        ChatMessage { role: "user".into(), content: "Hi".into() },
    ];
    let trimmed = assembler.trim_messages(&msgs, estimate_tokens("Be helpfulHi") + 10);
    assert_eq!(trimmed.len(), 2);
}

#[test]
fn test_trim_messages_drops_oldest() {
    let assembler = DefaultContextAssembler::new();
    let msgs = vec![
        ChatMessage { role: "system".into(), content: "Keep me".into() },
        ChatMessage { role: "user".into(), content: "A".repeat(200) },
        ChatMessage { role: "user".into(), content: "B".repeat(200) },
        ChatMessage { role: "user".into(), content: "C".repeat(200) },
    ];
    let trimmed = assembler.trim_messages(&msgs, 110);
    assert!(trimmed.len() < 4, "should drop messages");
    assert_eq!(trimmed[0].role, "system", "system message kept");
}

#[test]
fn test_trim_messages_preserves_system() {
    let assembler = DefaultContextAssembler::new();
    let msgs = vec![
        ChatMessage { role: "system".into(), content: "You are a helpful assistant".into() },
        ChatMessage { role: "user".into(), content: "Hello".into() },
        ChatMessage { role: "user".into(), content: "How are you?".into() },
    ];
    let trimmed = assembler.trim_messages(&msgs, 5);
    assert!(trimmed.len() <= 1, "only system may survive");
    if !trimmed.is_empty() {
        assert_eq!(trimmed[0].role, "system");
    }
}
