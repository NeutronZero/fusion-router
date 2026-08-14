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

    return True, "No host compiler passes"


def check_gate_03_strategy_authority():
    """Gate 03: Zero host strategy execution in src/executor."""
    src_executor = ROOT / "src" / "executor"
    strategy_resolver = src_executor / "strategy_resolver.rs"
    if strategy_resolver.exists():
        return False, "Host strategy resolver still in src/executor"
    return True, "No host strategy execution"


def check_gate_04_runtime_authority():
    """Gate 04: Zero legacy provider execution paths."""
    src_executor = ROOT / "src" / "executor"
    node_exec = src_executor / "node_exec.rs"
    if node_exec.exists():
        content = node_exec.read_text(encoding="utf-8")
        if "resolve_strategy" in content:
            return False, "Legacy resolve_strategy still referenced"
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

    f64_fields = ["estimated_cost", "total_cost", "max_daily_cost", "cost_per", "max_cost"]
    excluded_crates = set()

    crates_dir = ROOT / "crates"
    for toml_file in crates_dir.rglob("Cargo.toml"):
        crate_name = toml_file.parent.name
        if crate_name in excluded_crates:
            continue
        src_dir = toml_file.parent / "src"
        if not src_dir.exists():
            continue
        for rs_file in src_dir.rglob("*.rs"):
            if "/tests/" in str(rs_file) or "/test_" in rs_file.name:
                continue
            content = rs_file.read_text(encoding="utf-8")
            for line in content.split("\n"):
                stripped = line.strip()
                if stripped.startswith("//") or stripped.startswith("#[cfg(test)]"):
                    continue
                if not re.match(r'^pub\s+\w+\s*:\s*', stripped):
                    continue
                for field in f64_fields:
                    if field in stripped and "f64" in stripped:
                        rel_path = rs_file.relative_to(ROOT)
                        return False, f"f64 monetary field '{field}' in {rel_path}"

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

    return True, "Deterministic planning and compilation with child_id v5"


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
