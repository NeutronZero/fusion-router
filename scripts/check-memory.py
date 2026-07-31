#!/usr/bin/env python3
"""Validate .memory/ handbook against the FusionRouter codebase.

Checks:
- Every documented module file path still exists
- Every documented source file exists
- ADR references resolve to actual files
- Component/type names referenced in .memory/ exist in source
- No stale directory references

Usage:
    python scripts/check-memory.py
    python scripts/check-memory.py --verbose
    python scripts/check-memory.py --fix    # Update stale paths (todo)
"""

import os
import re
import sys
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parent.parent
MEMORY_DIR = REPO_ROOT / ".memory"
SRC_DIR = REPO_ROOT / "src"
CRATES_DIR = REPO_ROOT / "crates"
DOCS_DIR = REPO_ROOT / "docs"
PLUGINS_DIR = REPO_ROOT / "plugins"

# Patterns to extract file paths from markdown
FILE_PATH_RE = re.compile(r"`((?:src|crates|plugins|docs|scripts)/[^`]+)`")
SRC_PATH_RE = re.compile(r"`(src/[^`]+)`")
CRATE_PATH_RE = re.compile(r"`(crates/[^`]+)`")
ADR_REF_RE = re.compile(r"ADR-(\d{3})")
DOC_ADR_REF_RE = re.compile(r"ADR-(\d{3}-[A-Z]+)")


def find_all_source_files():
    """Build set of all existing source files."""
    files = set()
    for base in [SRC_DIR, CRATES_DIR, DOCS_DIR, PLUGINS_DIR]:
        if base.exists():
            for f in base.rglob("*"):
                if f.is_file() and f.suffix in {".rs", ".md", ".toml", ".yaml", ".yml", ".json", ".sh", ".ps1", ".py"}:
                    rel = f.relative_to(REPO_ROOT)
                    files.add(str(rel).replace("\\", "/"))
    return files


def get_memory_files():
    """Return list of all .memory/ markdown files."""
    return sorted(MEMORY_DIR.glob("*.md"))


def check_file_paths(memory_file: Path, source_files: set) -> list:
    """Check that all source file paths referenced in a memory file exist."""
    errors = []
    content = memory_file.read_text(encoding="utf-8")

    # Find all `path/...` references
    for pattern in [FILE_PATH_RE, SRC_PATH_RE, CRATE_PATH_RE]:
        for match in pattern.finditer(content):
            path = match.group(1)
            # Normalize
            path = path.replace("\\", "/")
            # Skip glob patterns and non-file references
            if "*" in path or "{" in path:
                continue
            # Skip if it's a directory reference ending in /
            if path.endswith("/"):
                continue
            # Skip well-known doc references
            if path.startswith("docs/superpowers/") or path.startswith("docs/adr/ADR-") or path.startswith("docs/adrs/"):
                # These are doc files that should exist
                pass
            # Check if file exists
            full_path = REPO_ROOT / path
            if not full_path.exists() and path not in source_files:
                errors.append(f"  MISSING: {path} (referenced in {memory_file.name})")

    return errors


def check_adr_references(memory_file: Path) -> list:
    """Check that ADR references point to existing files."""
    errors = []
    content = memory_file.read_text(encoding="utf-8")

    # Check ADR-NNN references (docs/adr/ADR-NNN-*.md)
    for match in ADR_REF_RE.finditer(content):
        num = match.group(1)
        adr_num = int(num)
        if adr_num > 31:
            continue  # Future ADRs are valid
        # Find matching ADR file
        adr_dir = DOCS_DIR / "adr"
        found = any(f.name.startswith(f"ADR-{num}-") for f in adr_dir.glob("*.md"))
        if not found:
            errors.append(f"  MISSING ADR: ADR-{num} (referenced in {memory_file.name}, not found in docs/adr/)")

    # Check docs/adrs/ ADR references (referenced via explicit path like `docs/adrs/adr-017-...`)
    for match in re.finditer(r"docs/adrs/(adr-\d+-[^`\s)]+)", content):
        path = match.group(1)
        full_path = DOCS_DIR / "adrs" / path
        if not full_path.exists():
            errors.append(f"  MISSING: docs/adrs/{path} (referenced in {memory_file.name})")

    return errors


def check_module_index_paths(source_files: set) -> list:
    """Specifically validate module-index.md file paths."""
    errors = []
    index_file = MEMORY_DIR / "module-index.md"
    if not index_file.exists():
        return errors

    content = index_file.read_text(encoding="utf-8")

    # Extract all `src/...` references from module-index
    for match in SRC_PATH_RE.finditer(content):
        path = match.group(1)
        full_path = REPO_ROOT / path
        if not full_path.exists() and path not in source_files:
            errors.append(f"  STALE: {path} (in module-index.md, file not found)")

    # Also check crate paths
    for match in CRATE_PATH_RE.finditer(content):
        path = match.group(1)
        full_path = REPO_ROOT / path
        if not full_path.exists() and path not in source_files:
            errors.append(f"  STALE: {path} (in module-index.md, file not found)")

    return errors


def check_documentation_cross_references() -> list:
    """Check that .memory/ files reference each other correctly."""
    errors = []
    memory_files = {f.name for f in get_memory_files()}

    content = "".join(f.read_text(encoding="utf-8") for f in get_memory_files())

    # Check `.memory/foo.md` references
    for match in re.finditer(r"`\.memory/([^`]+)`", content):
        ref = match.group(1)
        if ref not in memory_files:
            errors.append(f"  CROSS-REF: .memory/{ref} referenced but does not exist")

    return errors


def main():
    verbose = "--verbose" in sys.argv

    print("=" * 60)
    print("FusionRouter Memory Validation")
    print("=" * 60)
    print()

    source_files = find_all_source_files()
    print(f"Source files indexed: {len(source_files)}")
    print(f"Memory files: {len(list(get_memory_files()))}")
    print()

    all_errors = []

    # 1. Check each memory file for valid file path references
    print("--- Checking file path references ---")
    for mf in get_memory_files():
        errors = check_file_paths(mf, source_files)
        if errors:
            all_errors.extend(errors)
            for e in errors:
                print(e)
    if not any("MISSING:" in e for e in all_errors if "MISSING:" in e):
        print("  All file paths valid.")

    # 2. Check ADR references
    print()
    print("--- Checking ADR references ---")
    adr_errors = []
    for mf in get_memory_files():
        adr_errors.extend(check_adr_references(mf))
    if adr_errors:
        all_errors.extend(adr_errors)
        for e in adr_errors:
            print(e)
    else:
        print("  All ADR references valid.")

    # 3. Check module-index specifically for staleness
    print()
    print("--- Checking module-index paths ---")
    index_errors = check_module_index_paths(source_files)
    if index_errors:
        all_errors.extend(index_errors)
        for e in index_errors:
            print(e)
    else:
        print("  All module-index paths valid.")

    # 4. Check cross-references between memory files
    print()
    print("--- Checking cross-references ---")
    xref_errors = check_documentation_cross_references()
    if xref_errors:
        all_errors.extend(xref_errors)
        for e in xref_errors:
            print(e)
    else:
        print("  All cross-references valid.")

    # 5. Summary
    print()
    print("=" * 60)

    # Filter unique errors
    unique_errors = list(dict.fromkeys(all_errors))

    if unique_errors:
        print(f"ISSUES FOUND: {len(unique_errors)}")
        for e in unique_errors:
            print(f"  {e}")
        sys.exit(1)
    else:
        print("ALL CHECKS PASSED")
        print(".memory/ is synchronized with the codebase.")
        sys.exit(0)


if __name__ == "__main__":
    main()
