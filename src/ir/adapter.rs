//! Adapter between the v0.13 `WorkflowIR` contract (`fusion_ir`) and the live
//! v0.12 `crate::types::WorkflowIR` consumed by `build_compiler`.
//!
//! Deterministic by construction: string node ids map to UUIDs via a fixed
//! namespace (v5 hashing), so the same IR always yields the same `types::WorkflowIR`.

use crate::types::{IRNodeKind, WorkflowIR as TypesWorkflowIR};
use fusion_ir::{WorkflowIR, WorkflowNodeKind};

/// Stable v5 namespace prefixing all string id hashing in the contract adapters.
const ID_NAMESPACE: uuid::Uuid = uuid::Uuid::from_u128(0x46f2_0a17_8c4e_4b3a_9d19_63c7_81b2_90e1);

/// Deterministic `Uuid` from an arbitrary string id (parses real UUIDs as-is).
pub fn uuid_for(id: &str) -> uuid::Uuid {
    if let Ok(parsed) = uuid::Uuid::parse_str(id) {
        return parsed;
    }
    uuid::Uuid::new_v5(&ID_NAMESPACE, id.as_bytes())
}

/// Maps a v0.13 `WorkflowNodeKind` onto the v0.12 execution plane.
pub fn node_kind_of(kind: &WorkflowNodeKind) -> IRNodeKind {
    use fusion_ir::WorkflowNodeKind as K;
    match kind {
        K::Task | K::Tool | K::Retrieval | K::Memory => IRNodeKind::Generate,
        K::Review => IRNodeKind::Review,
        K::Judge => IRNodeKind::Judge,
        K::Security => IRNodeKind::Gate,
        K::Aggregation => IRNodeKind::Join,
        K::Output => IRNodeKind::Transform,
    }
}

/// Converts the v0.13 contract `WorkflowIR` into the live v0.12 `types::WorkflowIR`.
///
/// Errors only if an edge references a node that does not exist.
pub fn workflow_to_types(ir: &WorkflowIR) -> Result<TypesWorkflowIR, String> {
    let mut nodes = Vec::with_capacity(ir.nodes().len());
    for node in ir.nodes() {
        let mut config: std::collections::HashMap<String, serde_json::Value> =
            node.config().clone().into_iter().collect();
        if let Some(cap) = node.capability() {
            config.insert("capability".to_string(), serde_json::json!(cap));
        }
        config.insert("semantic_kind".to_string(), serde_json::json!(format!("{:?}", node.kind())));
        nodes.push(crate::types::IRNode {
            id: uuid_for(node.id()),
            kind: node_kind_of(&node.kind()),
            strategy: crate::types::StrategyKind::Single,
            model: None,
            config,
        });
    }

    let node_ids: std::collections::HashSet<uuid::Uuid> =
        nodes.iter().map(|n| n.id).collect();

    let mut edges = Vec::with_capacity(ir.edges().len());
    for edge in ir.edges() {
        let from = uuid_for(edge.from());
        let to = uuid_for(edge.to());
        if !node_ids.contains(&from) || !node_ids.contains(&to) {
            return Err(format!(
                "edge references unknown node ({from} -> {to})"
            ));
        }
        edges.push(crate::types::IREdge {
            from,
            to,
            condition: edge.condition().map(str::to_string),
        });
    }

    let metadata = ir.metadata();
    Ok(TypesWorkflowIR {
        plan_id: ir.workflow_id(),
        nodes,
        edges,
        metadata: crate::types::IRMetadata {
            policy_applied: metadata.policy_applied.clone(),
            estimated_cost: metadata.estimated_cost,
            estimated_tokens: metadata.estimated_tokens,
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use fusion_ir::WorkflowNodeKind;
    use fusion_ir::{WorkflowBuilder, WorkflowMetadata};
    use std::collections::BTreeMap;

    fn sample_ir() -> WorkflowIR {
        let mut config = BTreeMap::new();
        config.insert("goal".into(), serde_json::Value::String("build a parser".into()));
        WorkflowBuilder::new()
            .metadata(WorkflowMetadata {
                policy_applied: vec!["pol".into()],
                estimated_cost: 1.5,
                estimated_tokens: 100,
            })
            .add_node("n1", WorkflowNodeKind::Task, Some("CodeGeneration"))
            .unwrap()
            .with_config("n1", config)
            .unwrap()
            .add_node("n2", WorkflowNodeKind::Output, None)
            .unwrap()
            .sequential("n1", "n2")
            .unwrap()
            .build()
            .unwrap()
    }

    #[test]
    fn node_kind_mapping_covers_all_contract_kinds() {
        use fusion_ir::WorkflowNodeKind as K;
        let cases = [
            (K::Task, IRNodeKind::Generate),
            (K::Tool, IRNodeKind::Generate),
            (K::Retrieval, IRNodeKind::Generate),
            (K::Memory, IRNodeKind::Generate),
            (K::Review, IRNodeKind::Review),
            (K::Judge, IRNodeKind::Judge),
            (K::Security, IRNodeKind::Gate),
            (K::Aggregation, IRNodeKind::Join),
            (K::Output, IRNodeKind::Transform),
        ];
        for (src, expected) in cases {
            assert_eq!(node_kind_of(&src), expected);
        }
    }

    #[test]
    fn preserves_plan_shape_and_metadata() {
        let ir = sample_ir();
        let types = workflow_to_types(&ir).unwrap();
        assert_eq!(types.plan_id, ir.workflow_id());
        assert_eq!(types.nodes.len(), 2);
        assert_eq!(types.edges.len(), 1);
        assert_eq!(types.edges[0].from, uuid_for("n1"));
        assert_eq!(types.edges[0].to, uuid_for("n2"));
        assert_eq!(types.nodes[0].kind, IRNodeKind::Generate);
        assert_eq!(types.nodes[1].kind, IRNodeKind::Transform);
        assert_eq!(types.metadata.policy_applied, vec!["pol".to_string()]);
        assert_eq!(types.metadata.estimated_cost, 1.5);
        assert_eq!(types.metadata.estimated_tokens, 100);
    }

    #[test]
    fn deterministic_id_mapping() {
        let (a, b) = (uuid_for("n1"), uuid_for("n1"));
        assert_eq!(a, b);
        assert_ne!(a, uuid_for("n2"));
        assert_eq!(
            uuid_for("550e8400-e29b-41d4-a716-446655440000").to_string(),
            "550e8400-e29b-41d4-a716-446655440000"
        );
    }

    #[test]
    fn from_json_rejects_dangling_edges() {
        // Validated by fusion-ir itself; the adapter's own edge guard covers
        // non-validating IR sources (future plugin intake), so it is kept
        // defensively rather than unit-tested here.
        let json = r#"{
            "version": 1,
            "workflow_id": "550e8400-e29b-41d4-a716-446655440000",
            "nodes": [{"id": "n1", "kind": "Task", "capability": null, "config": {}}],
            "edges": [{"from": "n1", "to": "ghost", "kind": "Sequential", "condition": null}],
            "metadata": {"policy_applied": [], "estimated_cost": 0.0, "estimated_tokens": 0}
        }"#;
        assert!(WorkflowIR::from_json(json).is_err());
    }
}