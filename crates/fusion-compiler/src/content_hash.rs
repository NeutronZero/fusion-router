//! Deterministic FNV-1a content hash over planning IR topology and binding.
//! Identical IR yields identical hash across runs (no entropy sources).

use fusion_types::WorkflowIR;
use std::collections::HashMap;

/// Canonical byte-stable serialization of a node config map.
///
/// Object keys are recursively sorted so the hash is immune to HashMap
/// iteration order (mirrors `canonical_json` in the crate root). Returns
/// `None` for empty configs so legacy graphs without config keep their
/// historical hash values.
fn canonical_config_json(config: &HashMap<String, serde_json::Value>) -> Option<String> {
    if config.is_empty() {
        return None;
    }
    fn canonicalize(value: &serde_json::Value) -> serde_json::Value {
        match value {
            serde_json::Value::Object(map) => {
                let mut keys: Vec<&String> = map.keys().collect();
                keys.sort();
                let mut out = serde_json::Map::new();
                for key in keys {
                    out.insert(key.clone(), canonicalize(&map[key]));
                }
                serde_json::Value::Object(out)
            }
            serde_json::Value::Array(items) => {
                serde_json::Value::Array(items.iter().map(canonicalize).collect())
            }
            other => other.clone(),
        }
    }
    let value =
        serde_json::Value::Object(config.iter().map(|(k, v)| (k.clone(), v.clone())).collect());
    Some(serde_json::to_string(&canonicalize(&value)).expect("config value always serializes"))
}

pub fn compute_workflow_content_hash(ir: &WorkflowIR) -> u64 {
    const OFFSET: u64 = 0xcbf29ce484222325;
    const PRIME: u64 = 0x100000001b3;
    let mut h = OFFSET;
    let mix = |h: &mut u64, byte: u8| {
        *h ^= byte as u64;
        *h = h.wrapping_mul(PRIME);
    };
    let mix_u64 = |h: &mut u64, v: u64| {
        for b in v.to_le_bytes() {
            mix(h, b);
        }
    };
    let mix_str = |h: &mut u64, s: &str| {
        mix_u64(h, s.len() as u64);
        for b in s.as_bytes() {
            mix(h, *b);
        }
    };

    mix_u64(&mut h, ir.plan_id.as_u128() as u64);
    mix_u64(&mut h, (ir.plan_id.as_u128() >> 64) as u64);
    mix_u64(&mut h, ir.metadata.policy_version);
    mix_u64(&mut h, ir.nodes.len() as u64);

    let mut nodes: Vec<_> = ir.nodes.iter().collect();
    nodes.sort_by_key(|n| n.id);
    for n in nodes {
        mix_u64(&mut h, n.id.as_u128() as u64);
        mix_u64(&mut h, (n.id.as_u128() >> 64) as u64);
        mix_str(&mut h, &format!("{:?}", n.kind));
        mix_str(&mut h, &format!("{:?}", n.strategy));
        if let Some(ref m) = n.model {
            mix_str(&mut h, m);
        } else {
            mix(&mut h, 0);
        }
        // Mix a canonical serialization of the node config so graphs differing
        // only in config do not collide. Empty configs are skipped entirely,
        // preserving hash values for legacy config-free graphs.
        if let Some(canonical) = canonical_config_json(&n.config) {
            mix_str(&mut h, &canonical);
        }
    }

    let mut edges: Vec<_> = ir.edges.iter().collect();
    edges.sort_by_key(|e| (e.from, e.to));
    for e in edges {
        mix_u64(&mut h, e.from.as_u128() as u64);
        mix_u64(&mut h, e.to.as_u128() as u64);
        if let Some(ref c) = e.condition {
            mix_str(&mut h, c);
        } else {
            mix(&mut h, 0);
        }
    }
    h
}

#[cfg(test)]
mod tests {
    use super::*;
    use fusion_core::NanoUSD;
    use fusion_types::*;
    use std::collections::HashMap;

    fn sample_ir() -> WorkflowIR {
        let id = uuid::Uuid::nil();
        WorkflowIR {
            plan_id: id,
            nodes: vec![IRNode {
                id,
                kind: IRNodeKind::Generate,
                strategy: StrategyKind::Single,
                model: Some("m".into()),
                config: HashMap::new(),
            }],
            edges: vec![],
            metadata: IRMetadata {
                policy_version: 0,
                policy_applied: vec![],
                estimated_cost: NanoUSD::ZERO,
                estimated_tokens: 0,
            },
        }
    }

    #[test]
    fn hash_is_stable() {
        let a = compute_workflow_content_hash(&sample_ir());
        let b = compute_workflow_content_hash(&sample_ir());
        assert_eq!(a, b);
        assert_ne!(a, 0);
    }

    fn sample_ir_with_config(config: HashMap<String, serde_json::Value>) -> WorkflowIR {
        let mut ir = sample_ir();
        ir.nodes[0].strategy = StrategyKind::Consensus;
        ir.nodes[0].config = config;
        ir
    }

    #[test]
    fn hash_differs_when_consensus_config_count_differs() {
        let mut three = HashMap::new();
        three.insert("count".into(), serde_json::json!(3));
        let mut five = HashMap::new();
        five.insert("count".into(), serde_json::json!(5));

        let a = compute_workflow_content_hash(&sample_ir_with_config(three));
        let b = compute_workflow_content_hash(&sample_ir_with_config(five));
        assert_ne!(a, b, "graphs differing only in config must not collide");
    }

    #[test]
    fn hash_ignores_config_key_insertion_order() {
        let mut a_map = HashMap::new();
        a_map.insert("count".into(), serde_json::json!(3));
        a_map.insert("members".into(), serde_json::json!(["x", "y"]));

        let mut b_map = HashMap::new();
        b_map.insert("members".into(), serde_json::json!(["x", "y"]));
        b_map.insert("count".into(), serde_json::json!(3));

        let a = compute_workflow_content_hash(&sample_ir_with_config(a_map));
        let b = compute_workflow_content_hash(&sample_ir_with_config(b_map));
        assert_eq!(a, b, "key order in config must not affect the hash");
    }

    #[test]
    fn hash_ignores_nested_object_key_order_in_config() {
        let nested_a = serde_json::json!({"outer": {"z": 1, "a": 2}});
        let nested_b = serde_json::json!({"outer": {"a": 2, "z": 1}});

        let mut a_map = HashMap::new();
        a_map.insert("nested".into(), nested_a);
        let mut b_map = HashMap::new();
        b_map.insert("nested".into(), nested_b);

        let a = compute_workflow_content_hash(&sample_ir_with_config(a_map));
        let b = compute_workflow_content_hash(&sample_ir_with_config(b_map));
        assert_eq!(a, b, "nested object key order must not affect the hash");
    }

    #[test]
    fn hash_differs_between_empty_and_nonempty_config() {
        let empty = compute_workflow_content_hash(&sample_ir());
        let mut one = HashMap::new();
        one.insert("count".into(), serde_json::json!(3));
        let with_config = compute_workflow_content_hash(&sample_ir_with_config(one));
        assert_ne!(empty, with_config);
    }

    /// Reference implementation of the original scheme (before config mixing)
    /// to pin backward compatibility: graphs whose nodes carry no config must
    /// hash exactly as they did before the config was added to the mix.
    fn legacy_hash_without_config(ir: &WorkflowIR) -> u64 {
        const OFFSET: u64 = 0xcbf29ce484222325;
        const PRIME: u64 = 0x100000001b3;
        let mut h = OFFSET;
        let mix = |h: &mut u64, byte: u8| {
            *h ^= byte as u64;
            *h = h.wrapping_mul(PRIME);
        };
        let mix_u64 = |h: &mut u64, v: u64| {
            for b in v.to_le_bytes() {
                mix(h, b);
            }
        };
        let mix_str = |h: &mut u64, s: &str| {
            mix_u64(h, s.len() as u64);
            for b in s.as_bytes() {
                mix(h, *b);
            }
        };

        mix_u64(&mut h, ir.plan_id.as_u128() as u64);
        mix_u64(&mut h, (ir.plan_id.as_u128() >> 64) as u64);
        mix_u64(&mut h, ir.metadata.policy_version);
        mix_u64(&mut h, ir.nodes.len() as u64);

        let mut nodes: Vec<_> = ir.nodes.iter().collect();
        nodes.sort_by_key(|n| n.id);
        for n in nodes {
            assert!(
                n.config.is_empty(),
                "reference impl handles config-free IRs"
            );
            mix_u64(&mut h, n.id.as_u128() as u64);
            mix_u64(&mut h, (n.id.as_u128() >> 64) as u64);
            mix_str(&mut h, &format!("{:?}", n.kind));
            mix_str(&mut h, &format!("{:?}", n.strategy));
            if let Some(ref m) = n.model {
                mix_str(&mut h, m);
            } else {
                mix(&mut h, 0);
            }
        }

        let mut edges: Vec<_> = ir.edges.iter().collect();
        edges.sort_by_key(|e| (e.from, e.to));
        for e in edges {
            mix_u64(&mut h, e.from.as_u128() as u64);
            mix_u64(&mut h, e.to.as_u128() as u64);
            if let Some(ref c) = e.condition {
                mix_str(&mut h, c);
            } else {
                mix(&mut h, 0);
            }
        }
        h
    }

    #[test]
    fn config_free_graphs_keep_legacy_hash_values() {
        let mut ir = sample_ir();
        // Multi-node graph with edges, all configs empty.
        let b = uuid::Uuid::new_v4();
        ir.nodes.push(IRNode {
            id: b,
            kind: IRNodeKind::Judge,
            strategy: StrategyKind::Single,
            model: Some("m2".into()),
            config: HashMap::new(),
        });
        ir.edges.push(IREdge {
            from: ir.nodes[0].id,
            to: b,
            condition: None,
        });

        assert_eq!(
            compute_workflow_content_hash(&ir),
            legacy_hash_without_config(&ir),
            "config-free graphs must not change hash values (snapshot corpus compat)"
        );
    }
}
