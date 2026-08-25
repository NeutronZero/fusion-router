# ADR-041: TOCTOU-Safe Shell Path Policy

- **Status:** Accepted (staging implemented 2026-08-25; openat2 hard mode deferred pending Linux CI validation — see Implementation Status)
- **Date:** 2026-08-25
- **Applies to:** shell tool (`src/tools/shell_tool.rs`), path containment (`src/security/paths.rs`), tool configuration (`src/config/types.rs`)
- **Charter:** AF-003 Law 10 (path containment), ADR-035 (fail-closed deployment), Debt Register AD-018

## Context

The shell argument policy validates every path-bearing argument with
`canonicalize_within` (`src/security/paths.rs`) before spawning the allowlisted
binary. Validation resolves symlinks at *check* time, but the child process
re-resolves the same path string at *open* time. A local attacker with write
access anywhere on the resolution path — including inside an allowed read
directory — can swap a symlink between validation and exec:

```
t0  validate("root/link.txt")  -> canonicalizes to root/data.txt   OK
t1  attacker: mv link.txt link.txt.bak; ln -s /etc/shadow link.txt
t2  child:   open("root/link.txt") -> reads /etc/shadow            ESCAPED
```

The 2026-08-26 review confirmed this window is real (AD-018). Flag-carried
paths (`-f`, `--file=`) received the same check in the remediation pass, but
the check-vs-use race applies identically to them. The vulnerability class is
not fixable by validating harder or more often: any pure re-validation scheme
is still TOCTOU. The window must be closed *by construction* — either the host
performs the read itself (so no unvalidated open ever happens), or the kernel
pins the resolution (so no re-resolution happens).

## Decision

Path-bearing arguments on file-reading commands are rewritten to reference
host-verified content. Two mechanisms, selected by config:

### 1. Snapshot staging (default; all platforms)

For each path argument:

1. **Validate** — `canonicalize_within(root, candidate)` as today. Reject
   non-regular files (dirs, fifos, devices).
2. **Host-open** — immediately after validation, the host opens the *canonical*
   path (no symlinks remain in it) with `O_RDONLY`. Because the canonical path
   contains no link components, the attacker's swap trick cannot redirect this
   open; worst case the file was renamed, which surfaces as `ENOENT` → fail
   closed.
3. **Stage** — copy bytes into a per-execution staging directory
   (`<staging_root>/<execution-id>/<arg-hash><ext>`, default under
   `std::env::temp_dir()`, permissions `0700`). Copies are capped by
   `tools.max_staged_input_bytes` (default 64 MiB); larger files fail closed.
4. **Rewrite** — replace the argument value with the staged path. The child
   never receives the user-supplied path string.
5. **Reap** — the staging directory is deleted when the tool result is
   produced (drop guard; also swept at startup for stale `>1h` dirs).

Correctness argument: after step 4 there is no attacker-influenced path left
in the child's argv. The only files readable through the command are bytes the
host itself read from validated locations. The check-to-use window moves
entirely into the host, where open follows validate on the canonical path with
no intervening namespace traversal.

Cost: one bounded file copy per path argument. Acceptable for the text-tool
workload this command set serves; oversized inputs are refused rather than
streamed.

### 2. Kernel-pinned opens (`security.shell.openat2: true`; Linux 5.6+)

On supported kernels the host opens each validated candidate through
`openat2(..., RESOLVE_BENEATH | RESOLVE_NO_SYMLINKS | RESOLVE_NO_MAGICLINKS)`
relative to an `O_PATH` descriptor of its trust root, clears `O_CLOEXEC`, and
rewrites the argument to `/dev/fd/N`. The kernel guarantees the binding
between the validated directory and the open regardless of later namespace
mutation; nothing is copied.

- Implemented via the `rustix` crate behind `#[cfg(target_os = "linux")]`.
- Feature-probed at startup; if `openat2` is unavailable the limiter falls
  back to staging **with a loud warning**, never silently.
- `/dev/fd/N` works transparently for the entire current command set
  (`cat`, `sed -f`, `grep --file=`, …) because they accept ordinary paths.

### 3. Windows

No child-process fd/handle passing exists for argv-driven tools without
`STARTUPINFOEX` handle-list plumbing that the target binaries do not consume.
Staging is therefore the *only* mechanism on Windows and is always enabled
there. The residual risk on Windows equals the staging guarantee (closed by
construction), not the direct-path guarantee.

### Configuration

```yaml
tools:
  path_mode: stage        # stage | direct   (direct = legacy, logged warning)
  max_staged_input_bytes: 67108864
  openat2: auto           # auto | on | off  (Linux only)
```

`path_mode: direct` remains for operators who need zero-copy semantics on
trusted single-tenant hosts; release-profile validation warns on it unless
`unsafe_dev` is also set (ADR-035 posture).

## Alternatives Considered

- **Re-validate after spawn / watch with inotify:** still racy; the swap can
  land inside any observation gap. Rejected as primary mechanism.
- **Run children inside a full sandbox (bubblewrap/job objects):** correct and
  broader, but a much larger scope touching every connector, not just the
  shell tool. Recorded as future work; does not block this ADR.
- **Do nothing, document the precondition:** rejected — the precondition
  (local write access near an allowed root) is realistic for shared CI runners,
  which are exactly where release gates run.

## Consequences

- TOCTOU on shell path arguments is closed by construction on every supported
  platform: staging (default) or kernel resolution pinning (Linux opt-in).
- Text-tool invocations gain one bounded copy (≤ `max_staged_input_bytes`) per
  path argument; `direct` mode preserves today's latency profile with a
  documented trust assumption.
- New dependency: `rustix` (Linux target only, `cfg`-gated).
- `canonicalize_within` keeps its role as the first-pass validator; its
  contract docstring gains the note that callers must not hand the *original*
  (non-canonical) path to a downstream consumer.
- Tests (all required for closure):
  1. Race harness — a thread flips a symlink across the validation boundary N
     times while the exec loop runs; asserts no read ever observes content
     from outside the roots.
  2. Staged-path rewrite — spawned command sees the staged copy, original path
     absent from argv.
  3. Oversize input refused; staging dir cleaned on completion and startup sweep.
  4. `openat2` probe fallback logs a warning and stages (Linux CI matrix).
  5. `path_mode: direct` emits the release-profile validation warning.

## Debt Register Link

Closes AD-018 upon merge with tests 1–3 green; AD-018 narrows to "expand
per-command arg schemas" only if additional commands join the file-reading set.

## Implementation Status (2026-08-25)

- **Staging is implemented on all platforms** (src/tools/shell_tool.rs): validate ->
  host-open canonical -> handle/path identity check -> capped copy -> argv rewrite ->
  drop-guard cleanup + startup sweep for stale dirs (>1h). Config:
  	ools.shell_path_mode (stage default / direct warned in release profile),
  	ools.max_staged_input_bytes (64 MiB default).
  The identity check opens FIRST and compares the opened handle against a fresh
  stat of the validated canonical path (dev+ino on Unix; creation_time+len+mtime
  on Windows), so a swap between validation and open is detected rather than read.
- **openat2 hard mode is NOT implemented yet.** Staging already closes the
  TOCTOU window by construction on every platform, so hard mode is a zero-copy
  optimization, not a security dependency. It lands after Linux CI can compile
  and race-test it (ustix cfg-gated). This deviation from the Decision section
  is deliberate: unverified FFI must not ship behind an OS gate this repo cannot
  exercise.
- Tests: staged rewrite + drop cleanup, direct-mode passthrough, oversize refusal,
  session-dir cleanup, and a symlink-swap race harness asserting no staged copy
 ever contains outside-root bytes (22 shell-tool tests green).