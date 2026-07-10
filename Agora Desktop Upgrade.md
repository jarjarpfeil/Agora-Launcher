# Agora Desktop Upgrade Execution Plan

## Execution profile

**Primary executor:** DeepSeek V4 Flash
**Routine thinker:** Existing `thinker` subagent using MiniMax M3
**Architecture thinker:** `sol-architect-thinker` using Sol High
**Escalation executor:** Terra High
**Final architecture and safety reviewer:** Sol High

This plan is intentionally divided into small work packages. Complete only one package at a time. Never interpret a later package as permission to begin it early.

---

# 1. Agent operating rules

## 1.1 Required reading

Before editing anything:

1. Read `AGENTS.md`.
2. Read `.kilo/plans/MASTER_SPEC.md`, especially §19.
3. Read the relevant section of `BACKLOG.md`.
4. Inspect the current versions of every file named by the work package.
5. Check whether current `master` has changed since this plan was written.
6. Revalidate the reported defect before changing code.

Where this plan conflicts with current code, do not blindly follow old file or line assumptions. Follow the intended invariant and report the discrepancy.

Where this plan conflicts with `MASTER_SPEC.md §19`, stop and resolve the conflict before editing.

## 1.2 Scope discipline

For every work package:

* Make the smallest coherent change that satisfies the acceptance criteria.
* Do not refactor unrelated code.
* Do not rename unrelated types or files.
* Do not change formatting across entire files.
* Do not introduce new dependencies during Release A.
* Do not modify generated files or lock files unless the package explicitly requires it.
* Do not rewrite `MASTER_SPEC.md` §§0–18.
* Append architecture decisions under §19 when instructed.
* Do not commit or push unless the user has explicitly authorized commits.
* Treat each stated commit boundary as a stopping/reporting boundary when commits are not authorized.

## 1.3 Safety invariants

These invariants apply to every package:

1. Never silently proceed after a failed security, compatibility, dependency, or integrity check.
2. Network downloads must retain the existing allowlist, redirect, signature, and hash-verification controls.
3. Never derive a filesystem filename from a registry ID or mod ID unless the backend explicitly returns that filename.
4. Never invoke direct or delegated launch from a dialog component.
5. Never allow two separate frontend flows to implement the same install semantics independently.
6. Critical filesystem writes must remain staged and atomic.
7. The Rust core must own business rules; React should own presentation and user decisions.
8. Tauri command facades should delegate to `agora-core` rather than accumulate new business logic.
9. Community-supplied content must continue to use the existing sanitized Markdown path.
10. A failed test must be diagnosed, not disabled, filtered, weakened, or converted into a meaningless assertion.

## 1.4 Required validation after desktop changes

Run the repository’s `/desktop` workflow after every desktop package.

Also run, where applicable:

```text
cd desktop
npm run build
npm run test:e2e
```

For Rust changes:

```text
cargo fmt --check
cargo test --workspace
cargo clippy --workspace --all-targets
```

Do not claim success when a command was unavailable or could not run. Report exactly which checks ran and which did not.

---

# 2. Thinker-subagent protocol

## 2.1 Marker meanings

### `[THINKER REQUIRED]`

Before editing, invoke the existing `thinker` skill once.

Use MiniMax M3 for:

* Bounded state-machine design.
* React asynchronous-control questions.
* Choosing between two small implementation approaches.
* Reviewing a proposed interface before it is implemented.
* Identifying edge cases for a tightly scoped package.

The thinker must not edit code. Ask for one decisive response.

### `[SOL-ARCHITECT REQUIRED]`

Before editing, invoke `sol-architect-thinker` exactly once using the prompt supplied by the package.

The Sol architect must return:

1. The chosen architecture.
2. State ownership.
3. Public interfaces or commands.
4. Non-negotiable invariants.
5. Migration sequence.
6. Failure handling.
7. Required tests.
8. Explicitly rejected alternatives.

Do not ask it to implement code.

Do not invoke it repeatedly because its first answer was inconvenient. Escalate only when new evidence proves an assumption in its design was false.

### `[THINKER OPTIONAL]`

Invoke the MiniMax thinker only when the current code presents a real ambiguity that this plan does not settle.

## 2.2 Context limits for thinker calls

Pass only:

* The work package objective.
* Relevant interfaces and short code excerpts.
* Existing constraints.
* The exact decision needed.
* Existing test failures, when applicable.

Do not send the whole repository, entire `MASTER_SPEC.md`, or unrelated files.

## 2.3 Two-attempt escalation rule

Flash gets at most two implementation attempts for the same underlying failure.

After attempt one:

1. Preserve the failing output.
2. Diagnose whether the failure is implementation or design.
3. Make one focused correction.

After attempt two:

* Stop editing.
* Report the exact blocker.
* Ask Terra High to diagnose the existing diff and failures without broadening scope.
* Apply Terra’s recommendation only if it is compatible with the approved architecture.
* Escalate to Sol only when Terra determines that the architecture or specification is wrong.

Do not perform a third speculative Flash rewrite.

---

# 3. Definition of done for every work package

A package is complete only when:

* Its acceptance criteria are met.
* Relevant tests exist.
* Existing relevant tests still pass.
* TypeScript builds successfully.
* Relevant Rust tests pass when Rust changed.
* Error paths are visible to the user.
* Loading and retry behavior are defined.
* No temporary logging or debug UI remains.
* No unrelated diff is present.
* The completion report lists:

  * Files changed.
  * Behavior changed.
  * Tests added.
  * Commands run.
  * Remaining risks.
  * Any assumptions that could not be runtime-verified.

At each commit boundary, stop and request review before proceeding to the next package unless explicitly instructed to continue.

---

# 4. Release A — Critical desktop stabilization

Do not begin Release B until all Release A packages pass review.

## A0. Baseline and specification synchronization

### Objective

Establish a clean baseline and record this upgrade effort without changing runtime behavior.

### Inspect

* `AGENTS.md`
* `.kilo/plans/MASTER_SPEC.md`
* `BACKLOG.md`
* `desktop/package.json`
* Existing desktop E2E tests and Playwright configuration
* Current Git status and branch

### Required work

1. Record the current commit SHA.
2. Run the existing desktop build and test commands.
3. Record existing failures separately from regressions.
4. Add a new `MASTER_SPEC.md` subsection:

```text
§19.13 Desktop Reliability, UX Coherence, and Safe Operations
```

Initially document only these approved principles:

* One canonical launch orchestration path.
* One canonical install transaction path.
* React dialogs return user decisions and do not execute business operations.
* Process state survives navigation.
* User-changing operations are previewable and reversible.
* Existing snapshots become the basis of last-known-good recovery.
* Desktop UX work must include meaningful integration tests.

5. Add a new backlog section for this execution plan with packages A1 through D5.
6. Do not mark any package completed yet.

### Acceptance

* Baseline commands and failures are recorded.
* No runtime code changed.
* §19.13 contains principles, not speculative implementation details.
* Backlog accurately represents the remaining work.

### Commit boundary

`docs: define desktop reliability and UX upgrade`

---

## A1. Registry recovery and first-run dead end

`[THINKER REQUIRED]`

### Thinker prompt

```text
Design a minimal registry-availability state machine for Agora’s Onboarding,
Home, and Browse screens.

Required states:
unknown, downloading, ready, unavailable-with-cache,
unavailable-without-cache, and retrying.

Constraints:
- A user without a cached registry must always have a working recovery action.
- A user with a cached registry may continue offline.
- The app must not claim the registry is ready merely because a command returned.
- Do not redesign the entire app.
- Prefer a small shared presentation component and existing Tauri commands.
- Return state transitions, button behavior, and error-clearing rules only.
```

### Inspect

* `desktop/src/pages/Onboarding.tsx`
* `desktop/src/pages/Home.tsx`
* `desktop/src/pages/Browse.tsx`
* `desktop/src/lib/tauri.ts`
* Registry status and sync commands in Rust

### Required work

1. Enable registry download when no cached database exists.
2. Replace ambiguous `Skip & Finish` behavior:

   * When a cache exists, offer `Continue Offline`.
   * When no cache exists, offer `Continue Without Catalog` only with a clear warning.
   * Always retain a visible Retry action.
3. Verify `has_cached_db` after a registry sync instead of treating any returned status as success.
4. Clear stale errors when retrying.
5. Add a reusable registry-recovery panel used by Home and Browse.
6. Browse must show the recovery panel when registry queries fail because the database is missing.
7. Do not redirect users to Settings for registry recovery.
8. Do not weaken signature or schema validation.

### Acceptance tests

* Fresh install, successful registry download.
* Fresh install, failed download, Retry succeeds.
* Fresh install, failed download, user continues without catalog and can recover from Home.
* Cached registry plus network failure enters offline mode.
* Home’s Download Registry button works when `has_cached_db` is false.
* Browse does not show a generic empty catalog for a missing registry.
* Errors disappear after a successful retry.

### Commit boundary

`fix(desktop): make registry setup recoverable`

---

## A2. Immediate startup shell and theme precedence

`[THINKER REQUIRED]`

### Thinker prompt

```text
Define initialization precedence for Agora’s theme and startup shell.

Constraints:
- React must render a visible shell immediately.
- Stored theme applies synchronously.
- A stored custom accent must not be overwritten by Windows accent detection.
- Windows accent detection is optional enhancement data and may fail or hang.
- Onboarding-state loading must show a branded loading shell, not return null.
- Avoid introducing a new dependency.

Return the initialization sequence, source precedence, and fallback behavior.
```

### Inspect

* `desktop/src/components/theme/theme-provider.tsx`
* `desktop/src/App.tsx`
* Application entry point
* Global CSS theme variables

### Required work

1. Remove the `mounted` gate that returns `null`.
2. Initialize stored theme and custom accent synchronously.
3. Apply Windows accent only when the user has not chosen a custom accent.
4. Treat Windows-accent failure as non-fatal.
5. Do not block rendering while waiting for a Tauri invoke.
6. Replace the App-level onboarding `return null` with a visible startup shell.
7. Parse stored booleans explicitly; never use `Boolean("false")`.
8. Prevent stale asynchronous initialization from overwriting a user change made after startup.

### Acceptance tests

* First render contains visible UI before Tauri invokes resolve.
* A hanging accent request does not leave a blank window.
* Stored custom accent survives startup.
* Light, dark, and system themes apply correctly.
* `"false"` onboarding state is interpreted as false.
* Failed onboarding-setting read enters the explicitly selected fallback behavior.

### Commit boundary

`fix(desktop): render startup shell without blocking on theme hydration`

---

## A3. Browse request isolation and stale pagination

`[THINKER REQUIRED]`

### Thinker prompt

```text
Design the smallest reliable asynchronous request model for Agora Browse.

Current concerns:
- Metadata and search share loading state.
- Search filters can change while prior requests are running.
- browseLoadMore may append results from an old query.
- Client-side query filtering can disagree with backend search.
- The backend may retain pagination state.

Constraints:
- Prefer request-generation IDs or immutable query keys.
- Do not add a state-management dependency.
- Main search and metadata loading must be separate.
- Stale responses must never update visible results.
- Load-more results must belong to the current query generation.
- Errors need Retry and Clear Filters behavior.

Return state fields, request lifecycle, and stale-response rules.
```

### Inspect

* `desktop/src/pages/Browse.tsx`
* Browse-cache and pagination logic in Rust
* Tauri browse command interfaces

### Required work

1. Separate:

   * Metadata loading.
   * Initial search loading.
   * Pagination loading.
2. Create an immutable query key from all active search parameters.
3. Increment a request generation when the query key changes.
4. Ignore initial-search responses from older generations.
5. Capture the generation and query key before `browseLoadMore`.
6. Ignore load-more responses when either value is stale.
7. Reset items, pagination state, and pagination errors on a new query.
8. Prevent duplicate results when pages overlap.
9. Remove the second client-side query filter unless it is intentionally identical to backend semantics.
10. Clear errors at the start of a new request.
11. Show Retry and Clear Filters actions.
12. Remove temporary `console.log` statements.
13. If backend pagination is global rather than query-keyed, correct it in the backend rather than compensating with fragile frontend state.

### Acceptance tests

Use deferred mocked Tauri responses:

* Query A begins, query B begins, B resolves, A resolves: only B is displayed.
* Query A load-more begins, filters change to B, A load-more resolves: A results are ignored.
* Category metadata can load without hiding completed search results.
* Failed search followed by successful Retry clears the error.
* Changing filters resets pagination.
* Overlapping pages do not create duplicate cards.
* Search text produces the same visible result set as the backend response.

### Commit boundary

`fix(browse): isolate query generations and stale pagination`

---

## A4. Command palette reachability and actionable results

### Inspect

* `desktop/src/App.tsx`
* `desktop/src/components/command-palette.tsx`
* `desktop/src/components/Sidebar.tsx`

### Required work

1. Add `Ctrl+K` and `Cmd+K` shortcuts.
2. Prevent the shortcut when a conflicting browser or text-editing command should take precedence.
3. Add a discoverable command-palette button to the sidebar.
4. Separate display sections from actionable results.
5. Keyboard indices must count only actionable rows.
6. Initial selection must point to an actionable result.
7. Arrow navigation must never land on a section heading.
8. Selecting an instance must open that specific instance editor.
9. Reset selection safely when filtering changes.
10. Preserve Radix dialog focus and Escape behavior.
11. Add suitable ARIA listbox or command-result semantics.

### Acceptance tests

* Shortcut opens and closes the palette.
* Sidebar button opens it.
* Enter on the initial result performs an action.
* Arrow keys never select a heading.
* Selecting an instance opens the exact instance.
* A query with no results does not cause modulo-by-zero or invalid-index behavior.
* Mouse and keyboard activation behave identically.

### Commit boundary

`fix(desktop): make command palette reachable and actionable`

---

## A5. Health-dialog launch-mode defect and filename correctness

`[SOL-ARCHITECT REQUIRED — CALL 1 OF 3]`

### Sol architect prompt

```text
Design Agora’s canonical launch orchestration and a safe migration sequence.

Current architecture:
- Instances.tsx chooses direct or delegated launch.
- HealthDialog invokes delegated launch itself after warnings.
- Direct launch returns a PID and component-local state controls running UI.
- Running state is lost when navigating away.
- game-exited events clear local state.
- Health findings expose mod_id, while disable_mod_for_test requires filename.
- The CLI also performs health-gated direct launch.
- Rust business logic should continue migrating into agora-core.

Provide:
1. A minimal P0 patch that fixes the HealthDialog bug now.
2. The target launch-controller architecture for Release B.
3. State ownership between agora-core, Tauri, and React.
4. Commands/events/interfaces.
5. Direct versus delegated behavior.
6. Concurrent-launch policy.
7. Health-decision behavior.
8. Reliable filename handling.
9. Migration sequence.
10. Required tests.

Reject any design where a dialog directly invokes a launch command.
Do not implement code.
```

### Required P0 work

Implement only the architect’s approved minimal patch during A5.

At minimum:

1. `HealthDialog` must not call `launchInstance` or `launchInstanceDirect`.
2. It returns a user decision to its parent.
3. The parent performs the same canonical launch function used by the normal launch button.
4. Direct launch after warning approval must:

   * Call the direct command.
   * Record the PID.
   * Set the running instance.
   * Display the console.
5. Delegated launch must remain delegated.
6. The dialog closes only after successful launch or explicit cancellation.
7. Launch errors remain visible.
8. Health actions that disable a mod must receive a real filename.
9. Extend the Rust health finding and TypeScript type with `filename` when the backend can determine it.
10. If no filename can be determined, do not render the Disable action.
11. Never pass `mod_id` as a filename.
12. Avoid an unnecessary second health scan when the parent already holds a current report, unless the architect explicitly requires revalidation.

### Acceptance tests

* Warning approval preserves direct launch.
* Warning approval preserves delegated launch.
* Direct launch stores PID and shows running state.
* A failed launch leaves the dialog or error state recoverable.
* Cancel performs no launch.
* Disable action passes the reported filename.
* A finding without a filename has no Disable action.
* Health fix re-scan updates the report.
* Existing CLI health-gated launch behavior is not regressed.

### Documentation

Add the approved target launch architecture to `MASTER_SPEC.md §19.13`, clearly distinguishing:

* The minimal A5 repair.
* The Release B target architecture.

### Commit boundary

`fix(launch): preserve launch mode through health approval`

---

## A6. Onboarding and device-flow consistency

### Inspect

* `desktop/src/pages/Onboarding.tsx`
* Relevant GitHub device-flow UI in `Settings.tsx`
* MSA and direct-launch settings
* Current §19 direct-launch decisions

### Required work

1. Preserve service-toggle selections when moving backward and forward.
2. Initialize toggle state from persisted settings.
3. Keep `Open in browser` enabled while polling.
4. Add Copy Code.
5. Add an explicit Cancel action that invalidates the current polling attempt.
6. Show expiration or remaining-time information when available.
7. Do not disable all navigation indefinitely during polling.
8. Apply the same reusable device-flow component to Onboarding and Settings when practical.
9. Correct stale onboarding copy:

   * Direct JVM launch is available.
   * In-app MSA authentication is optional.
   * Mojang launcher delegation remains available as fallback.
10. Clearly distinguish:

* GitHub governance authentication.
* Microsoft/Minecraft launch authentication.
* GitHub Copilot authentication.

### Acceptance tests

* Service selections survive Back and Continue.
* Persisted settings populate the service step.
* Browser fallback remains clickable while polling.
* Cancel stops the active attempt.
* A stale poll result cannot update a newer attempt.
* Onboarding accurately describes current launch behavior.

### Commit boundary

`fix(onboarding): preserve choices and clarify authentication flows`

---

# 5. Release B — Desktop application infrastructure

Do not begin this release until Release A is merged or otherwise approved.

## B1. Typed destination and history model

Use the target application architecture from Sol Call 1.

### Objective

Replace loosely related `activeTab`, `selectedModId`, and `editingInstanceId` state with one typed destination model.

### Preferred constraint

Do not add a routing dependency unless Sol Call 1 explicitly established that it is necessary. A typed hash/history model is acceptable.

### Required behavior

Support destinations equivalent to:

```text
Tab(home | browse | instances | governance | ai | settings)
ModDetail(itemId)
InstanceDetail(instanceId)
```

Required outcomes:

* Back and forward navigation work.
* Refresh restores the current destination where safe.
* Command-palette instance results open the exact instance.
* Selecting a sidebar tab clears incompatible nested state.
* Invalid destinations fall back safely.
* Optional integrations such as AI cannot leave an invalid route active when disabled.

### Acceptance tests

* Browse → mod → Back restores Browse.
* Instances → editor → Back restores Instances.
* Browser/app Back and Forward work.
* Command palette opens a specific instance.
* Invalid IDs show a recoverable not-found state.
* Disabling AI while on AI navigates safely.

### Commit boundary

`refactor(desktop): introduce typed application destinations`

---

## B2. Canonical launch and process controller

Implement the target architecture from Sol Call 1.

### Required behavior

The controller must own:

* Launch mode.
* Health preflight.
* Awaiting-user-decision state.
* Launch-in-progress state.
* Running instance identity.
* PID when applicable.
* Console/event subscription.
* Exit status.
* Kill action.
* Launch errors.
* Concurrent-launch policy.

### Backend requirements

Prefer backend process state as the source of truth.

Add a query command when needed so React can recover running state after:

* Navigation.
* Component remount.
* Window reload.
* Event-listener reconnection.

Every launch entry point must use the same controller.

### Acceptance tests

* Running state survives navigation.
* Direct process exit clears state.
* Reopening Instances shows the correct running instance.
* A second launch obeys the defined concurrency policy.
* Kill targets only the correct PID.
* Delegated launch never displays a fake direct-running PID.
* Health warnings flow through one decision state.
* Event listener cleanup does not leak duplicate handlers.

### Commit boundary

`refactor(launch): centralize launch and process state`

---

## B3. Typed settings access and settings decomposition

`[THINKER REQUIRED]`

### Thinker prompt

```text
Design a typed settings layer over Agora’s current get_setting/set_setting
Tauri API.

Requirements:
- Explicit parsers for boolean, string, number, enum, and nullable values.
- No Boolean(value) coercion.
- One failed setting must not prevent unrelated settings from loading.
- Reads and writes expose pending, success, and error states.
- Existing stored values must remain compatible.
- Settings.tsx must be decomposable into focused sections.
- Do not introduce a new state-management dependency.

Return the TypeScript API, migration behavior, and error strategy.
```

### Required work

1. Introduce typed setting definitions and parsers.
2. Replace broad `Promise.all` initialization with independent or settled reads.
3. Preserve successfully loaded settings when another setting fails.
4. Replace blocking `alert()` calls with inline or centralized accessible notifications.
5. Decompose Settings into focused components:

   * Appearance.
   * Accounts.
   * Integrations.
   * Launch and Java.
   * MCP and AI.
   * Advanced.
6. Load actual MCP approval values before rendering controls.
7. Hide bearer tokens by default.
8. Require confirmation before regenerating a token.
9. Show errors for failed token actions.
10. Add launcher-path Browse, Auto-detect, and Test actions using existing backend abilities or focused new commands.

### Acceptance tests

* Stored string `"false"` loads as false.
* One failed setting does not reset every setting.
* Save errors remain visible beside the affected section.
* Successful saves provide feedback.
* Sensitive tokens are not displayed automatically.
* Token regeneration requires confirmation.
* Launcher path can be selected and tested.

### Commit boundary

`refactor(settings): add typed settings and focused sections`

---

## B4. Accessible dialogs, notifications, and sidebar

### Required work

1. Convert hand-rolled fixed-overlay modals to the existing Radix dialog primitives.
2. Prioritize:

   * Health dialog.
   * Create instance.
   * Paste log.
   * Destructive confirmations that need more context than native `confirm`.
3. Every dialog must have:

   * Accessible title.
   * Description where needed.
   * Focus trap.
   * Escape behavior.
   * Initial focus.
   * Focus restoration.
4. Add one consistent accessible notification system without introducing a new dependency during this package.
5. Add `aria-current` to active navigation.
6. Add sidebar collapse behavior for constrained widths.
7. Ensure the offline banner does not cover interactive content.
8. Correct malformed color-variable usage.

### Acceptance tests

* Keyboard-only users can open, operate, and close every converted dialog.
* Focus returns to the initiating control.
* Screen-reader labels exist.
* Sidebar remains usable at the minimum supported window width.
* Offline status does not obstruct controls.
* Notifications are announced and dismissible.

### Commit boundary

`fix(a11y): standardize desktop dialogs and navigation`

---

## B5. Critical integration-test matrix

`[THINKER REQUIRED]`

### Thinker prompt

```text
Design the strongest practical test matrix for Agora’s critical desktop
workflows using its existing Playwright and Rust test infrastructure.

Constraints:
- Existing browser E2E uses mocked Tauri invokes.
- Native Tauri behavior must not be falsely claimed as tested by Vite-only tests.
- Avoid adding a frontend test dependency unless it provides unique value.
- Prioritize registry recovery, install transactions, health-gated launch,
process exit, snapshots, and rollback.
- Distinguish mocked UI tests, Rust integration tests, and manual native checks.

Return the test layers, fixtures, and minimum release gate.
```

### Required work

Create explicit test layers:

1. **Mocked UI integration**

   * Startup.
   * Registry failure/retry.
   * Browse stale-request handling.
   * Command palette.
   * Health decision.
   * Settings partial failure.

2. **Rust integration**

   * Health findings.
   * Process state.
   * Snapshot/restore.
   * Install staging and rollback.
   * Manifest atomicity.

3. **Native smoke checklist**

   * Real Tauri startup.
   * Real registry sync.
   * Instance creation.
   * Direct launch where credentials/environment permit.
   * Delegated launch.
   * Crash investigation.

Do not filter expected Tauri errors so broadly that real regressions disappear.

### Commit boundary

`test(desktop): add critical workflow integration coverage`

---

# 6. Release C — Canonical safe-install infrastructure

Do not implement this release without the Sol architecture decision.

## C0. Install-transaction architecture

`[SOL-ARCHITECT REQUIRED — CALL 2 OF 3]`

### Sol architect prompt

```text
Design one canonical, transactional install system for Agora.

Current conditions:
- ModDetail primary GitHub path asks for an informational dependency plan.
- Plan failure currently allows installation to continue.
- Raw Modrinth install bypasses that planning.
- Versions-tab install bypasses it.
- InstanceEditor has another add-mod path.
- install_mod_version and install_raw_modrinth are distinct commands.
- Dependency graph logic exists in agora-core.
- Snapshot logic exists in agora-core.
- Thick mod_install logic still remains in desktop/src-tauri.
- CLI install behavior must eventually use the same core logic.
- All network files must remain verified.
- Agora should differentiate through explainable, reversible operations.

Design:
1. InstallIntent.
2. ResolvedInstallPlan.
3. Plan fingerprint or equivalent stale-plan protection.
4. Required and optional dependency resolution.
5. Conflict representation.
6. Existing-file and replacement behavior.
7. Download staging.
8. Hash/signature verification.
9. Snapshot timing.
10. Atomic application.
11. Manifest update.
12. Failure rollback.
13. Post-install health scan.
14. User cancellation.
15. Progress events.
16. Frontend responsibilities.
17. Tauri facade responsibilities.
18. agora-core responsibilities.
19. CLI reuse.
20. Migration sequence from all current install entry points.
21. Required tests.

Fail closed when a required dependency or integrity check cannot be resolved.
Do not implement code.
```

### Documentation

Before implementation:

1. Add the approved architecture to `MASTER_SPEC.md §19.13`.
2. Add explicit invariants and command shapes.
3. Update corresponding Backlog packages.
4. Record rejected alternatives.

### Commit boundary

`docs: define canonical transactional install architecture`

---

## C1. Read-only install-plan contract

Implement the approved plan-resolution portion only.

### Required behavior

The plan must describe:

* Requested item and exact candidate.
* Target instance.
* Source and expected hashes.
* Required dependencies.
* Optional dependencies.
* Conflicts.
* Already-installed dependencies.
* Replacements or version changes.
* Files that will be added, removed, or disabled.
* Snapshot requirement.
* Disk-space estimate when practical.
* Warnings.
* Blocking errors.
* Plan fingerprint or generation.
* Human-readable explanations.

Planning must make no changes to the instance.

### Acceptance tests

* Required dependency appears in the plan.
* Installed dependency is not duplicated.
* Conflict is visible before execution.
* Unresolvable required dependency blocks execution.
* Optional dependency is clearly optional.
* Plan is deterministic for unchanged input.
* No file or manifest changes occur during planning.
* GitHub and Modrinth sources produce the same normalized plan shape.

### Commit boundary

`feat(core): resolve normalized install plans`

---

## C2. Transactional install execution

### Required behavior

1. Validate that the plan is still current.
2. Create the required recovery snapshot.
3. Download all required artifacts into staging.
4. Verify every artifact before touching the live instance.
5. Abort and clean staging on any required failure.
6. Apply files and manifest changes atomically.
7. Avoid partial dependency installation.
8. Emit structured progress.
9. Run a post-install health scan.
10. Return:

    * Installed items.
    * Existing items reused.
    * Warnings.
    * Health result.
    * Snapshot identifier.
    * Rollback availability.
11. Automatically restore or leave the original live state intact when application fails.
12. Never suppress dependency-plan failure and proceed anyway.

### Acceptance tests

Inject failures at each stage:

* Second dependency download fails.
* Hash verification fails.
* Disk write fails.
* Manifest serialization fails.
* Atomic rename fails.
* Health scan returns warnings.
* Health scan returns blockers.
* Cancellation occurs during staging.
* Stale plan is submitted.

After every pre-commit failure, the live instance must be unchanged.

After every application failure, snapshot restore must produce the original instance state.

### Commit boundary

`feat(core): execute atomic verified install transactions`

---

## C3. One frontend install flow

### Required work

Create one reusable install flow used by:

* Main ModDetail Install action.
* Versions tab.
* Raw Modrinth item.
* InstanceEditor Add Mod.
* Future update flows.

The UI must show:

1. Target instance.
2. Exact selected version.
3. Required dependencies.
4. Optional-dependency choices.
5. Conflicts.
6. Files and versions changing.
7. Snapshot/recovery behavior.
8. Verification state.
9. Progress.
10. Post-install health result.
11. Open Instance.
12. Roll Back when available.

Remove direct page-level calls to legacy install commands after migration.

### Acceptance tests

* Every entry point renders the same normalized confirmation.
* Every entry point executes the same backend transaction.
* Required dependencies are installed.
* Optional dependencies honor the user’s selection.
* Blocking conflicts prevent confirmation.
* Failure provides a meaningful recovery action.
* No page retains a bypass path.

### Commit boundary

`refactor(desktop): route every install through safe install flow`

---

## C4. Safe batch updates

Build updates on the same planning and execution system.

### Required behavior

* Detect outdated installed items.
* Resolve updates as a set rather than one independent mod at a time.
* Show version changes and dependency effects.
* Snapshot before applying.
* Stage and verify all required artifacts.
* Apply atomically.
* Run health scan.
* Offer rollback.
* Do not update locked instances without explicit unlock behavior.
* Preserve exact versions when a user chooses not to update.

### Acceptance tests

* Compatible batch update succeeds.
* One failed artifact leaves the original instance unchanged.
* Conflicting updates are blocked before download.
* Rollback restores all previous versions.
* Locked instance cannot be modified silently.

### Commit boundary

`feat(instances): add transactional safe updates`

---

# 7. Release D — Product differentiation and UX coherence

## D1. Action-oriented Home and instance-aware discovery

`[THINKER REQUIRED]`

### Thinker prompt

```text
Design a useful Agora Home information hierarchy using only data that
currently exists or can be obtained through small focused local commands.

Prioritize:
- Continue playing.
- Recover from last crash.
- Registry problems.
- Safe available updates.
- Restore last known good.
- Recent instance changes.
- Compatible recommendations.
- Relevant governance alerts.

Do not invent fake featured or trending data.
Do not turn Home into a dense administration dashboard.
Return section order, empty states, and primary actions.
```

### Required work

Replace the current placeholder-focused Home with action-oriented cards.

Also make Browse optionally aware of an active instance:

* Compatible with selected instance.
* Minecraft version and loader compatibility.
* Already installed.
* Update available.
* Curated status.
* Source.
* Why recommended.

### Acceptance tests

* Home always presents at least one useful next action.
* No misleading featured/trending promise remains without data.
* An instance can be selected as discovery context.
* Compatibility labels reflect backend compatibility checks.
* Recommendations explain their basis.

### Commit boundary

`feat(home): surface recovery, updates, and compatible discovery`

---

## D2. Last-known-good recovery and reproducibility model

`[SOL-ARCHITECT REQUIRED — CALL 3 OF 3]`

### Sol architect prompt

```text
Design Agora’s last-known-good and reproducibility architecture.

Existing capabilities:
- Snapshots.
- Loadouts.
- Instance manifests with hashes and source metadata.
- Direct launch with process events.
- Crash investigation.
- Transactional install/update architecture from Release C.

Desired product behavior:
- Automatic snapshots before meaningful changes.
- Successful-launch markers.
- Diff since last known good.
- One-click restore.
- Guided crash isolation without losing user state.
- Exportable reproducible instance lockfile.
- Detection of drift from the lockfile.
- No hosted backend.
- Local-first privacy.
- Hash and source verification must remain authoritative.

Provide:
1. Data model.
2. When a snapshot becomes last-known-good.
3. Definition of successful launch.
4. Snapshot retention policy.
5. Change/diff representation.
6. Restore semantics.
7. Interaction with crashes and health warnings.
8. Lockfile schema and signing or verification approach.
9. Config-hash policy.
10. Import and drift behavior.
11. agora-core ownership.
12. Tauri/React interfaces.
13. Migration sequence.
14. Required tests.

Do not implement code.
```

Document the approved decision in `MASTER_SPEC.md §19.13` before implementation.

### Commit boundary

`docs: define last-known-good and reproducible instance model`

---

## D3. Last-known-good user flow

Implement the approved recovery model.

### Required behavior

* Snapshot automatically before install, remove, update, bulk enable/disable, pack import, or other material changes.
* Mark a state last-known-good only under the approved successful-launch rule.
* Show:

  * Last known good timestamp.
  * Changes since that state.
  * New, removed, disabled, and updated mods.
* Provide one-click restore with confirmation.
* Restore atomically.
* Retain enough history for recovery without unbounded disk growth.
* Explain when no known-good state exists.

### Acceptance tests

* Successful launch establishes a known-good state.
* Failed launch does not.
* Install after known-good produces a clear diff.
* Restore returns exact files and manifest state.
* Restore failure does not destroy the current state.
* Retention removes only snapshots allowed by policy.

### Commit boundary

`feat(recovery): add last-known-good instance restoration`

---

## D4. Explainable Crash Doctor

Build on the existing deterministic crash investigator rather than replacing it with AI.

### Required flow

1. Identify crash and fingerprint.
2. Show ranked suspects and evidence.
3. Show whether each signal came from:

   * Stack frames.
   * Curated conflicts.
   * Prior local crashes.
   * Dependency relationships.
   * Confirmed prior fixes.
4. Create or reference a temporary recovery snapshot.
5. Disable one test candidate by filename.
6. Launch through the canonical launch controller.
7. Ask whether the crash was fixed.
8. Record the outcome.
9. Restore disabled mods when the candidate is ruled out.
10. Continue to the next candidate.
11. Let the user restore the pre-investigation snapshot.
12. Use AI only for explanation or supplementary interpretation, with clear labeling.

### Acceptance tests

* Test-disable always uses filename.
* Ruled-out mod is restored.
* Confirmed culprit is recorded.
* Investigation can be cancelled and fully restored.
* Navigation does not lose investigation state.
* AI unavailability does not block deterministic investigation.

### Commit boundary

`feat(crash): add guided reversible crash isolation`

---

## D5. Reproducible sharing and drift detection

Implement the approved lockfile design.

### Required behavior

Export a lockfile containing, as approved:

* Minecraft version.
* Loader and loader version.
* Exact installed item versions.
* Source identities.
* File hashes.
* Enabled state.
* Relevant instance settings.
* Config hashes or approved config metadata.
* Lockfile schema version.
* Creation metadata that does not expose private account information.

Support:

* Clone from lockfile.
* Verify existing instance.
* Show drift.
* Repair drift through the safe-install transaction.
* Preserve unavailable artifacts as explicit unresolved entries.
* Never substitute an unverified “closest” version silently.

### Acceptance tests

* Export then import reproduces exact verified artifacts.
* Modified jar produces drift.
* Missing artifact is reported.
* Different config produces config drift according to policy.
* Repair uses the transaction system.
* Invalid or future-schema lockfile fails safely.

### Commit boundary

`feat(sharing): export and verify reproducible instance lockfiles`

---

# 8. Deferred items

Do not include these in the preceding packages unless separately authorized:

* Dev Mode sandbox implementation.
* Remote MCP access.
* New hosted services.
* Social feeds requiring a new backend.
* Telemetry uploads.
* Mobile app.
* Large visual rebrand.
* Rewriting the entire component library.
* Replacing Tauri.
* Replacing SQLite.
* General migration of every thick desktop Rust module unrelated to the active package.
* Localization of every existing UI string.
* Plugin marketplace architecture.

---

# 9. Final release gate

Before declaring the upgrade complete:

1. Run all TypeScript builds.
2. Run all Playwright tests.
3. Run workspace Rust tests.
4. Run Clippy and formatting checks.
5. Perform the native desktop smoke checklist.
6. Perform a fresh-install registry test.
7. Perform an offline cached-registry test.
8. Perform a direct-launch health-warning test.
9. Perform a delegated-launch health-warning test.
10. Perform a successful transactional install.
11. Inject a failed transactional install and verify no live changes.
12. Perform a safe update and rollback.
13. Establish and restore last known good.
14. Complete a guided crash-isolation cycle.
15. Export, import, and verify a reproducible lockfile.
16. Review keyboard navigation and dialog focus.
17. Review the complete diff for:

    * Silent error suppression.
    * Duplicate launch paths.
    * Duplicate install paths.
    * Business logic left in React.
    * Unverified writes.
    * Stale comments or onboarding text.
    * Debug logs.
    * Tests that assert only that the page rendered.

## Final reviewer assignment

Have DeepSeek V4 Pro review the complete diff first.

Its review prompt should require reproducible findings involving:

* Race conditions.
* Stale closures.
* Incorrect Tauri argument names.
* Direct/delegated launch regressions.
* Partial transaction states.
* Missing rollback.
* Filename versus mod-ID confusion.
* Tests that do not exercise the real defect.

Then use Sol High once for final architecture acceptance only if the total changes still match the three approved Sol architecture decisions.

Do not ask Sol to rewrite the finished implementation unless the review uncovers a fundamental architecture violation.
