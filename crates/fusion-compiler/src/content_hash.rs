//! Deterministic FNV-1a content hash over planning IR topology and binding.
//! Identical IR yields identical hash across runs (no entropy sources).

use fusion_types::WorkflowIR;

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
}
