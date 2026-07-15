use fusion_router::requirements::extractor::{DefaultRequirementsExtractor, RequirementsExtractor};
use fusion_router::types::{ChatMessage, ContextSnapshot, FileRef, Intent, Complexity};

fn make_snapshot(messages: Vec<(&str, &str)>, files: Vec<&str>) -> ContextSnapshot {
    ContextSnapshot {
        messages: messages.iter().map(|(r, c)| ChatMessage {
            role: r.to_string(),
            content: c.to_string(),
        }).collect(),
        files: files.iter().map(|f| FileRef {
            name: f.to_string(),
            content: "dummy".to_string(),
            mime_type: None,
        }).collect(),
        tools: vec![],
        max_tokens: 4096,
        temperature: 0.7,
    }
}

#[test]
fn test_intent_classification_code() {
    let extractor = DefaultRequirementsExtractor;
    let ctx = make_snapshot(vec![("user", "Write a function to sort an array in Rust")], vec![]);
    let req = extractor.extract(&ctx);
    assert_eq!(req.intent, Intent::Code);
}

#[test]
fn test_intent_classification_debug() {
    let extractor = DefaultRequirementsExtractor;
    let ctx = make_snapshot(vec![("user", "Fix this bug: the program crashes on startup")], vec![]);
    let req = extractor.extract(&ctx);
    assert_eq!(req.intent, Intent::Debug);
}

#[test]
fn test_intent_classification_general() {
    let extractor = DefaultRequirementsExtractor;
    let ctx = make_snapshot(vec![("user", "What is the weather today?")], vec![]);
    let req = extractor.extract(&ctx);
    assert_eq!(req.intent, Intent::General);
}

#[test]
fn test_complexity_low() {
    let extractor = DefaultRequirementsExtractor;
    let ctx = make_snapshot(vec![("user", "Hi")], vec![]);
    let req = extractor.extract(&ctx);
    assert_eq!(req.complexity, Complexity::Low);
}

#[test]
fn test_complexity_high_with_files() {
    let extractor = DefaultRequirementsExtractor;
    let ctx = make_snapshot(vec![("user", "Review this code")], vec!["a.rs", "b.rs", "c.rs", "d.rs", "e.rs", "f.rs"]);
    let req = extractor.extract(&ctx);
    assert_eq!(req.complexity, Complexity::High);
}
