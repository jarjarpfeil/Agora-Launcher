# Baseline Verification Record

> **Phase 0 baseline captured 2026-07-16** on branch `architecture/core-services-cli-parity`.
> Commit `2fe5e3a` (identical to `origin/master`). No source changes in this phase — only documentation additions.
> Pre-existing failures are recorded below and are NOT caused by this packet.
>
> **Compiler test side effects:** The Python compiler tests (`pytest compiler/test_compile*.py`) mutate repository fixtures when run in-place. During baseline capture they appended 9 `compile` entries to `registry/governance/audit_log.json` and created `registry/governance/audit_log_archive.20260717.json` (200 KB dummy archive from audit-log rotation tests). These generated artifacts were removed after capture; they do not reflect user or prior work.

## Command Results

| # | Command | Result | Details |
|---|---|---|---|
| 1 | `cargo fmt --all -- --check` | **FAILED** | Pre-existing formatting differences in `crates/agora-core/src/registry_sync.rs` (2 hunks). Not caused by this packet — no source files were modified. |
| 2 | `cargo clippy --workspace --all-targets --all-features -- -D warnings` | **FAILED** | ~150 pre-existing clippy lint violations in `agora-core` (lib + test). Categories: `collapsible_if`, `manual_inspect`, `unnecessary_map_or`, `needless_borrow`, `ptr_arg`, `manual_flatten`, `derivable_impls`, `too_many_arguments`, `unnecessary_sort_by`, `let_and_return`, `useless_conversion`, `for_kv_map`, etc. None from this packet. |
| 3 | `cargo test -p agora-core --lib` | **PASSED** | 709/709 tests passed in 1.82s. |
| 4 | `cargo test -p agora-core --test launch_planner_integration` | **PASSED** | 49/49 integration tests passed in 0.17s. |
| 5 | `cargo test -p agora-cli` | **PASSED** | 6/6 CLI unit tests passed. |
| 6 | `cargo build -p agora-cli` | **PASSED** | CLI binary `agora.exe` builds successfully. |
| 7 | `cargo check -p agora-desktop` | **PASSED** | Desktop crate compiles without errors. |
| 8 | Python compiler tests | **1 FAILED, 82 PASSED** | `test_schema_version` expects version 5 but compiler produces version 6. Pre-existing. ⚠️ The compiler test suite mutates repo fixtures when run in-place (audit_log entries + archive). |
| 9 | Script tests (`scripts/test_scripts.py`) | **PASSED** | 56/56 tests passed. |
| 10 | Frontend TypeScript check (`tsc --noEmit`) | **PASSED** | Exit code 0. |
| 11 | Frontend Vite build | **PASSED** | Built successfully. Warning: `index.js` chunk is 806 KB (exceeds 500 KB advisory limit). No errors. |

## CLI Help Output

```
Agora Minecraft Launcher CLI

Usage: agora.exe [OPTIONS] <COMMAND>

Commands:
  list-instances
  get-instance
  mods
  health
  registry
  snapshots
  import
  launch
  auth
  serve
  sync
  runtime
  help

Options:
      --data-dir <DATA_DIR>  Path to Agora data directory
      --json                 JSON output
  -h, --help                 Print help
```

## CLI Data-Directory Observations

- No `data-dir` subcommand exists — the path is supplied via the `--data-dir <DATA_DIR>` global option.
- Default data directory is OS-standard (e.g., `%APPDATA%/Agora` on Windows).
- `registry status --json` returns expected empty-state JSON:

```json
{
  "has_cached_db": false,
  "cached_tag": null,
  "cached_schema_version": null,
  "latest_tag": null,
  "update_available": false,
  "checked": false,
  "message": "No registry database found. Click Check for Updates."
}
```

## Relevant BACKLOG.md State — With MASTER_SPEC Reconciliation

### Where BACKLOG and MASTER_SPEC agree

- **§19.2 (MASTER_SPEC.md):** Migration status from desktop to agora-core is tracked. Thick modules remain in desktop: `crash_investigator.rs`, `mod_install.rs`, `instances.rs`, `mojang.rs`, `mcp.rs`, `version_cache.rs`, `governance.rs`.
- **§19.10:** CLI capabilities include `instances`, `mods`, `health`, `registry`, `snapshots`, `import`, `launch`, `auth`, `serve` (stub).
- **Deferred items:** Code signing, auto-update, Dev Mode sandboxed builds, telemetry aggregation, i18n — all still deferred.

### Where BACKLOG and MASTER_SPEC conflict

| Claim | BACKLOG.md | MASTER_SPEC.md §19.13.3 | Resolution |
|---|---|---|---|
| C3 InstallPipeline completeness | Marked `[x]` (implemented). "One canonical `InstallFlow` for all desktop install entry points; CLI mod install/remove reuse the same core transaction." | "C0 design documented; C1 core types present; C2 execution scaffold exists with cfg-gated commands and stub resolver. **Unsafe commands are NOT registered in production builds. Legacy install paths remain active.** Full implementation deferred to Release C2-C4." | **MASTER_SPEC wins.** The install pipeline has scaffold-level implementation (core types, cfg-gated commands, resolver stub). The safe commands exist but unsafe commands are not registered in production. BACKLOG's `[x]` is optimistic — the feature is not fully operational. |

All other BACKLOG items in the "Release B/C/D" sections are marked `[x]` and correspond to code that exists and passes tests. The discrepancy above is the only significant status disagreement affecting this baseline.

## Pre-Existing Failures (unrelated to this packet)

1. `cargo fmt --check` fails on 2 formatting hunks in `registry_sync.rs`.
2. `cargo clippy -D warnings` fails on ~150 clippy lints across `agora-core`. All are style lints (no correctness or security issues).
3. Python `test_schema_version` expects version 5; compiler produces version 6 — schema version bump not reflected in test expectation.
4. Frontend Vite build warns about large chunk size (806 KB index.js) — advisory only, not an error.
