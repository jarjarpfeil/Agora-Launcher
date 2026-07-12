Reviewed the current tree at latest commit `8584fc76eb5dc99d62b66b4590916a13a15c6525`.

## Verdict: not complete — request changes

The repository itself still marks **C1–C4 and D1–D5 incomplete**.  The authoritative specification likewise says Release C contains only the C0 design, C1 types, and a disabled C2 scaffold, with legacy install paths still active.

The latest changes improve safety by removing the unfinished install commands from the production Tauri registry and replacing the prior fake-success frontend executor with a hard failure.   However, the complete upgrade plan is nowhere near its final release gate.

## Release-blocking findings

### P1 — The displayed batch-update feature cannot work

The Instances page detects updates, creates a snapshot, and then calls:

```ts
resolveInstallPlan(intent);
applyInstallPlan(plan);
```

But those commands are deliberately not registered with Tauri.

Therefore, Agora can show users that updates are available and let them confirm an update, but confirmation fails with an unknown-command error after creating an unnecessary snapshot.

Until C2 is operational, the Update buttons should be hidden or visibly disabled as unavailable. They should not expose a workflow that cannot complete.

---

### P1 — Existing snapshots became unreadable after the format change

`SnapshotFileEntry` now requires a `sha256` field:

```rust
struct SnapshotFileEntry {
    relative_path: String,
    size: u64,
    sha256: String,
}
```

Snapshots created before this commit do not contain that field. There is no `#[serde(default)]`, schema version, migration, or compatibility parser.

Consequences:

* Restoring an older snapshot fails while parsing its manifest.
* Listing snapshots silently omits every old snapshot whose manifest no longer deserializes.
* Existing users may believe their recovery snapshots disappeared.

Add a versioned manifest and make `sha256` optional for legacy entries, computing it from the archive content when needed.

---

### P1 — Snapshot hashes are recorded but never verified

Snapshot creation now hashes files, which is useful.

Restore extracts and copies those files without comparing the extracted content to the stored hashes.

A corrupted or modified snapshot can therefore be restored without detection. Verification should happen completely in the extraction directory before any live instance files are moved.

---

### P1 — Failed snapshot restore can destroy the state it was supposed to protect

After moving the current live state into `.agora_pre_restore`, restoration copies snapshot files into the instance. If one copy fails, `rollback_restore` attempts to rename the old directories back.

However:

* Partially restored destinations may already exist.
* The rollback does not remove those destinations first.
* Renaming onto an existing nonempty directory can fail.
* The rollback error is discarded by `let _ = rollback_restore(...)`.

This directly fails D3’s requirement that restore failure must not destroy the current state.

Restore needs a proper staged swap or journaled directory exchange, followed by verified cleanup only after success.

---

### P1 — “Last known good” is not connected to an actual snapshot

The approved architecture defines LKG as a pointer to a promoted snapshot, including a current snapshot ID.

The current launch implementation:

1. Creates a pre-launch snapshot.

2. Discards the returned snapshot and its ID.

3. On a successful launch, writes `lkg.json` without any snapshot ID.

The marker therefore cannot identify what should be restored. It is an audit note saying that a launch succeeded, not a usable LKG recovery record.

The snapshot ID must be retained through the launch session and written as `currentLkgSnapshotId` only after successful promotion.

---

### P1 — Launch classification does not follow the approved model

The current implementation sets:

```rust
crash_report_found: exit_code != 0,
log_crash_signature_matched: false,
was_user_cancelled: false,
```

This means:

* It does not actually check for a crash report.
* It does not inspect captured logs for a crash signature.
* A process killed by the user is classified as a crash rather than Cancelled.
* Delegated launches have no equivalent classification or promotion path at all.

The D2 design specifically requires meaningful classification signals and only promotes genuine successful launches.

---

### P1 — Drift detection compares incompatible path formats

Snapshot entries for mod files are stored as:

```text
mods/example.jar
```

because the snapshot walker prefixes paths with `mods/`.

Current live scanning records:

```text
example.jar
```

without the prefix. It also scans only `mods/`, while the snapshot index includes the manifest, configurations, resource packs, saves, and other tracked entries.

A perfectly unchanged instance will consequently report:

* Snapshot `mods/example.jar` as removed.
* Live `example.jar` as added.
* Every non-mod snapshot entry as removed.

D5 drift results are currently unusable.

---

### P1 — Lockfile import likely fails for every valid lockfile

The importer constructs an instance request with:

```rust
instance_id: String::new()
```

An empty ID remains empty after sanitization. `instance_dir()` joins it to the instances root directory, and the root is created automatically.

`create_instance` then rejects the request because that directory already exists.

The importer must generate a nonempty collision-resistant instance ID from the supplied name and reject it clearly if it cannot do so.

---

### P1 — Lockfile import does not reproduce an instance

Even after fixing the empty ID, import only creates an empty instance skeleton. It does not:

* Resolve any listed mod.
* Download artifacts.
* Verify artifact hashes.
* Restore enabled states.
* Reproduce settings.
* Report unavailable artifacts.
* Invoke the safe-install transaction.

Export is similarly incomplete. It omits enabled state, source URLs, instance settings, loader hashes, manifest hash, config hash, content hash, and signature information.

That is far below D5’s required reproducibility contract.

There is also a hash-design bug: validation hashes a structure that still contains its own `contentHash` field, making conventional self-consistent content hashes impossible.

---

### P1 — The backend launch concurrency check has a race

`launch_instance_direct` checks whether a process is active, releases the state lock, performs substantial network and setup work, spawns the process, and only then records the process in state.

Two concurrent invocations can both pass the initial check before either records ownership, resulting in two direct launches.

The backend needs a reserved `Launching` record or mutex covering the complete start operation—not merely a check followed much later by assignment.

---

### P1 — A failed kill causes Agora to lose the running process

`kill_process` clears `running_process` before invoking `taskkill` or `kill`.

If spawning the kill command fails or the command returns failure, the game may remain running while Agora has already forgotten its PID.

Clear ownership only after confirmed termination, or restore the tracked process state on failure.

---

### P1 — The Rust/TypeScript install protocol is still incompatible

Adding `rename_all = "camelCase"` fixed some field naming, but several representations still disagree.

For example, Rust defines:

```rust
#[serde(tag = "type", rename_all = "camelCase")]
enum OptionalDepsPolicy {
    Include(Vec<String>),
    ExcludeAll,
    Prompt,
}
```

TypeScript expects:

```ts
{ type: 'include', deps: string[] }
```

Other mismatches include:

* `InstallAction` struct-variant fields remain `source_type`, `item_id`, and `candidate_version`, while TypeScript sends camelCase.
* `DepDisposition::InstallCandidate(ResolvedDownload)` does not match TypeScript’s `{ type, artifact }` shape.
* `HealthOutcome::Completed(HealthReport)` does not match the expected `{ type: 'completed', report }` shape.

Use named struct variants and exact JSON fixtures shared between Rust and TypeScript before activating the commands.

---

### P1 — Release C remains an unimplemented scaffold

The resolver still returns a hardcoded blocked stub plan with an empty fingerprint, no dependencies, no real candidate, and no file actions.

The executor still:

* Performs no download or hash verification.
* Does not validate plan freshness.
* Skips missing staged additions rather than failing.
* Writes the existing manifest back unchanged.
* Can return Cancelled after live application without rollback or health verification.
* Treats an unreadable manifest as a green health result.

Meanwhile, every legacy install and removal command remains registered.

C1, C2, C3, and C4 are therefore correctly still marked incomplete.

---

### P1 — Home’s snapshot Restore action opens the wrong instance

Home stores snapshots with only their snapshot ID and instance name.

When Restore is clicked, it calls:

```ts
navigateToInstanceDetail(snapshotId)
```

rather than passing the owning instance ID or performing a restore.

The result is navigation to a nonexistent instance whose ID happens to equal the snapshot UUID.

Other D1 deviations:

* “Continue Playing” navigates to Instance Detail rather than launching.
* Recommendations are placeholder text with a Browse button, not actual explained recommendations.
* Browse has no active-instance context or installed/update/recommendation labels.

---

### P1 — Crash Doctor remains only partially reversible

The deterministic ranking and use of the canonical launch callback are good improvements.

However, when dependents must be disabled, failures are silently ignored and the launch continues with a potentially partial disable set.

When the suspect is ruled out, only the original suspect is re-enabled. Any dependents disabled during that test are not tracked or restored.

It also has:

* No pre-investigation snapshot.
* No restore-all action on cancellation.
* Component-local state that is lost on navigation.
* Hand-rolled fixed overlays rather than the standard accessible dialog primitive.

That fails several D4 acceptance conditions.

## Lower-severity gaps

### P2 — Retention policy is not implemented as defined

`RetentionPolicy` exposes configurable counts and a 2 GB cap, but `retention_plan`:

* Hardcodes keeping one non-LKG snapshot.
* Hardcodes one pre-restore snapshot.
* Never uses `size_cap_bytes`.
* Is not called by the desktop implementation.

### P2 — Process exit state is discarded

The React controller defines an `exited` phase but resets straight to the initial idle state on `game-exited`, losing exit status and useful post-launch information.

Delegated launch is immediately represented as `exited` despite there being no actual process outcome.

### P2 — Authoritative documentation still contains encoding damage

Earlier portions of `MASTER_SPEC.md` still contain corrupted control characters and truncated identifiers, including the CLI name and cleanup backlog entries.

Since this is the source of truth supplied to agents, it should be normalized as UTF-8 before further automated implementation.

## Test and release-gate assessment

The newly added snapshot tests only cover snapshots created and restored by the same current implementation. They do not cover:

* Legacy snapshot manifests.
* Hash tampering.
* Partial-copy failure.
* Rollback failure.
* Interrupted restore recovery.

The LKG tests cover pure classification and diff helpers, but not the required integration from snapshot → launch → promotion → restore.

There are no attached workflow runs for the latest commit.  I also could not perform an independent local build because this environment could not resolve GitHub while cloning, so this review is based on the current repository files and commit diff.

## Current package assessment

| Package    | Status                                                              |
| ---------- | ------------------------------------------------------------------- |
| C0         | Architecture documented, but protocol details still need correction |
| C1         | Stub only                                                           |
| C2         | Disabled and unsafe scaffold                                        |
| C3         | Not migrated; legacy bypasses remain                                |
| C4         | UI prototype invokes unavailable commands                           |
| D1         | Partial, with broken snapshot navigation                            |
| D2         | Architecture documented; implementation incomplete                  |
| D3         | Partial and not safely restorable                                   |
| D4         | Partial; not fully reversible or navigation-safe                    |
| D5         | Incomplete; drift and import are functionally broken                |
| Final gate | Not started/completed                                               |

The safest next correction order is:

1. Repair snapshot backward compatibility and failed-restore rollback.
2. Disable the nonfunctional update confirmation UI.
3. Finish C1 and its exact Rust/TypeScript contract tests.
4. Finish C2 with real staging, verification, stale-plan validation, manifest mutation, and failure injection.
5. Migrate every install/update/remove path through C3/C4.
6. Tie LKG promotion to an actual snapshot ID.
7. Correct drift path normalization and implement a real lockfile round trip.
8. Complete D1/D3/D4 UI behavior and run the full final release gate.
