# Agora v1 — Modrinth-First Launcher Refactor

> **Status:** Implementation-ready. Design resolved across prior planning session.
> **Target:** Refactor Agora from a registry-first launcher into a Modrinth-first, idealized-experience launcher where curated features port to "Modrinth Mode" as an enrichment layer, and ship the missing launch-autonomy and instance-lifecycle layers.
> **Date:** 2026-06-28

---

## Reference Implementations (study/adapt per phase)

The implementing agent MUST consult these two local source trees where cited. They contain proven implementations of the net-new surfaces in this plan. Do not reinvent; adapt patterns.

- **Modrinth App (Theseus):** `C:\Users\jarja\Downloads\Modrinth\code-main`
  - Tauri 2.x + Vue, but the **Rust backend** is the reference for: MSA `MinecraftAuthStep` (9-step p256 ECDSA device-token flow); `.mrpack` install/export (`api/pack/`); DFS dependency resolver (`packages/modrinth-content-management`); `ContentSet`/`ContentEntry`/`InstanceLink` instance model (`state/instances/model/`); in-process MCP-style listener and `notify`-based crash/file watcher (`state/instances/watcher.rs`); JRE auto-install via Adoptium (`api/jre.rs`, `get_optimal_jre_key()`); Discord RPC (`discord-rich-presence` crate).
- **Prism Launcher:** `C:\Users\jarja\Downloads\PrismLauncher-develop\PrismLauncher-develop\launcher\`
  - C++/Qt6, but the **patterns** are the reference for: `ResourceAPI` trait abstraction across 7 platforms (`launcher/modplatform/ResourceAPI.h`, `ModrinthAPI.h`, `FlameAPI.h`); `InstanceCopyTask` with 12 copy prefs incl. hardlink/symlink/clone strategies (`launcher/InstanceCopyTask.h`); `Component`/`PackProfile` patch system (`launcher/minecraft/`); Java detection + validation + auto-install (`launcher/java/JavaUtils.h`, `JavaChecker.h`, `launcher/minecraft/launch/AutoInstallJava.h`); log4j XML log parsing + 4 paste services (`launcher/logs/LogParser.h`, `launcher/net/PasteUpload.h`); Packwiz metadata (`launcher/modplatform/packwiz/Packwiz.h`); server ping triad (`ServerPingTask`/`McClient`/`McResolver`); per-instance directory layout (`FileSystem.h` `FS::` namespace — 25+ path accessors).

---

## Locked Design Constraints (apply throughout)

| # | Decision |
|---|---|
| C1 | **Workspace:** three crates — `agora-core` (lib), `agora-tauri` (GUI bin), `agora` (standalone CLI + `serve` mode). MCP listener is an in-process module of `agora-core`, not a separate binary. |
| C2 | **Three front-ends, one core:** Tauri GUI, `agora` CLI, and the MCP listener are all thin facades over `agora_core::operations::*`. Each op is `async fn(&Ctx, ...) -> Result<T, AgoraError>`. |
| C3 | **Ctx:** single `Ctx` struct (Arc-wrapped) carrying `reqwest::Client`, sqlite handles, paths, keyring accessor, runtime handle. Pass `&Ctx` to every op. |
| C4 | **Concurrency:** CLI + GUI running simultaneously on one machine is **unsupported behavior**, documented, not engineered. One freebie: SQLite WAL mode on `local_state.db` (connection flag, near-zero cost). No per-instance advisory-lock layer. MCP listener is in-process within its host so GUI+MCP concurrency is a non-issue by construction. |
| C5 | **Launch model:** direct Java spawn + MSA as default; official-launcher delegation kept as a user toggle. Phase 5 implements direct spawn and is the security-review gate. |
| C6 | **MSA tokens:** shared keyring entry `agora.msa` under account `<xbox-userhash>`. GUI and CLI read the same store; CLI triggers its own device-code flow when no stored token exists. |
| C7 | **MCP:** off by default per AGENTS.md; user-enables in Settings; GUI generates a random Bearer, stores in keyring `agora.mcp.bearer`, spawns the in-process listener on `127.0.0.1:39741`. Mutating MCP tools route through GUI approval dialogs when GUI is the host; refused when no GUI is host. |
| C8 | **Platforms:** signed Windows MSI/NSIS via Tauri in v1. Linux/macOS compile and "work" but receive no packaging/signing. |
| C9 | **UI customization:** shadcn/ui + Tailwind. System-tracked light/dark theme, reuse Windows personalization accent color when available, plus preset themes + user color picker. Layout presets across ALL major pages (Home/Browse/Instance editor/Settings/Mod detail), resizable splitters, reorderable sidebar/tabs, collapsible sections, plus a dedicated "workbench split-view" for side-by-side comparison. Full VS Code-style free-dock is explicitly **out of v1**; leave an architectural seam. |
| C10 | **Navigation:** left sidebar + Ctrl+K command palette as **first-class peer** (not secondary). Palette searches: installed instances (jump/launch), installed mods (toggle/locate), settings pages/keys, AND live Modrinth catalog (names + inline install). Results sectioned: Instances / Installed mods / Settings / Online catalog — local always above network. |
| C11 | **Progressive disclosure:** global "Advanced mode" toggle + per-section "Show advanced ▾" expanders beneath it. One toggle lights up advanced controls across every page; consistent visual cue (e.g. amber dot / "Advanced" tag). |
| C12 | **Window model:** single-window default; explicit "Open in new window" action per instance; Settings toggle to flip new-window behavior to default. |
| C13 | **Instance editor:** sub-sidebar of tabs (Overview / Mods / World backups / Snapshots / Loadout profiles / Java & args / Advanced). |
| C14 | **GPU detection:** use DXGI (`IDXGIFactory::EnumAdapters` → `DXGI_ADAPTER_DESC`: `DedicatedVideoMemory`, `SharedSystemMemory`, `VendorId`). Do NOT use WMI `AdapterRAM` (DWORD wraps >4GB). Classifier = (vendor × VRAM × dedicated/shared flag). Intel Arc dedicated GPUs promoted; AMD Ryzen APU iGPUs demoted. VendorId map: NVIDIA=0x10DE, AMD=0x1002, Intel=0x8086. |
| C15 | **Mod cache:** content-addressed `mod_cache/<sha256>/file.jar`, hardlink into instance `mods/` (copy fallback on FAT/exFAT USB). Cache location is a user setting with sensible default (`app_data/mod_cache/`); portable mode → `<app_dir>/mod_cache/`. |
| C16 | **AI assistant:** narrow scope only — crash explanation in plain English citing the matched signature's `solution_markdown`. NLP mod discovery is **out of v1** (lives in MCP for power users). BYOK chatbot is **v2**. |
| C17 | **Servers / server browser:** **out of v1.** Schema entries remain (`registry/servers/*.json`) but no server-browser UI ships. |
| C18 | **Curation growth model:** GitHub PR filter (natural non-technical-user filter) → curator team review → approved entries auto-routed to existing triage-poll pipeline (`compiler/compile.py` `_respond_to_circuit_breaker` / `fetch_triage_poll`). No new infrastructure. |
| C19 | **Packs:** two tiers — simple (JSON list of mod IDs + version + status) and complex (same manifest + `override_source` pointing to a GitHub-release zip of configs/assets, SHA-256 verified, extracted into instance). Reuses `download_strategy` pattern. |
| C20 | **Curated overlay:** two-column default on Modrinth project pages — Agora curator note panel pinned at top of body, plus an "Agora Curated" badge next to project name. Movable under the customizable-UI model. |

---

## Phase 1 — Workspace restructure (foundational, do first)

**Goal:** split the current `desktop/src-tauri` monolith into a 3-crate Cargo workspace without breaking the running app.

### Steps
1. Create `crates/` at repo root with `Cargo.toml` workspace manifest listing `agora-core`, `agora-tauri`, `agora-cli` (rename target crate to `agora-cli` for the binary, but name the binary `agora`). Add `agora-mcp` is **not** a separate crate — MCP listener lives inside `agora-core/src/mcp/`.
2. Create `crates/agora-core/` with empty `lib.rs`. Add itself as a dependency of `desktop/src-tauri` (which becomes the `agora-tauri` crate; keep path `desktop/` for the React frontend).
3. **Move modules one at a time** (app stays green commit-by-commit), in this order (lowest coupling first):
   - `models.rs`, `paths.rs`, `error.rs` (new `AgoraError` via `thiserror`), `loader_manifests.rs`
   - `dependency_ops.rs` → `crates/agora-core/src/operations/dependencies.rs`
   - `crash_diagnostics.rs` + `crash_investigator.rs` → `.../operations/crash.rs`
   - `registry.rs` → `.../operations/catalog/registries.rs`
   - `modrinth_raw.rs` split → browse half → `.../operations/catalog/modrinth.rs`; install half → `.../operations/install.rs`
   - `mod_install.rs` → merge into `.../operations/install.rs`
   - `instances.rs` → `.../operations/instances.rs`
   - `registry_sync.rs` → `.../operations/registry_sync.rs`
   - `auth.rs` → `.../operations/auth.rs`
   - `mojang.rs` + `launcher_profiles.rs` → `.../operations/launch.rs` (delegation path only here; direct-spawn added in Phase 5)
   - `governance.rs` → `.../operations/governance.rs`
   - `download.rs` → `.../download.rs` (shared util)
   - `ai_assistant.rs` → `.../operations/ai.rs`
4. Introduce `Ctx` struct (`crates/agora-core/src/ctx.rs`) carrying: `reqwest::Client`, registry read-only sqlite handle, `local_state.db` read/write handle (WAL mode), `Paths`, keyring accessor, `tokio` runtime handle. Wrap in `Arc<Ctx>`.
5. Refactor each moved op to take `&Ctx` (or `&Arc<Ctx>` for spawned tasks) instead of module globals / `AppHandle`. **No `tauri::`, `clap::`, or MCP types leak into `agora-core`.**
6. Collapse `desktop/src-tauri/src/commands.rs` to thin `#[tauri::command]` facades — each ~5-10 lines delegating to `agora_core::operations::*`. `desktop/src-tauri/src/lib.rs` becomes the Tauri builder + plugin registration + `State<Ctx>` construction (~50 lines).
7. Create `crates/agora-cli/` (binary name `agora`) depending on `agora-core` + `clap`. Initial skeleton: `main.rs`, `parse.rs` (clap types), `render.rs` (human table + `--json`). Placeholder subcommands; implementations fill in Phases 9 / 5 / 6 etc.
8. Verify the existing React frontend still builds and runs against the relocated Rust backend (Tauri command signatures preserved).

### Operation surface (the contract all three facades obey)
All ops are `pub async fn`, inputs/outputs `Serialize + Deserialize`. Implement:
- `instances::{list, get, create, delete, launch, clone, import, snapshot, restore}`
- `install::{install_project, apply_plan, remove, check_updates}`
- `dependencies::{install_plan, disable_plan, removal_plan, detect_conflicts, health}`
- `crash::{triage, list_reports, investigate}`
- `catalog::{search, project, for_you, curated_annotation}`
- `registry_sync::sync_registry` + `status`
- `auth::github_*` (existing) + `auth::msa_*` (added Phase 5)

### Validation
- `cargo build --workspace` green.
- `cargo test -p agora-core` for moved modules (port any existing tests).
- Tauri app launches, existing browse/registry/crash commands still work.

---

## Phase 2 — Schema inversion (multi-source, source-agnostic)

**Goal:** make the curated DB a fully functional standalone catalog (not merely a Modrinth overlay), and make Modrinth `project_id` the join key for enrichment with an `agora:<slug>` fallback for GitHub-only mods.

### Manifest schema change (`registry/mods/*.json`, `registry/packs/*.json`)
Replace scalar `download_strategy` + `source_identifier` with a prioritized `sources` array:
```json
"sources": [
  { "type": "github_release", "identifier": "CaffeineMC/sodium", "release_tag": "mc1.21-0.6.0", "sha256": "..." },
  { "type": "modrinth_id", "identifier": "AANobbMI", "sha1": "...", "sha512": "..." }
]
```
Installer walks the array, picks the first source whose platform is enabled in settings. Curator lists GitHub first for determinism (release tags more stable than Modrinth version hashes across republishes).

### Join key
- Add `modrinth_project_id TEXT` as the canonical enrichment join key across `registry_items`, `known_conflicts`, `mod_manual_dependencies`, `mod_jar_aliases`, `curator_reviews`, `pack_mods`.
- For GitHub-only mods (no Modrinth ID), generate a synthetic `agora:<slug>` key. Backfill in compiler.
- Keep internal registry `id` as a secondary surrogate so compiler still works.
- **Join key ≠ sort key.** Sorting uses a separate `normalized_score` (see Phase 7).

### `compiler/compile.py`
- Backfill: curated entries with `download_strategy == "modrinth_id"` MUST declare `modrinth_id`. Entries without one get `agora:<slug>`.
- Validate the `sources` array (each entry has `type`, `identifier`, and the appropriate hash field).
- Social-metrics hydration must also fetch GitHub star counts for GitHub-only mods (proxy for Modrinth download counts) so they have a ranking-comparable signal.

### `CatalogSource` trait (in `agora-core/src/operations/catalog/source.rs`)
Adapt Prism's `ResourceAPI` pattern (see `launcher/modplatform/ResourceAPI.h`). Proposed signature:
```rust
#[async_trait]
pub trait CatalogSource: Send + Sync {
    fn name(&self) -> &str;
    fn is_enabled(&self, ctx: &Ctx) -> bool;
    async fn search(&self, ctx: &Ctx, q: &SearchQuery) -> Result<Vec<CatalogItem>, AgoraError>;
    async fn project(&self, ctx: &Ctx, id: &ProjectRef) -> Result<ProjectDetail, AgoraError>;
    async fn versions(&self, ctx: &Ctx, id: &ProjectRef) -> Result<Vec<Version>, AgoraError>;
    async fn resolve_dependencies(&self, ctx: &Ctx, v: &Version) -> Result<DepGraph, AgoraError>;
    async fn download(&self, ctx: &Ctx, v: &Version, dest: &Path) -> Result<Hashes, AgoraError>;
    async fn verify(&self, file: &Path, expected: &Hashes) -> Result<(), AgoraError>;
}
```
Implementations (v1 ships the first two + registry; others are listed in the "not in v1" appendix):
- `ModrinthSource` (API-driven, always-current)
- `GitHubReleasesSource` (registry-curated GitHub-release bundles)
- `AgoraRegistrySource` (queries signed `registry.db` — the only source that is signed + offline-capable + carries curated notes/conflicts)

Browse = fan-out across all enabled sources, merge on `normalized_score`.

### Dependency declarations
Dependencies reference identity as `{source_type, identifier}` tuples (e.g. `{"modrinth":"P7dR8mSH"}` for Fabric API, `{"agora":"fabric-api"}` for a pure-Agora entry). Resolver asks "for this identity, is there an enabled source that can satisfy it?"; GitHub-only-mods-depending-on-GitHub-only-mods resolve entirely via `mod_jar_aliases` + GitHub release URLs with no Modrinth consulted.

### Category taxonomy unification
Add `modrinth_category_map.json` (or compiler-side enrichment of the existing `categories` table) normalizing Modrinth facets (`optimization`, `performance`, `technology`) and Agora `base_categories` (`["optimization", "rendering"]`) to a single internal taxonomy before For-You overlap computation.

### Validation
- A GitHub-only curated mod sorts correctly in unified browse (uses `net_score` + GitHub star proxy).
- With Modrinth **disabled in settings**, the curated catalog still browses and installs GitHub-release mods.
- Disabling/Enabling a source doesn't break browse for the others.

---

## Phase 3 — Crash triage decoupling + launch-path watcher

**Goal:** Agora's best-in-class crash triage works on any instance (including Modrinth-only), and is wired to the act of launching.

### Decouple from registry DB
- Load the `crash_signatures` corpus into an in-process `Lazy<HashSet<CrashSignature>>` on startup. It is ~4 patterns today, <200 even at scale.
- `crash::triage(&Ctx, log: &str)` no longer hits `registry.db`. The signature set is shipped with `agora-core` (as `include_str!` of `crash-signatures/*.json`) and optionally updated via `registry_sync`.
- Keep the existing `action_button_json` action-button mechanism (one-click "Disable suspected mod").

### Launch-path watcher
- The full wiring to *observe the game process* lands with Phase 5 (direct spawn). In this phase, prepare:
  - A `notify`-based watcher (adapt Theseus `state/instances/watcher.rs`) over each instance's `crash-reports/`, `logs/latest.log`, `saves/`, `servers.dat`.
  - Surface triage results in the instance UI when a new crash report appears.
- When launched via the delegation path (current `launch.rs`), the watcher reads the instance dir asynchronously (best-effort — the official launcher owns the process, so this is "observe the directory," not "observe the process") until Phase 5 makes the process directly owned.

### Validation
- Triage runs on a Modrinth-only instance with zero registry hits.
- Dropping a fabricated crash report into `crash-reports/` triggers the triage UI within the debounced watch window.

---

## Phase 4 — Dependency & conflict resolution ported to Modrinth Mode + pre-launch health scanner

**Goal:** Agora's topological resolver and conflict detection work on Modrinth installs (not just registry items), and users see a go/no-go gauge before launch.

### Transitive Modrinth resolution
- Add `ModrinthSource::resolve_dependencies` walking the Modrinth `version.dependencies` graph (`DependencyType::Required/Optional/Incompatible/Embedded`) recursively. Mirror Theseus's `resolve_content` DFS with `SkippedReason` (AlreadyInstalled, ConflictingDependency, NoCompatibleVersion, Quilt-Fabric API exception) and `loader_aliases` (neoforge↔neo, paper/purpur/spigot/bukkit).
- Merge with any curated `known_conflicts` rows keyed by `modrinth_project_id`.
- Generalize `dependencies::install_plan` / `disable_plan` / `removal_plan` to accept either a registry `id` OR a Modrinth `project_id`. Lookup: query enrichment tables by `modrinth_project_id`; hit → augment the Modrinth-API dep list with curated overrides; miss → fall back to pure Modrinth version metadata.

### Pre-launch `health()` scan (predictive crash interception)
- `dependencies::health(&Ctx, target: &InstanceId) -> HealthReport` runs on launch — predictive interception *before* Java ever boots, so users don't wait 2 minutes for a heavy pack to load only to crash on the title screen:
  1. Scan every JAR in `mods/` via existing `extract_jar_metadata` (reads `fabric.mod.json` / `forge.toml` / `META-INF/mods.toml`).
  2. Parse the explicitly-declared relationship fields — Fabric: `depends`, `recommends`, `breaks`, `conflicts`, `provides`; Forge: `dependencies` with type `required`/`optional`/`incompatible`/`discouraged`. Surface mismatches as blockers/warnings.
  3. Resolve each JAR to its Modrinth `project_id` via `mod_jar_aliases` or Modrinth `version_file` hash lookup.
  4. Run `detect_conflicts` against curated `known_conflicts` (authoritative) + heuristic package-name/version-range collisions (`provides` field overlap — two mods declaring they supply the same mod id; `breaks`/`conflicts` field direct hits; missing `depends`). Second.
  5. Verify Java version matches the loader's requirement.
  6. Return `HealthReport { score: green|yellow|red, warnings: Vec<Warning>, blockers: Vec<Blocker> }`.
- Surface as a color-coded gauge in the instance UI + a go/no-go dialog before launch (in Phase 5, this gates the spawn; in delegation path, it's advisory).
- Neither Modrinth App nor Prism runs this pre-launch — they warn at install only. This is the v1 differentiator.

### Actionable / self-healing health dialog (UX: doctor, not gatekeeper)
- The go/no-go dialog is **actionable, not just blocking.** Each warning/blocker carries inline "Fix It" buttons:
  - Missing required dependency → "Install" runs it through the Phase 2 `CatalogSource::resolve_dependencies` + `install::apply_plan` pipeline.
  - Conflicting-mod pair → "Disable `<mod>`" one-click via `dependencies::disable_plan`.
  - Outdated/broken mod matching a signature → "Disable suspected mod" reuses the existing `action_button_json` mechanism.
- **Safety constraint on self-heal:** a "Fix It" action must route through the SAME install/disable pipeline that re-runs `detect_conflicts`, so a remediation that would itself introduce a new conflict still surfaces that conflict before applying. Never auto-apply a mutation that the health scanner would itself flag. No silent bypass.
- **Per-warning "don't show again" silencing:** minor warnings the user knows are stable (e.g. a soft version mismatch) get a persistent-silence toggle stored per-instance per-warning-signature, so recurring non-issues don't create launch-flow fatigue. Blockers are never silenceable.

### Validation
- A conflicting-mod-pair instance launches no further than the health dialog until resolved.
- "Fix It: Install dependency" resolves the missing dep and re-runs health → gauge turns green.
- Silence on a minor warning persists across launches; a blocker cannot be silenced.

---

## Phase 5 — MSA + direct Java spawn track (SECURITY-REVIEW GATE)

> ⚠️ **This phase is the highest-risk, highest-effort work in the roadmap. A dedicated security review MUST gate the merge of direct-spawn code.** Do not merge without it. The MSA token storage must reuse the existing `keyring` + `aes-gcm` + `pbkdf2` pattern from GitHub OAuth (`auth.rs`) rather than inventing a new credential path.

**Goal:** unblock launch logs, in-app console, per-instance Java, Java auto-download, and a crash watcher that actually observes the process. Keep official-launcher delegation as a user toggle.

### MSA auth (adapt Theseus `state/minecraft_auth.rs`)
- p256 ECDSA device-token flow. `MinecraftAuthStep` enum (9 steps): GetDeviceToken, SisuAuthenticate, GetOAuthToken, RefreshOAuthToken, SisuAuthorize, XstsAuthorize, MinecraftToken, MinecraftEntitlements, MinecraftProfile.
- Token storage: `keyring` entry `agora.msa`, account `<xbox-userhash>`. Shared with CLI (`agora launch` reads same store; if absent, CLI runs its own device-code flow).
- No plaintext token files; no tokens in `local_state.db`.
- **Security-review checklist (must pass before merge):**
  - Token never logged or rendered in UI redacted form.
  - Refresh-token rotation handled; re-auth flow on expiry.
  - No token leaves the machine except to Microsoft/Mojang endpoints (whitelist `login.liveonline.com`, `user.auth.xboxlive.com`, `xsts.auth.xboxlive.com`, `api.minecraftservices.com` — exact hostnames to confirm against Theseus).
  - Keyring absent → `aes-gcm`+`pbkdf2` passphrase vault fallback (portable-mode case) uses the existing `auth.rs` fallback, not a new one.

### Direct Java spawn
- For each instance, compose the classpath + mainClass + args from the Modrinth/Mojang version manifest + loader version info (adapt Theseus `daedalus/src/modded.rs` `merge_partial_version`).
- Spawn the Java process directly via `tokio::process::Command`, feeding the auth token. Owns the process → can stream stdout/stderr, capture exit code, and observe crashes.
- Keep `launch::launch_delegated` (current path) as the user-toggle fallback.

### In-app console
- Stream the spawned Java stdout/stderr into a ring buffer (adapt Theseus `state/process.rs` `LogRingBuffer` at 50k lines). Tauri event channel streams to the React console component.
- Optional: log4j XML parsing for color/level (adapt Prism `launcher/logs/LogParser.h`) — stretch; plain text first.
- Paste upload to 0x0.st / paste.gg / paste.ee (4 services, adapt Prism `launcher/net/PasteUpload.h`) — opt-in, no Agora backend.

### Log sanitization + density toggles (privacy + usability)
- **Automated log sanitizer** (in-process regex cleaner, runs before any log is saved-to-disk-shared, copied to clipboard, or pushed to a paste service): strip personal system data — Windows account usernames in file paths (`C:\Users\JohnDoe\…` → `C:\Users\<user>\…`), machine names, absolute home paths, and any token-shaped strings. Aligns with the Phase 11 privacy ethos — raw logs frequently leak account names that users do not realize they're pasting publicly.
- **Console density toggles:** inline filter checkboxes in the GUI console view to toggle visibility per level (`[INFO]`/`[WARN]`/`[ERROR]`) and per mod-id tag, so users isolate problem areas without leaving the app. Filters apply to both the live stream and the pasted/saved output.

### Per-instance Java + auto-download
- `JvmConfig.java_path` becomes authoritative per-instance when direct-spawn is on.
- Java auto-detection: scan Windows registry + PATH for installed JREs (adapt Prism `launcher/java/JavaUtils.h::FindJavaPaths`). Validate with a `JavaChecker` task (adapt `launcher/java/JavaChecker.h`) returning `Valid|Invalid|TooOld`.
- Java auto-download from Adoptium API (adapt Theseus `api/jre.rs::auto_install_java` + `get_optimal_jre_key` which derives the best JRE from MC manifest + loader version). Per-instance Java dirs mirror Prism (`instanceJavaDir()`).
- **Invisible JVM self-healing (no "you lack Java, confirm install" dialog):** if a user clicks Launch on an instance requiring a missing JRE version, show an inline progress bar over the launch button ("Downloading Java 21 (Adoptium)…") and automatically hand the execution handle to the spawned process when complete. No modal, no path/layout prompt. Directories default to the per-instance Java dir; advanced users override via Phase 10's Java & args tab.

### Hardware-adaptive JVM GC architect (replaces the raw args box)
- Modded Minecraft's default GC causes periodic freeze-frames ("GC stutter"). Players blindly copy-paste flag strings (Aikar's flags) and often misapply Java 8 flags to Java 21. Replace the raw string args box with an Intelligent GC tuning wizard.
- The Rust backend queries total system RAM + CPU thread count via the `sysinfo` crate (NOT the stretch DXGI GPU classifier — this is plain system info, cheap and robust), and reads the instance's target JRE version (known from Phase 5 Java resolution).
- Three selectable engines, computed from those inputs (no manual flag string):
  - **Low-Latency Engine (Java 21 Generational ZGC):** auto-inject `-XX:+UseZGC -XX:+ZGenerational` when the target JRE is Java 21+. Drastically cuts stutters on high-RAM allocations.
  - **High-Efficiency G1GC Engine (Aikar's derivation):** injects dynamically-sized `-XX:G1ReservePercent`, `-XX:MaxGCPauseMillis`, `-XX:+ParallelRefProcEnabled`, region sizing, etc., computed mathematically from the exact RAM the user allocated on the slider.
  - **Manual:** advanced users still edit raw flags (Phase 10 Advanced toggle).
- Heap allocation is a RAM slider with safe OS-headroom guardrails (never allocate >75% of detected RAM by default; warn if it would starve the OS).
- The chosen GC profile serializes into `JvmConfig.custom_args` so it round-trips through snapshots/CLI normally.


### Crash watcher (full version)
- Now that the process is owned, the watcher (Phase 3 scaffolding) gets full process-observation: exit code → health-score mapping, capture crash stack traces, surface the existing `crash::triage` + `crash::investigate` pipeline automatically on non-zero exits.

### Close-on-launch window behavior (with crash-watcher caveat)
- Add a user setting "When I start Minecraft:" with options: **Keep Agora open** (default), **Hide to tray while playing**, **Close window (keep process in tray)**.
- **Critical nuance — "close window" must NOT quit the process.** The crash watcher (above) needs the Rust process alive to observe the Java process and catch crashes + capture exit codes. The Prism pattern is correct here: hide-to-tray + keep watching, re-show the window automatically on a crash (to surface the triage UI) or on game exit. Full process quit is only honored when no game is currently being observed.
- Tray icon shows current state (idle / "Watching <instance>"). Click restores the window.


### CLI launch (`agora launch`)
- Requires stored MSA token OR interactive device-code flow (TTY). Non-interactive context with no stored token **refuses** rather than starting OAuth silently.
- `agora launch <id> | --last | --name "X"`, `--yes` to skip the health-confirm.

### Validation
- Launch via direct spawn produces an in-app console with live log lines.
- Non-zero exit code triggers crash triage automatically.
- Token is absent from any repo file or commit (grep the whole workspace as part of review).
- Delegation toggle still works end-to-end.
- A pasted log does not contain the Windows account username; level/mod-id filters narrow the live console view.
- GC architect detects Java 21 and injects ZGC; G1GC flag sizing scales with the RAM slider; manual mode still editable under Advanced.
- "Close window while playing" hides to tray without quitting; a non-zero game exit re-shows the window with the crash triage UI; full quit is refused while a game is being observed.

---

## Phase 6 — Instance lifecycle

**Goal:** close the largest QOL gap vs both competitors.

### Clone (adapt Prism `launcher/InstanceCopyTask.h`)
- `instances::clone(&Ctx, src, prefs: ClonePrefs)` with the 12 Prism copy prefs: `copy_saves`, `keep_playtime`, `copy_game_options`, `copy_resource_packs`, `copy_shader_packs`, `copy_servers`, `copy_mods`, `copy_screenshots`, `use_sym_links`, `link_recursively`, `use_hard_links`, `use_clone`. Defaults match Prism.

### Import (adapt Prism `launcher/InstanceImportTask.h` + Theseus `api/pack/import/mod.rs`)
- Accept `.mrpack` (Agora already parses these), Prism/MMC zip, CurseForge zip, ATLauncher, GDLauncher formats. Theseus's `ImportLauncherType` enum is the proven model.
- **Do not duplicate world/asset directories:** symlink or hardlink saves/screenshots/resource-packs into the new instance (Prism `useSymLinks`/`useHardLinks`/`useClone`).
- `agora import <path> [--symlink-saves]`.

### Auto-detect launchers onboarding (zero-friction migration)
- For players migrating from another launcher, manually digging through hidden dirs (`AppData/Roaming`) to find an instance folder is the obstructive first step. Solve it in onboarding:
  - On first run, the Rust backend scans standard install paths for the official Minecraft Launcher, Prism, Modrinth App, CurseForge, ATLauncher, GDLauncher.
  - Detected instances render in a clean grid; user one-click imports worlds/accounts/mods using the hardlink/symlink strategies above (no export-to-zip step required). This is the "idealized first launch" vignette — a migrator presses one button and is playing within minutes.
  - Same screen reachable later from Settings → "Detect & import launchers".

### Export to zip (adapt Prism `launcher/ExportToZipTask.h`)
- Per-instance zip export with `.mrpack` (Theseus `api/pack/export_mrpack.rs`) and MMC formats.

### One-click client→server exporter (server environment generator)
- Creating a dedicated server for a custom modpack is a tedious chore — players manually copy the instance dir and delete client-only mods (Sodium, Iris, Controlling, Mod Menu) or the server fatally crashes on boot.
- `instances::export_server(&Ctx, src, dest)` produces a clean server directory/zip containing only server-side mods:
  - Reuses Phase 4's `extract_jar_metadata` reading the `fabric.mod.json` `environment` field (`"client"` → excluded; `"server"`/`"*"` → included) and Forge equivalent (`mods.toml` side declaration).
  - Downloads the appropriate Fabric/Quilt/NeoForge server standalone jar from the Phase 2 loader manifests + SHA-256 verification.
  - Bundles pre-written `start.sh` / `start.bat` with the hardware-adaptive Java args (Phase 5 GC architect) baked in.
- One-click action in the instance panel → "Generate Server Environment". No Agora backend.

### Tags / groups (adapt Prism `launcher/InstanceList.h`)
- `addGroup`, `removeGroup`, `renameGroup`, `setGroupForInstances`. UI: instance sidebar grouping.

### Shared mod-jar cache (C15)
- `mod_cache/<sha256>/file.jar`. Hardlink into instance `mods/` (NTFS supports; copy fallback on FAT/exFAT). Configurable location setting; portable mode default `<app_dir>/mod_cache/`. Keyed by the source's authoritative hash.

### Snapshots + rollback (Prism hardlink strategy)
- Every meaningful instance change (mod add/remove/update, config edit, options change) creates an immutable restore point using hardlinks (cheap — seconds, not GB).
- `instances::snapshot` + `instances::restore`. UI: Snapshots tab in the instance editor sub-sidebar.
- Pairs with world backups: snapshot before launch, auto-prune to last N.

### Mod loadout profiles per instance
- One instance holds multiple named profiles of which mods are `enabled`, switchable at launch. Stores enable-set as a profile. UI: Loadout profiles tab.
- (Borrowed concept from Vortex mod manager — see prior discussion; deploy via hardlink-in-place, not Vortex's staging-dir model.)

### Safe updates + in-context version rollback (fear-free updating)
- **Bulk Safe Update with auto-snapshot:** "Update All" silently captures a pre-update snapshot via the snapshot system above before applying updates. Bulk-updating 30 mods introduces a fear of breaking changes; the auto-snapshot makes it reversible. Pair with the crash watcher (Phase 5): on next launch crash, surface a prominent alert — *"Your game crashed after the recent update. Roll back to the pre-update snapshot?"* — one-click restore.
- **In-context version dropdown:** in the instance editor mod list, each mod row has a version-selector dropdown pulling all versions from Modrinth (via `CatalogSource::versions`). If an update is broken, the user selects a previous stable version inline without re-browsing the global catalog. Pairs with the auto-snapshot so any downgrade is similarly reversible.

### Vanilla keybind conflict analyzer (scoped, honest coverage)
- Players with 150+ mods routinely discover on launch that `R` opens inventory sort + weapon ability + quest log + map overlay simultaneously; resolving this in vanilla options is notoriously frustrating.
- v1 scope (clearly labeled "vanilla + supported mods only"): parse the instance's `options.txt` vanilla keybinds + mods exposing a standard, reliably-readable keybind field. Render a color-coded virtual keyboard — keys with overlapping bindings glow red; users reassign directly in the Agora UI, rewriting `options.txt`.
- **Scope honesty:** mod keybind formats are heterogeneous (Cloth Config, AutoConfig, per-mod raw configs, configs generated only post-launch). Full coverage is unreliable and a "no conflicts" green light users can't trust. v1 ships vanilla + the reliable subset, clearly labeled. Broad mod-config coverage deferred to v1.1/v2 (see appendix). This is bounded, ships real value for the common case, and never overclaims.

### Validation
- Clone, import (with symlinked saves), export round-trip work.
- Snapshot/restore on a broken instance restores to a working state in one click.
- Cache dedup: 5 instances running Fabric API use one disk copy.
- Onboarding auto-detect finds an installed Prism instance and one-click imports it without duplicating saves.
- "Update All" auto-snapshots; a post-update crash offers one-click rollback.
- In-context version dropdown lists prior Modrinth versions; selecting one swaps the mod without re-browsing.
- Client→server export omits client-only mods (verified by `fabric.mod.json` `environment`) and produces a bootable server dir with `start.sh`/`start.bat`.
- Vanilla keybind analyzer surfaces an overlapping-key binding in `options.txt` as red on the virtual keyboard; reassigning in the UI rewrites `options.txt` and clears the conflict.

---

## Phase 7 — Curated-overlay browse + "For You" generalization

**Goal:** the registry becomes the "boutique commentary" layered over the entire Modrinth catalog; recommendation engine works on a 100% Modrinth install.

### Curated overlay on Modrinth project pages (C20)
- When browse loads a Modrinth project, fire `catalog::curated_annotation(modrinth_id)` side-channel. If a row exists, render an "Agora curator note" panel + "Agora Curated" badge (movable under the customizable-UI model).
- Modrinth project *with* Agora badge = curated; *without* = still browsable. Boutique framing preserved.

### "For You" generalization
- Read the instance's installed JARs (via `extract_jar_metadata`), resolve each to a Modrinth `project_id` (via `mod_jar_aliases` or Modrinth `version_file` hash lookup), build the category-overlap set against Modrinth category facets (unified via the taxonomy map from Phase 2), rank candidates by overlap + curated `net_score` (where present) + Modrinth download count (where present).
- Works on a 100% Modrinth install with zero curated manifests.

### Unified sorting (Phase 2 follow-up)
- `normalized_score = w1*net_score + w2*downloads + w3*velocity + immune_boost`. GitHub-only mods substitute GitHub star count for `downloads` (hydrated by compiler). The join key (`modrinth_project_id` / `agora:<slug>`) is NOT the sort key.

### Validation
- A curated row appears as a badge + note on its Modrinth project page; a non-curated row browses normally without the badge.
- For-You on an instance with 10 Modrinth mods returns sensible related candidates with no manifest involvement.

---

## Phase 8 — Two-tier pack install (C19)

### Simple pack
JSON manifest listing curated mod IDs + version + status (required/recommended/optional). Install = resolve each via the multi-source installer from Phase 2. Existing `registry/packs/*.json` format, augmented with the new `sources` array.

### Complex pack
Same manifest + `override_source` pointing to a GitHub-release zip (like a mod using the `github_release` download strategy) or a direct signed URL. Non-mod files (configs, scripts, custom assets, KubeJS/CraftTweaker tweaks, `defaultconfigs/`, `serverpack/`) ship as a zip, SHA-256 verified, extracted into the instance. Reuses `download.rs` pattern verbatim.

### Validation
- Simple pack installs cleanly via the unified installer.
- Complex pack extracts its override bundle into the correct instance subdirs with hash verification.

---

## Phase 9 — Standalone `agora` binary (full CLI surface in v1)

**Goal:** full CLI surface per user decision, backed by `agora-core`, with safe mutation gating.

### Subcommands (clap)
```
agora instances list | get <id>
agora launch <id> | --last | --name "X" [--yes]         # gated by MSA token (Phase 5)
agora mods install <project> [-v ver] -i <instance> [--yes]
agora mods list -i <instance>
agora mods remove <project> -i <instance> [--yes]
agora health <instance>                                    # exit code 0 green / 1 yellow / 2 red
agora registry sync | status
agora import <path> [--symlink-saves]
agora snapshots create|restore <instance|id>
agora auth login                                          # MSA device-code
agora serve                                               # headless MCP host (in-process listener)
```
- `render.rs`: human table (default) or `--json` (scripts/CI).
- Mutating subcommands require `--yes` OR interactive y/N prompt if `stdin.is_tty()`. **Refuse with clear error otherwise** — no silent mutation. Inverse of `rm`'s footgun.
- Read-only subcommands run non-interactively, headless, cron-safe.

### `agora serve` mode
- The MCP listener (`agora-core/src/mcp/listener.rs`) is a `serve(ctx, addr, bearer)` function called by the GUI (in-process task) OR by `agora serve` (standalone host). Same code, two hosts.
- Bound to `127.0.0.1:39741` only; refuses external interfaces. Bearer from `LAUNCHER_MCP_TOKEN` env (standalone) or keyring `agora.mcp.bearer` (GUI host).

### Validation
- `agora instances list` works with GUI closed (reads on-disk state fresh).
- `agora mods install` without `--yes` and without TTY refuses.
- `agora serve` starts the listener on loopback only.

---

## Phase 10 — Progressive-disclosure design pass (unifying UX)

**Goal:** one comfortable-but-customizable system (VS Code model) — Windows-grade polish, not gamer-flashy, not Apple-minimal.

### Component foundation
- Adopt **shadcn/ui** on the existing Tailwind stack (copy-paste components owned in-repo). Pairs with `lucide-react` icons. Neutral modern aesthetic, no Microsoft-design fingerprints.

### Theming
- System-tracked light/dark. Reuse Windows personalization accent color when available (`windows` crate / `GetImmersiveColorFromColorType`). Plus preset themes + user color picker.

### Layout (presets across ALL major pages per C9)
- Home dashboard: pinned instances with quick-launch, For-You feed, recent activity.
- Browse: list + grid toggle (user picks, remembered per content type). CurseForge-desktop density over Modrinth-mobile feel.
- Mod detail: presets — current (existing Agora) / two-column sticky / hero gallery / three-pane master-detail.
- Instance editor: sub-sidebar of tabs (Overview / Mods / Shaders & Packs / World backups / Snapshots / Loadout profiles / Java & args / Advanced).
  - **Shaders & Packs tab:** first-class management for shader packs + resource packs. Modrinth's API natively hosts both content types, so this tab mirrors the Mods interface — browse, install, toggle enable/disable, update, version-dropdown — rather than forcing users to open the instance folder and manipulate files manually. Reuses the Phase 2 `CatalogSource` (serves mods, resource packs, shaders) and the Phase 6 in-context version dropdown.
- Settings: standard.
- Resizable splitters, reorderable sidebar/tabs, collapsible sections, plus dedicated "workbench split-view" for side-by-side comparison (two instances / two mod lists / two mods). Full VS Code-style free-dock is **out of v1**; leave an architectural seam (panel components with serializable layout state).

### Window model (C12)
- Single window default; "Open in new window" action per instance; Settings toggle to make new-window the default behavior.

### Command palette (C10)
- Ctrl+K, first-class peer of the sidebar. Searches: installed instances (jump/launch), installed mods (toggle/locate), settings pages/keys, AND live Modrinth catalog (names + inline install). Sectioned results: local above network.

### Progressive disclosure (C11)
- Global "Advanced mode" toggle + per-section "Show advanced ▾" expanders beneath it. Amber dot / "Advanced" tag visual cue. Set once, applies across every page.

### Validation
- Beginner never sees JVM args / custom commands / component patches by default; toggling Advanced lights them in place.
- Each page's presets are switchable and remembered per content type.
- Workbench split-view shows two instances side by side.

---

## Phase 11 — Privacy / transparency page + lockdown mode

### Privacy settings page
- Enumerate every network call Agora will ever make: Modrinth API, GitHub releases (registry sync), GitHub OAuth (governance), MSA endpoints (launch), Adoptium (Java download). Each independently toggleable.
- "Lockdown" master toggle: disables all external calls, falls back to cached catalog + local mods. (Reuse the "Modrinth disabled" branch from Phase 2.)
- This converts the zero-telemetry absence into a productized feature. Modrinth App serves ads here; the contrast is the positioning.

### Graceful offline auto-state
- Build an automatic offline transition mirroring the Lockdown state. If a network request times out or the machine drops connectivity, the launcher elegantly swaps into an "Offline Mode" visual state rather than hanging, showing broken asset squares, or throwing generic errors.
- In Offline Mode: gently dim global browse features, surface a non-intrusive "Offline" indicator, and keep the local instance library, command palette (`Ctrl+K`), snapshots, and launch fully interactive and fast (cached catalog + local mods). Restore online state automatically when connectivity returns.
- The shared mod-jar cache (Phase 6) makes Offline Mode genuinely useful — a user with a populated cache can build and play modpacks with zero network.

### Validation
- With lockdown on, no outbound network calls occur during browse/launch.
- Each toggle independently cuts its corresponding calls.
- Pulling the network cable mid-session transitions to Offline Mode without errors/hangs; local library + palette + launch remain usable; reconnecting restores online state automatically.

---

## Phase 12 — Remainder (stretch / simple-only)

These land only if cheap, cuttable without remorse:

- **AI crash explainer (narrow):** existing `ai_assistant.rs` context injection → plain-English crash explanation citing the matched signature's `solution_markdown`. One LLM call per crash, budget-safe.
- **DXGI hardware survey (stretch):** the GPU classifier (C14). If the (vendor × VRAM × dedicated/shared flag) detection via DXGI is robust, wire it into a one-time hardware survey that guides a "Buttery vanilla" template (Sodium/Lithium/FerriteCore recommendations, shaders-skip for weak GPUs). If fragile, cut.
- **Discord RPC (stretch, simple-only):** `discord-rich-presence` crate, ~50 lines, off by default. `{state: "Playing <instance>", details: "<loader> <mc_version>"}`. Cut if anything feels fragile.
- **Curator submission/review workflow (C18):** a Governance-page UI walking curators through reviewing an open GitHub PR, approved-and-queued-for-next-compile, auto-routed to the existing triage-poll pipeline. Cheap; lands.

---

## Phase 13 — v1 packaging (Windows-first, C8)

- Signed Windows MSI/NSIS via Tauri's bundler. Primary v1 artifact.
- CLI binary `agora` bundled inside the Tauri app resources (always alongside the GUI), AND published standalone on GitHub Releases for headless/script users.
- `agora-mcp` is not a separate binary; the MCP listener module ships inside both `agora-tauri` and `agora` (Phase 9 `serve` mode).
- Linux/macOS compile and run but receive no packaging/signing in v1.

---

## Appendices

### A. Explicitly NOT in v1 (cut-list)
- Server browser / server UI (C17 — schema entries remain but no UI).
- `agora://` deep-links (deferred; GitHub Pages plumbing exists for later).
- CurseForge source / Packwiz source (`CatalogSource` trait supports them later; Phase 2 ships Modrinth + GitHub + registry only).
- BYO-cloud save sync (Phase 6 world-backup convenience covers the v1 need; cloud-sync conflicts are a rabbit hole).
- NLP mod discovery in-app (lives in MCP for power users).
- BYOK chatbot (v2 — the MCP work in v1 is the foundation).
- Multi-game support / Bedrock / OptiFine / legacy versions (1.2.5, Beta).
- Friends / social / P2P tunneling / Discord RPC-as-social / any Agora-hosted backend.
- Full VS Code-style free-dock (left architectural seam for v1.1).
- Broad mod-config-keybind coverage (Keybind analyzer ships v1 for vanilla + reliable subset only; full heterogeneous-format coverage — Cloth Config, AutoConfig, per-mod raw configs, post-launch-generated configs — deferred to v1.1/v2).
- **Friend-Sync Streams (v1.1 networking bundle):** decentralized recipe-based modpack sync over any free URL host. Bundled with the deferred networking features. **Preserved design notes for implementation:** stream file = a *recipe* (Modrinth project IDs + version hashes), NEVER a payload — Agora only ever pulls JARs from the Modrinth CDN with hash verification, so a malicious stream author can choose mods but cannot serve a tampered JAR; the Phase 4 health scan + curated conflicts still gate launch. Reuses Phase 6 export (subset of `.mrpack` manifest) + mod cache + snapshot system (sync → auto-snapshot → reversible). New piece required: per-mod **ownership tracking** on the installed-mods index ("this mod came from stream S" vs "user manually added") so stream syncs never clobber a user's manual tweak; syncs are never destructive of user-owned mods. Networking path is opt-in, user-provided URLs, fits the Phase 11 privacy toggles + offline mode.

### B. Stretch / simple-only (in v1 only if cheap)
- Discord RPC.
- DXGI hardware survey (only if detection robust enough to be useful).

### C. Risk register
| Risk | Mitigation |
|---|---|
| **MSA / direct-spawn security surface (Phase 5)** | **Gating security review before merge.** Reuse existing `keyring`+`aes-gcm`+`pbkdf2` pattern. No new credential path. Token-absence grep on the whole workspace before merge. |
| Token leakage | Whitelist only MS/Mojang endpoints in Tauri capabilities. Never log/render tokens. |
| Concurrent CLI + GUI undefined behavior (C4) | Documented as unsupported; WAL mode on `local_state.db` protects against the common case. |
| Schema migration breaks registered mods | Compiler backfill preserves existing `id`; additive `modrinth_project_id` + `sources` fields only. |
| Commodifying trust: curated list too sparse for "boutique" feel | Phase 7 makes the registry an *enrichment overlay* over Modrinth, so sparseness no longer breaks browse; the boutique layer augments, doesn't gate. |
| Free-dock escape-hatch debt if implemented later | Phase 10 leaves panel components with serializable layout state as the seam; full-dock in v1.1 builds on it without rewrite. |

---

## Constraints referenced from AGENTS.md (agent must honor throughout)
- `$0/month server footprint` — no Agora-hosted backend in v1.
- `Security by delegation` preserved as an opt-in toggle (C5).
- `Whitelist over denylist` — capabilities, shell scopes, network access all whitelisted.
- Verify every download with SHA-256 / package signatures (Modrinth files: SHA-1/SHA-512; Agora pins: SHA-256 + Ed25519).
- `tauri-plugin-sql` with parameterized queries only. Never concatenate SQL.
- Never render community content with `dangerouslySetInnerHTML`; use `react-markdown`/`remark-gfm` (already deps).
- Never store secrets/tokens in source files or manifests; keyring only.
- Treat `MASTER_SPEC.md` as read-only; do not edit.
- Do not modify `.lock` files or `registry/archived/` history.
- After registry/compiler/loader changes, run `/registry`. After desktop changes, `/desktop`. After web changes, `/web`.
