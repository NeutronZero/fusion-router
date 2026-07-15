use fusion_router::context::assembler::{ContextAssembler, DefaultContextAssembler};
use fusion_router::types::ChatCompletionRequest;

#[tokio::test]
async fn test_context_assembler_basic() {
    let assembler = DefaultContextAssembler::new();
    let request = ChatCompletionRequest {
        model: "test".to_string(),
        messages: vec![
            fusion_router::types::ChatMessage {
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
