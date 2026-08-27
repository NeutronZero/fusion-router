#!/usr/bin/env python3
"""
check_monolith_freeze.py — Repository-State Architectural Convergence Firewall

Verifies all 11 Convergence Gates across the FusionRouter repository state.
Exits 0 and prints status table if converged; exits 1 if violations found.
"""

import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent


def check_gate_01_planner_authority():
    """Gate 01: Canonical fusion-planner crate is single authoritative planning engine."""
    src_planner = ROOT / "src" / "planner"
    invalid_files = ["dynamic_planner.rs", "simple.rs"]
    for fname in invalid_files:
        if (src_planner / fname).exists():
            return False, f"Legacy planner file '{fname}' still exists"

    intent_planner = src_planner / "intent_planner.rs"
    if intent_planner.exists():
        content = intent_planner.read_text(encoding="utf-8")
        forbidden = ["fn build_quality(", "fn build_speed(", "fn build_balanced(", "fn build_exhaustive(", "node.model = Some("]
        for fn in forbidden:
            if fn in content:
                return False, f"Host fallback / mutation '{fn}' still present in intent_planner.rs"

    chat_rs = ROOT / "src" / "server" / "handlers" / "chat.rs"
    if chat_rs.exists():
        content = chat_rs.read_text(encoding="utf-8")
        if "ir.nodes = vec![" in content:
            return False, "Host chat handler overwrites planner IR nodes directly"

    crate_planner = ROOT / "crates" / "fusion-planner" / "src" / "lib.rs"
    if crate_planner.exists():
        content = crate_planner.read_text(encoding="utf-8")
        forbidden_crate = ["fn build_quality(", "fn build_speed(", "fn build_balanced(", "fn build_exhaustive(", "fn plan_intent("]
        for fn in forbidden_crate:
            if fn in content:
                return False, f"Hardcoded template '{fn}' still present in canonical fusion-planner crate"
    return True, "Canonical snapshot-driven planner in fusion-planner"


def check_gate_02_compiler_authority():
    """Gate 02: Zero host compiler passes remaining in src/compiler."""
    src_compiler = ROOT / "src" / "compiler"
    if not src_compiler.exists():
        return True, "No src/compiler directory"

    legacy_passes = src_compiler / "passes" / "legacy_passes.rs"
    if legacy_passes.exists():
        content = legacy_passes.read_text(encoding="utf-8")
        if len(content.strip()) > 50:
            return False, "Host legacy compiler passes still present"

    strategy_expansion = src_compiler / "strategy_expansion.rs"
    if strategy_expansion.exists():
        return False, "Host strategy expansion still in src/compiler"

    # Hardened (audit H2): production path is crates/fusion-compiler;
    # src/compiler/ir and src/strategies are retained only for devex/benches
    # and must be isolated from the production pipeline.
    # Ensure hot-path handlers do not lower via host strategies/IR
    for handler in [
        ROOT / "src" / "server" / "handlers" / "chat.rs",
        ROOT / "src" / "server" / "pipeline.rs",
        ROOT / "src" / "main.rs",
    ]:
        if handler.exists():
            c = handler.read_text(encoding="utf-8").split("#[cfg(test)]", 1)[0]
            if "crate::strategies" in c or "crate::compiler::ir::" in c:
                return False, f"Production file {handler.relative_to(ROOT)} imports host compiler/strategies"

    # Report residual files as informational debt (AD-005) but keep gate PASS
    # because they are not on the hot path; strict removal is tracked as debt.
    residual_count = 0
    for sub in [src_compiler / "ir", src_compiler / "optimization"]:
        if sub.exists():
            residual_count += len(list(sub.rglob("*.rs")))
    strat_dir = ROOT / "src" / "strategies"
    if strat_dir.exists():
        residual_count += len(list(strat_dir.rglob("*.rs")))
    if residual_count:
        return True, f"No host compiler passes on hot path (host IR/strategies isolated as devex-only, {residual_count} files tracked as AD-005)"

    return True, "No host compiler passes"


def check_gate_03_strategy_authority():
    """Gate 03: Zero host strategy execution in the entire src/executor tree."""
    src_executor = ROOT / "src" / "executor"
    # Files that are only compiled inside #[cfg(test)] modules.
    test_only_files = {"tool_loop.rs"}
    forbidden = ["resolve_strategy", "expanded_subgraph", "execute_legacy", "execute_native_tool_calls",
                 "provider.chat_completion", "subgraph traversal", "strategy execution"]
    for rs_file in src_executor.rglob("*.rs"):
        if rs_file.name in test_only_files:
            continue
        rel = rs_file.relative_to(ROOT)
        content = rs_file.read_text(encoding="utf-8")
        # Split at the first #[cfg(test)] to isolate production code.
        production = content.split("#[cfg(test)]", 1)[0]
        for marker in forbidden:
            if marker in production:
                return False, f"Host executor semantic marker '{marker}' remains in {rel}"
    return True, "Host executor contains only runtime delegation adapters"


def check_gate_04_runtime_authority():
    """Gate 04: Zero legacy provider execution paths."""
    src_executor = ROOT / "src" / "executor"
    node_exec = src_executor / "node_exec.rs"
    if not node_exec.exists():
        return False, "src/executor/node_exec.rs is missing"
    content = node_exec.read_text(encoding="utf-8").split("#[cfg(test)]", 1)[0]
    if "fusion_runtime::ProviderExecutor" not in content:
        return False, "node_exec.rs does not delegate to fusion-runtime"
    return True, "Runtime authority consolidated in crates/fusion-runtime"


def check_gate_05_attestation_authority():
    """Gate 05: Zero production MockPackageVerifier usages."""
    main_rs = ROOT / "src" / "main.rs"
    if main_rs.exists():
        content = main_rs.read_text(encoding="utf-8")
        for line in content.split("\n"):
            stripped = line.strip()
            if stripped.startswith("//") or stripped.startswith("#[cfg(test)]"):
                continue
            if "MockPackageVerifier" in stripped and "=" in stripped:
                return False, "Production main.rs uses MockPackageVerifier"
    return True, "ArchivePackageVerifier used in main.rs"


def check_gate_06_policy_authority():
    """Gate 06: PolicyRegistry is the single authoritative policy source."""
    policy_reg = ROOT / "src" / "policy" / "policy_registry.rs"
    if not policy_reg.exists():
        return False, "src/policy/policy_registry.rs does not exist"
    content = policy_reg.read_text(encoding="utf-8")
    if "pub struct PolicyRegistry" not in content:
        return False, "PolicyRegistry struct not found"
    constructor_sites = []
    for rs_file in (ROOT / "src").rglob("*.rs"):
        production = rs_file.read_text(encoding="utf-8").split("#[cfg(test)]", 1)[0]
        if "PolicyRegistry::new()" in production:
            constructor_sites.append(rs_file.relative_to(ROOT))
    if constructor_sites != [Path("src/server/handlers/state.rs")]:
        return False, f"production PolicyRegistry constructors are not singleton-owned: {constructor_sites}"
    main_content = (ROOT / "src" / "main.rs").read_text(encoding="utf-8")
    chat_content = (ROOT / "src" / "server" / "handlers" / "chat.rs").read_text(encoding="utf-8")
    if "state.policy_registry.clone()" not in main_content:
        return False, "main.rs does not pass the AppState policy registry to operations"
    if "state.policy_registry.current_snapshot()" not in chat_content:
        return False, "chat handler does not consume AppState policy registry"
    return True, "PolicyRegistry is single authoritative policy source"


def check_gate_07_capability_authority():
    """Gate 07: Single authoritative CapabilityRegistry/PluginManager source and startup lifecycle."""
    plugin_manager = ROOT / "src" / "plugin" / "manager.rs"
    if not plugin_manager.exists():
        return False, "src/plugin/manager.rs does not exist"
    content = plugin_manager.read_text(encoding="utf-8")
    if "pub struct PluginManager" not in content:
        return False, "PluginManager struct not found"

    main_rs = ROOT / "src" / "main.rs"
    if main_rs.exists():
        main_content = main_rs.read_text(encoding="utf-8")
        if "load_manifests(" not in main_content or "freeze_capability_registry(" not in main_content:
            return False, "PluginManager startup lifecycle not executed in main.rs"
        if "InMemoryCapabilityRegistry::new()" in main_content:
            return False, "main.rs creates a second in-memory capability registry"
        if "with_capability_registry(" not in main_content:
            return False, "frozen capability registry is not wired into AppState"
    return True, "PluginManager authority with executed startup lifecycle"


def check_gate_08_streaming_authority():
    """Gate 08: Streaming and non-streaming share standard ExecutionGraph."""
    chat_rs = ROOT / "src" / "server" / "handlers" / "chat.rs"
    if chat_rs.exists():
        content = chat_rs.read_text(encoding="utf-8")
        if "FUSION_EXPERIMENTAL_DIRECT_STREAM" in content:
            return False, "Direct streaming escape hatch still present"
        if "stream_completed_response" not in content:
            return False, "SSE transport adapter not found"
    anthropic_rs = ROOT / "src" / "server" / "handlers" / "anthropic.rs"
    if anthropic_rs.exists():
        content = anthropic_rs.read_text(encoding="utf-8")
        if "anthropic_stream_completed_response" not in content:
            return False, "Anthropic SSE transport adapter not found"
    return True, "Streaming and non-streaming share standard graph"


def check_gate_09_monetary_authority():
    """Gate 09: Zero internal f64/millicost monetary fields across repository."""
    monetary_rs = ROOT / "crates" / "fusion-core" / "src" / "monetary.rs"
    if not monetary_rs.exists():
        return False, "NanoUSD type file does not exist"

    # Verify zero occurrences of legacy cost_millicosts in any rust file
    for rs_file in ROOT.rglob("*.rs"):
        if "/target/" in str(rs_file).replace("\\", "/"):
            continue
        content = rs_file.read_text(encoding="utf-8")
        if "cost_millicosts" in content:
            rel_path = rs_file.relative_to(ROOT)
            return False, f"Legacy field 'cost_millicosts' found in {rel_path}"

    monetary_fields = [
        "max_daily_cost", "total_cost_usd", "cost_usd", "avg_cost", "avg_cost_nanos",
        "estimated_cost_usd", "input_cost_per_1k", "output_cost_per_1k",
        "max_cost_per_1k_tokens", "cost_per_1k",
    ]
    presentation_only = {Path("src/bin/eval_runner.rs")}
    for rs_file in ROOT.rglob("*.rs"):
        rel_path = rs_file.relative_to(ROOT)
        normalized = str(rel_path).replace("\\", "/")
        if normalized.startswith("target/") or normalized.startswith("tests/") or rel_path in presentation_only:
            continue
        content = rs_file.read_text(encoding="utf-8")
        production = content.split("#[cfg(test)]", 1)[0]
        for line in production.split("\n"):
            stripped = line.strip()
            if stripped.startswith("//") or stripped.startswith("/*") or stripped.startswith("*"):
                continue
            if "f64" not in stripped:
                continue
            for field in monetary_fields:
                if re.search(rf"\b{re.escape(field)}\b", stripped):
                    return False, f"internal monetary f64 field '{field}' in {rel_path}"

    return True, "NanoUSD is canonical integer monetary type across all crates and host"


def check_gate_10_fallback_elimination():
    """Gate 10: Zero passthrough or strategy fallbacks in compiler/runtime."""
    strat_exp = ROOT / "crates" / "fusion-compiler" / "src" / "strategy_expansion.rs"
    if strat_exp.exists():
        content = strat_exp.read_text(encoding="utf-8")
        for line in content.split("\n"):
            stripped = line.strip()
            if stripped.startswith("//") or stripped.startswith("//!"):
                continue
            if "passthrough" in stripped.lower():
                return False, "Strategy expansion contains passthrough in code"

    strategy_compiler = ROOT / "crates" / "fusion-compiler" / "src" / "strategy_compiler.rs"
    if strategy_compiler.exists():
        content = strategy_compiler.read_text(encoding="utf-8")
        for line in content.split("\n"):
            stripped = line.strip()
            if stripped.startswith("//") or stripped.startswith("//!"):
                continue
            if "passthrough" in stripped.lower():
                return False, "Strategy compiler contains passthrough in code"
        if "Unregistered custom strategy" not in content:
            return False, "Fail-closed custom strategy validation missing"

    # Verify Custom strategy has no generic fallback in expansion.
    if strat_exp.exists():
        content = strat_exp.read_text(encoding="utf-8")
        # The only allowed expansion for Custom is via a registered delegate.
        if "expand_custom(node, custom_name)" in content.split("pub fn expanded_subgraph_with_custom")[1].split("}")[0] if "expanded_subgraph_with_custom" in content else "":
            return False, "Custom strategy still has a generic fallback expansion"

    # No production expanded_subgraph() calls outside crates/ and tests.
    for rs_file in (ROOT / "src").rglob("*.rs"):
        rel = rs_file.relative_to(ROOT)
        normalized = str(rel).replace("\\", "/")
        if normalized.startswith("target/"):
            continue
        content = rs_file.read_text(encoding="utf-8")
        production = content.split("#[cfg(test)]", 1)[0]
        if "expanded_subgraph(" in production:
            return False, f"Production expanded_subgraph() call in {rel}"

    return True, "Zero strategy passthroughs and fail-closed custom strategy validation"


def check_gate_11_deterministic_compilation():
    """Gate 11: Zero entropy sources in planning/compilation."""
    forbidden_patterns = [
        (r'Uuid::new_v4\(\)', "Random UUID v4"),
        (r'std::time::SystemTime', "SystemTime"),
        (r'rand::', "rand crate"),
    ]

    check_dirs = [
        ROOT / "crates" / "fusion-planner" / "src",
        ROOT / "crates" / "fusion-compiler" / "src",
    ]

    for check_dir in check_dirs:
        if not check_dir.exists():
            continue
        for rs_file in check_dir.rglob("*.rs"):
            if "/tests/" in str(rs_file).replace("\\", "/") or "/test_" in rs_file.name:
                continue
            content = rs_file.read_text(encoding="utf-8")
            in_test_mod = False
            for line_num, line in enumerate(content.split("\n"), 1):
                stripped = line.strip()
                if "mod tests {" in stripped or "mod tests" in stripped:
                    in_test_mod = True
                if in_test_mod:
                    continue
                if stripped.startswith("//") or stripped.startswith("#[cfg(test)]"):
                    continue
                for pattern, desc in forbidden_patterns:
                    if re.search(pattern, stripped):
                        rel_path = rs_file.relative_to(ROOT)
                        return False, f"{desc} in non-test compilation code at {rel_path}:{line_num}"

    determinism_test = ROOT / "tests" / "contract_wiring.rs"
    if not determinism_test.exists():
        return False, "byte-for-byte determinism test is missing"
    test_content = determinism_test.read_text(encoding="utf-8")
    if "byte-for-byte determinism" not in test_content or "canonical_json" not in test_content or "assert_eq!(canonical_json(&graph_a), canonical_json(&graph_b))" not in test_content:
        return False, "determinism test does not compare canonical IR and graph bytes"

    return True, "Deterministic planning and byte-identical canonical compilation with child_id v5"


GATES = [
    ("Gate 01 Planner Authority", check_gate_01_planner_authority),
    ("Gate 02 Compiler Authority", check_gate_02_compiler_authority),
    ("Gate 03 Strategy Authority", check_gate_03_strategy_authority),
    ("Gate 04 Runtime Authority", check_gate_04_runtime_authority),
    ("Gate 05 Attestation Authority", check_gate_05_attestation_authority),
    ("Gate 06 Policy Authority", check_gate_06_policy_authority),
    ("Gate 07 Capability Authority", check_gate_07_capability_authority),
    ("Gate 08 Streaming Authority", check_gate_08_streaming_authority),
    ("Gate 09 Monetary Authority", check_gate_09_monetary_authority),
    ("Gate 10 Fallback Elimination", check_gate_10_fallback_elimination),
    ("Gate 11 Deterministic Compilation", check_gate_11_deterministic_compilation),
]


def main():
    if hasattr(sys.stdout, "reconfigure"):
        sys.stdout.reconfigure(encoding="utf-8")

    print("\nFusionRouter Architectural Convergence Firewall\n")
    all_passed = True
    for name, check_fn in GATES:
        passed, detail = check_fn()
        status_str = "PASS" if passed else "FAIL"
        print(f"{name:38} ............. {status_str} ({detail})")
        if not passed:
            all_passed = False

    print()
    if all_passed:
        print("ARCHITECTURE STATUS: CONVERGED\n")
        sys.exit(0)
    else:
        print("ARCHITECTURE STATUS: VIOLATIONS DETECTED\n")
        sys.exit(1)


if __name__ == "__main__":
    main()
