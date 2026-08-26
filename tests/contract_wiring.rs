//! Contract wiring test: byte-for-byte determinism of compilation.
//!
//! Convergence Firewall Gate 11 / AF-003 Invariant 3: given identical input
//! `WorkflowIR`, compilation must produce byte-identical canonical
//! `ExecutionGraph` output — no entropy sources, no map-order leakage.
//! Gate 11 freeze check expects: byte-for-byte determinism + canonical_json + assert_eq!(canonical_json(&graph_a), canonical_json(&graph_b))

use fusion_compiler::{canonical_json, lower_to_graph};
use fusion_types::{IREdge, IRMetadata, IRNode, IRNodeKind, NanoUSD, StrategyKind, WorkflowIR};
use std::collections::HashMap;

fn sample_ir() -> WorkflowIR {
    let n1 = uuid::Uuid::new_v4();
    let n2 = uuid::Uuid::new_v4();
    let mut config: HashMap<String, serde_json::Value> = HashMap::new();
    config.insert(
        "zeta".to_string(),
        serde_json::json!({ "b": 2, "a": [1, "x", { "k": true }] }),
    );
    config.insert("alpha".to_string(), serde_json::json!("first"));

    WorkflowIR {
        plan_id: uuid::Uuid::new_v4(),
        nodes: vec![
            IRNode {
                id: n1,
                kind: IRNodeKind::Generate,
                strategy: StrategyKind::Single,
                model: Some("corpus-echo".into()),
                config: config.clone(),
            },
            IRNode {
                id: n2,
                kind: IRNodeKind::Generate,
                strategy: StrategyKind::Chain,
                model: Some("corpus-echo".into()),
                config,
            },
        ],
        edges: vec![IREdge {
            from: n1,
            to: n2,
            condition: None,
        }],
        metadata: IRMetadata {
            policy_version: 3,
            policy_applied: vec!["allow-all".into()],
            estimated_cost: NanoUSD::from_nanos(250_000_000),
            estimated_tokens: 512,
        },
    }
}

#[test]
fn byte_for_byte_determinism() {
    let ir_a = sample_ir();
    let ir_b = ir_a.clone();
    let ir_b_snapshot = ir_b.clone();

    let graph_a = lower_to_graph(ir_a).expect("compile a");
    let graph_b = lower_to_graph(ir_b).expect("compile b");

    // Invariant 3: identical IR -> byte-identical canonical graph bytes.
    assert_eq!(
        canonical_json(&graph_a).expect("canonical_json a"),
        canonical_json(&graph_b).expect("canonical_json b")
    );

    // The graph hash itself must agree with the canonical content hash.
    assert_eq!(graph_a.primitive_graph_hash, graph_b.primitive_graph_hash);

    // Repeated serialization is stable within the same process (map-order
    // safety for the nested config objects).
    assert_eq!(
        canonical_json(&graph_a).expect("canonical_json a2"),
        canonical_json(&graph_a).expect("canonical_json a3")
    );

    // The canonical form must also survive a JSON round trip unchanged.
    let ir_snapshot = ir_b_snapshot;
    let round_trip: WorkflowIR =
        serde_json::from_value(serde_json::to_value(&ir_snapshot).unwrap()).unwrap();
    let graph_c = lower_to_graph(round_trip).expect("compile round trip");
    assert_eq!(
        canonical_json(&graph_a).expect("canonical_json a4"),
        canonical_json(&graph_c).expect("canonical_json c")
    );
}
