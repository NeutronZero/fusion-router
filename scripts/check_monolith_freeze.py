#!/usr/bin/env python3
"""
check_monolith_freeze.py — Monolith Freeze Policy Enforcer

Enforces the freeze policy on `src/compiler/` and `src/planner/` during the
Phase 3 porting period into the 3-tier `crates/` workspace.

Exits with code 1 if any modified or untracked files are detected under
frozen directories.
"""

import sys
import subprocess
from pathlib import Path

FROZEN_PATHS = ["src/compiler", "src/planner", "src/resource"]

def get_git_diff_files():
    try:
        # Check staged and unstaged changes against HEAD
        output = subprocess.check_output(
            ["git", "diff", "--name-only", "HEAD"], text=True
        )
        files = [f.strip() for f in output.splitlines() if f.strip()]

        # Also check untracked files
        untracked = subprocess.check_output(
            ["git", "ls-files", "--others", "--exclude-standard"], text=True
        )
        files.extend([f.strip() for f in untracked.splitlines() if f.strip()])
        return files
    except Exception as e:
        print(f"[check_monolith_freeze] Error running git command: {e}")
        sys.exit(1)

def main():
    if hasattr(sys.stdout, "reconfigure"):
        sys.stdout.reconfigure(encoding="utf-8")

    changed_files = get_git_diff_files()

    violations = []
    for rel_path in changed_files:
        normalized = rel_path.replace("\\", "/")
        for frozen in FROZEN_PATHS:
            if normalized.startswith(frozen):
                violations.append(normalized)

    if violations:
        print("\n" + "=" * 80)
        print("[ERROR] MONOLITH FREEZE POLICY VIOLATION DETECTED")
        print("=" * 80)
        print("The following files in frozen monolith paths were modified:")
        for v in violations:
            print(f"  - {v}")
        print("\nReason: src/compiler/, src/planner/, and src/resource/ are frozen during the Phase 3 port.")
        print("All compiler/planner/resource changes must be ported to crates/ instead of modifying the monolith.")
        print("=" * 80 + "\n")
        sys.exit(1)
    else:
        print("[OK] Monolith freeze check passed (zero modifications in frozen paths).")
        sys.exit(0)

if __name__ == "__main__":
    main()
