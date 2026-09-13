# KakaoTalk Layout AdBlocker — Remaining Rust Migration

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.
>
> **Do not restart Phase 0 or Phase 1.** They are done on branch `feat/rust-native-migration`. Read this file, then `kakaotalk_rust_migration_plan.md` and `CLAUDE.md`, then implement from Phase 2.

**Goal:** Finish the Rust native port from the current `kakao-core` golden-parity engine through Win32, shadow mode, event-driven hybrid, mutation, settings, tray, updater, and (only at the end) switching the default release off Python.

**Architecture:** Keep `kakao-core` pure. Put all `unsafe` Win32 in `kakao-win32`. Put process scan, engine loop, CLI, tray, startup, and updater in `kakao-app`. Python v11 stays the reference implementation until Definition of Done.

**Tech Stack:** Rust stable, `windows` (windows-rs), `serde`/`serde_json`, `thiserror`, `tracing`/`tracing-subscriber`, `crossbeam-channel`. Tray later (`tray-icon`). HTTP updater later (`reqwest` + Tokio only if needed). No Tauri/WebView/C++ DLL.

**Branch:** `feat/rust-native-migration` (from `main`). Continue on this branch. Do not rewrite `kakao-core` decision logic to “improve” ads.

**Implementation status (code on this branch):**

- [x] Phase 2 `kakao-win32` (trait, FakeWin32, RealWin32, process, event hook, mutex, startup registry)
- [x] Phase 3 shadow / dump / FakeWin32 scan parity
- [x] Phase 4 WinEvent + coalesce + reconciliation (engine worker)
- [x] Phase 5 hide/resize/close apply + restore on FakeWin32 (live `--apply`)
- [x] Phase 6 v11 settings/rules load
- [x] Phase 7 CLI + named mutex + Win32 tray menu
- [x] Phase 8 startup registry helpers + updater verify/download-hash
- [x] Python v11 moved to `legacy/python-v11`; default release is Rust EXE
- [ ] Live KakaoTalk false-positive matrix (manual)
- [ ] Settings window (tray-only is the shipped UI)

## Global Constraints

- Windows only. Non-Windows fail-fast exit code `2`.
- Layout-only. Do not add hosts/DNS/registry/network blocking.
- v11 algorithm is a frozen contract. If Rust disagrees with Python, change Rust.
- Identity is `(hwnd, pid, class_name)`, never HWND alone.
- Unknown title ≠ empty title. Truncated is a third state.
- Owned `WS_POPUP` ads: `GetParent` returns owner (main). Golden: `tests/fixtures/window_dumps/owned_popup_legacy_ad.json`.
- `Close` is last and more conservative than Hide/Resize.
- Snapshot before every mutation. Restore on OFF/stop.
- WinEvent callbacks: no enum, no mutation, no heavy logging — channel send only.
- No admin/UAC.
- Every step: `cargo fmt`, `cargo clippy --all-targets --all-features -- -D warnings`, `cargo test --workspace` in `rust/`.
- Python tests must stay green: `python -m pyright` and `pytest -q`.
- Do not delete or weaken fixtures to make Rust pass.
- Settings files stay `layout_settings_v11.json` / `layout_rules_v11.json` under `%APPDATA%\KakaoTalkAdBlockerLayout`.
- Version is `11.1.0` (`rust/crates/kakao-app/src/config.rs` and `legacy/python-v11/kakao_adblocker/config/paths.py`).
- Named mutex: `Local\KakaoTalkLayoutAdBlocker_v11`. Diagnostic flags (`--self-check`, `--dump-tree`, `--dump-tree-series`) do not take the mutex.

---

## Current state (already done — do not redo)

Commits on `feat/rust-native-migration`:

| Commit | What |
|---|---|
| `a244e13` | `kakaotalk_rust_migration_plan.md` |
| `6953f65` | Python golden exporter + 10 golden JSON files |
| `ae7eb03` | `rust/` workspace, `kakao-core`, CI `rust-core` job |

Verified when Phase 1 landed:

- `python -m pyright` → 0 errors
- `pytest -q` → 238 passed
- `python -m kakao_adblocker.dev.export_fixture_decisions --check` → goldens match
- `cd rust; cargo test --workspace` → `python_golden_fixtures_match_rust_evaluation` passed (10/10)
- `cargo clippy --all-targets --all-features -- -D warnings` and `cargo fmt --all -- --check` passed

### What exists

```text
kakao_adblocker/dev/fixture_runner.py          # FixtureAPI + FIXTURE_CASES + build_golden_payload
kakao_adblocker/dev/export_fixture_decisions.py
tests/fixtures/window_dumps/*.json             # 10 dumps (do not edit to pass tests)
tests/fixtures/golden/*.json                   # Python 1-tick expected Evaluation
tests/test_golden_decisions_v11.py
rust/Cargo.toml                                # workspace, members = ["crates/kakao-core"]
rust/crates/kakao-core/                        # pure decision engine
.github/workflows/windows-ci.yml               # validate (Python) + rust-core
```

### kakao-core public API (reuse, do not fork)

```rust
// rust/crates/kakao-core/src/lib.rs
pub fn evaluate_dump(dump_json: &str, settings: &LayoutSettings, rules: &LayoutRules) -> Result<Evaluation, serde_json::Error>;
pub fn evaluate_graph(graph: &WindowGraph, settings: &LayoutSettings, rules: &LayoutRules) -> Evaluation;

pub struct WindowIdentity { pub hwnd: i64, pub pid: i64, pub class_name: String }
pub enum WindowText { Known(String), Unknown { error_code: u32 }, Truncated(String) }
pub struct WindowNode {
    pub hwnd: i64, pub pid: i64, pub class_name: String, pub title: WindowText,
    pub structural_parent: Option<i64>, pub owner: Option<i64>,
    pub rect: Option<Rect>, pub visible: bool,
}
impl WindowNode { pub fn win32_parent(&self) -> i64; /* owner.or(structural_parent).unwrap_or(0) */ }

pub struct LayoutSettings { pub enabled: bool, pub aggressive_mode: bool }
pub struct LayoutRules { /* LayoutRulesV11 fields; overlay(json) for golden overrides */ }

pub struct Evaluation {
    pub main_windows: Vec<MainWindowPayload>,
    pub candidates: Vec<CandidatePayload>,
    pub actions: ActionLog,          // hide/show/close unique-sorted; set_pos in call order
    pub state: EngineStatePayload,
}
```

Live scan must build a `WindowGraph` with the same meaning as `FixtureAPI` in `kakao_adblocker/dev/fixture_runner.py`:

- `EnumWindows` = structural parent none (includes owned popups).
- `EnumChildWindows` = dump `children[]` only (owned popup is **not** a child of main).
- `GetParent` = `owner || structural_parent` (owned popup parent is the main hwnd).

If live `Evaluation` and golden `Evaluation` diverge, fix the scanner/graph builder, not the classifier.

### Python reference map

| Concern | File |
|---|---|
| Win32 wrapper | `kakao_adblocker/win32_api.py` |
| Protocol / identity | `kakao_adblocker/protocols.py` |
| Scan | `kakao_adblocker/event_engine/scanner.py` |
| Signals | `kakao_adblocker/event_engine/signals.py` |
| Apply/hide/close/restore | `kakao_adblocker/event_engine/actions.py` |
| Loop / stop / mutex-adjacent engine | `kakao_adblocker/event_engine/controller.py` |
| Dump JSON | `kakao_adblocker/event_engine/dump.py` |
| Resize / tokens | `kakao_adblocker/layout_engine.py` |
| Settings/rules load | `kakao_adblocker/config/models.py`, `storage.py`, `paths.py` |
| CLI | `kakao_adblocker/app/cli.py`, `app/__init__.py` |
| Tray | `kakao_adblocker/ui.py` |
| PID / startup / updater | `kakao_adblocker/services.py` |

### Baseline commands (run after every phase)

```powershell
python -m pyright
pytest -q --basetemp .pytest_tmp
python -m kakao_adblocker.dev.export_fixture_decisions --check
cd rust
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --workspace
```

On ARM64 Windows without ARM64 MSVC CRT, host tests can use:

```powershell
cmd /c "\"C:\Program Files (x86)\Microsoft Visual Studio\2022\BuildTools\VC\Auxiliary\Build\vcvarsarm64_amd64.bat\" && rustup run stable-x86_64-pc-windows-msvc cargo test --workspace"
```

CI `windows-latest` is x64 and uses the `rust-core` job as-is.

---

## Target crate layout after remaining work

```text
rust/
├─ Cargo.toml                          # add kakao-win32, kakao-app members
├─ crates/
│  ├─ kakao-core/                      # EXISTS — decision only
│  ├─ kakao-win32/                     # CREATE — unsafe isolation
│  │  └─ src/{lib.rs,api.rs,process.rs,window.rs,event_hook.rs,mutation.rs}
│  └─ kakao-app/                       # CREATE — binary
│     └─ src/{main.rs,engine/*,config/*,diagnostics/*,tray/,startup/,updater/}
└─ tests/{fixtures (via repo paths), parity}
```

Add workspace members incrementally. Do not create `kakao-app` mutation paths before shadow mode is real.

---

## Phase 2 — `kakao-win32`

**Gate:** Phase 1 golden still 10/10. No KakaoTalk mutation in this phase.

### Task 2.1: Crate + trait matching Python `Win32ApiLike`

**Files:**

- Create: `rust/crates/kakao-win32/Cargo.toml`
- Create: `rust/crates/kakao-win32/src/lib.rs`
- Create: `rust/crates/kakao-win32/src/api.rs`
- Modify: `rust/Cargo.toml` — add member `crates/kakao-win32`
- Modify: workspace deps — add `windows` with features needed below

**windows-rs features (minimum):** `Win32_Foundation`, `Win32_UI_WindowsAndMessaging`, `Win32_System_Threading`, `Win32_System_ProcessStatus` (or `Win32_System_Diagnostics_ToolHelp` for PID snapshot). Add features only when a call needs them.

**Trait to implement** (names aligned with `kakao_adblocker/protocols.py` + `win32_api.py`):

```rust
pub trait Win32Api {
    fn enum_windows(&self, cb: &mut dyn FnMut(i64) -> bool) -> bool;
    fn enum_child_windows(&self, parent: i64, cb: &mut dyn FnMut(i64) -> bool) -> bool;
    fn get_window_thread_process_id(&self, hwnd: i64) -> i64;
    fn get_class_name(&self, hwnd: i64) -> String;
    fn get_window_text_result(&self, hwnd: i64) -> kakao_core::WindowText;
    fn get_parent(&self, hwnd: i64) -> i64; // 0 if none
    fn get_window_rect(&self, hwnd: i64) -> Option<kakao_core::model::Rect>;
    fn get_client_rect(&self, hwnd: i64) -> Option<kakao_core::model::Rect>;
    fn is_window(&self, hwnd: i64) -> bool;
    fn is_window_visible(&self, hwnd: i64) -> bool;
    fn show_window(&self, hwnd: i64, cmd: i32) -> bool;
    fn set_window_pos(&self, hwnd: i64, x: i32, y: i32, w: i32, h: i32, flags: u32) -> bool;
    fn send_message_timeout(&self, hwnd: i64, msg: u32, wparam: usize, lparam: isize, timeout_ms: u32) -> (bool, isize);
    fn update_window(&self, hwnd: i64) -> bool;
    fn get_last_error(&self) -> u32;
}
```

Port text-result rules from `kakao_adblocker/win32_api.py` `get_window_text_result` (lines 169–193):

- `GetWindowTextLengthW == 0` **and** last error ≠ 0 → `WindowText::Unknown { error_code }`
- length 0 and no error → known empty `WindowText::Known("")`
- copy failure when length > 0 → Unknown
- truncated when `copied >= buffer_len - 1`

Constants (same as Python): `SW_HIDE=0`, `SW_SHOW=5`, `SWP_NOSIZE=0x0001`, `SWP_NOMOVE=0x0002`, `SWP_NOZORDER=0x0004`, `SWP_NOACTIVATE=0x0010`, `WM_CLOSE=0x0010`, `SMTO_ABORTIFHUNG=0x0002`.

**Rules:**

- `unsafe` only inside `kakao-win32`.
- `kakao-core` must stay `unsafe`-free (`rg "unsafe" rust/crates/kakao-core` → no matches).
- Provide a test double of `Win32Api` in `kakao-win32` or `kakao-app` that can load dump JSON the same way `FixtureAPI` does (or reuse `WindowGraph` + a graph-backed adapter). Live mutation tests must not target KakaoTalk.

### Task 2.2: Process PID helper

**Files:** Create `rust/crates/kakao-win32/src/process.rs`

Match `ProcessInspector.get_process_ids("kakaotalk.exe")` case-insensitive. Prefer ToolHelp / `windows` crate process snapshot. Isolate per-process failures. Do not require `psutil`.

### Task 2.3: Win32 smoke tests (own hidden window only)

**Test:** `rust/crates/kakao-win32/tests/smoke.rs`

Create a message-only or off-screen test window, then:

1. enumerate and find it
2. class/text/rect/visible
3. hide → not visible → show → visible
4. set_pos then restore original rect
5. destroy the window

Skip or `#[cfg(windows)]`. Never use a KakaoTalk hwnd.

**Commit:** `feat(win32): add safe windows-rs wrapper`

**Done when:** x64 (or documented host) `cargo test -p kakao-win32` passes; no `unsafe` outside this crate.

---

## Phase 3 — Shadow mode (`kakao-app`)

**Gate:** Phase 2 smoke green. **Still no Hide/Resize/Close on real KakaoTalk.**

### Task 3.1: Live graph builder

**Files:**

- Create: `rust/crates/kakao-app/Cargo.toml` (bin `kakao-adblock-rs`, deps: kakao-core, kakao-win32, clap, tracing, tracing-subscriber)
- Create: `rust/crates/kakao-app/src/engine/scanner.rs`
- Create: `rust/crates/kakao-app/src/engine/graph_build.rs`

Build `WindowGraph` from live `Win32Api`:

1. PIDs for `kakaotalk.exe`
2. `enum_windows` filtered to those PIDs
3. For each top-level: class, `get_window_text_result`, `get_parent`, rect/visible
4. Recurse `enum_child_windows` into `children`
5. If `GetParent(hwnd) != 0` and the window is still in the EnumWindows set, store that parent as `owner` and keep `structural_parent = None` (owned popup)

This must match `FixtureAPI._load_node` + `get_parent`.

### Task 3.2: Dump JSON compatible with Python `--dump-tree`

**Files:** Create `rust/crates/kakao-app/src/diagnostics/dump.rs`

Python payload (`kakao_adblocker/event_engine/dump.py` `build_window_dump_payload`):

```json
{
  "timestamp": "...",
  "pids": [],
  "main_windows": [],
  "windows": [ { "hwnd", "class", "text", "pid", "visible", "rect", "depth", "children": [] } ],
  "owned_popups": [ { "...", "owner": <hwnd> } ]
}
```

`--dump-tree` must write this. `--dump-tree-series` adds `frames[].candidates` using `Evaluation.candidates` (same keys as golden).

CLI flags to accept (Python `kakao_adblocker/app/cli.py`): `--dump-tree`, `--dump-tree-series`, `--dump-dir`, `--dump-series-duration-ms` (max 10000), `--dump-series-interval-ms` (min 10), `--self-check`, `--json`, `--minimized`, `--startup-launch`, plus new `--shadow`.

Non-Windows: print to stderr, exit `2`.

### Task 3.3: `--shadow`

**Files:** Create `rust/crates/kakao-app/src/main.rs`, `engine/controller.rs`

```text
kakao-adblock-rs.exe --shadow
```

Does: PID detect, enumerate, `evaluate_graph`, log planned actions.

Does **not:** `ShowWindow`, `SetWindowPos`, `SendMessageTimeout(WM_CLOSE)`.

Log line shape:

```text
[shadow] hwnd=0x... pid=... class=... decision=strong action=hide reason=legacy
```

Default engine path in this phase is shadow even without the flag if mutation is not compiled in; prefer an explicit `--shadow` and refuse mutation unless a later `--apply` (Phase 5) is passed.

### Task 3.4: Compare helper

**Files:** Create `scripts/compare_shadow_dump.py` **or** `rust/crates/kakao-app` subcommand `compare-dump`

Compare Python dump vs Rust dump: pids, main_windows, owned_popups, candidates, planned actions. Exit 1 on mismatch.

Manual matrix (do not claim done without at least one live KakaoTalk shadow run, or record “not run” in the PR):

- friend list, chat list, chat room, settings, popup, ad before/after, minimize/restore, KakaoTalk restart

**Commit:** `feat(app): add process/window scanner` then `feat(app): add shadow mode`

**Done when:** `--dump-tree` JSON loads; `--shadow` never mutates (grep `show_window`/`set_window_pos`/`send_message_timeout` in the shadow code path); golden tests still 10/10.

---

## Phase 4 — Event-driven hybrid

**Gate:** Shadow works. Do **not** remove polling until this phase is measured.

### Task 4.1: `SetWinEventHook` wrapper

**Files:** Create `rust/crates/kakao-win32/src/event_hook.rs`

Events: `EVENT_OBJECT_CREATE`, `SHOW`, `HIDE`, `DESTROY`, `LOCATIONCHANGE`, `NAMECHANGE`, `EVENT_SYSTEM_FOREGROUND`.

Callback body: build a tiny `WinEvent { hwnd, event, time }` and `try_send` on a `crossbeam-channel` (or std mpsc). No enum, no mutation, no `evaluate_graph`.

### Task 4.2: Coalesce + targeted rescan + reconciliation

**Files:** Create `rust/crates/kakao-app/src/engine/events.rs`

```text
callback → channel → engine worker
  → coalesce same HWND (e.g. 50–100ms)
  → targeted rescan of that hwnd’s owner/main subtree
  → evaluate_graph (shadow: plan only)
```

Always keep:

- 2–5 s full reconciliation scan
- Kakao process start burst: `burst_scan_iterations` (default 3) × `burst_scan_interval_ms` (default 20) from `LayoutSettingsV11`

Keep reading `poll_interval_ms` / `idle_poll_interval_ms` from settings even if unused as primary path. Do not delete polling fallback without a benchmark.

**Commit:** `feat(win32): add SetWinEventHook` then `feat(app): add event coalescing and reconciliation`

**Done when:** KakaoTalk restart is detected; missing events recovered by reconciliation; idle wake-ups drop vs Python 50ms/200ms polling (measure, do not assume).

---

## Phase 5 — Real mutation (ordered)

**Gate:** Shadow false-positive goal met on the manual matrix. Mutation still behind `--apply` or a settings `enabled` path that is off until restore tests pass.

Enable in this exact order. Each step has restore before the next is turned on.

1. Hide + restore
2. Resize + restore (`OnlineMainView`: width=parent-2, height=parent-31; `LockModeView`: width=parent-2, height=parent; flags `SWP_NOMOVE | SWP_NOZORDER | SWP_NOACTIVATE`)
3. Popup `WM_CLOSE` timeout 500ms + hide + zero-size fallback (`actions.py` `dismiss_popup_window`)
4. Empty `EVA_ChildWindow` close only after `is_window` becomes false (`closed_windows++` only then). Custom-scroll guard is the **candidate child subtree**, not the whole main window.
5. Aggressive mode (token / subtree token). Geometry-only bottom `Chrome_WidgetWin_*` hide only if `hide_bottom_banner_without_token=true`.

**Before every action re-check:** engine enabled, not stopping, hwnd valid, pid still KakaoTalk, identity still matches, still classified as ad, mode allows the action.

**Snapshot before mutate:**

```text
WindowIdentity, was_visible, rect, parent/owner, hide_reason, timestamp
```

**Shutdown** (copy `LayoutOnlyEngine.stop` behavior):

```text
stop requested → block new mutations → wake worker → join timeout 2.0s
  → restore hidden/resized → stop tray/app
```

Join timeout: warn via state/log, continue shutdown. Restore failures stay in-memory (`restore_failures`, `last_restore_error`, `reset_restore_failures()`). No cross-process snapshot persistence.

**Tests:** graph-backed fake `Win32Api` running `evaluate` then applying hide/show/set_pos against the fake, compared to golden `actions` + disable/stop restore for `owned_popup_legacy_ad` and `popup_adfit_webview`.

**Commits:**

- `feat(actions): enable hide + restore`
- `feat(actions): enable resize + restore`
- `feat(actions): enable popup close fallback chain`

**Done when:** ON→OFF restores; process exit restores; stale HWND is not restored; popup fallback hide is restored; empty EVA does not increment `closed_windows` if the hwnd still exists.

---

## Phase 6 — Settings / rules compatibility

**Files:** Create `rust/crates/kakao-app/src/config/settings.rs`, `rules.rs`

Load `%APPDATA%\KakaoTalkAdBlockerLayout\layout_settings_v11.json` and `layout_rules_v11.json` with the same coerce/fallback as `kakao_adblocker/config/models.py` + `storage.py`:

- Missing fields → defaults
- `ad_candidate_classes` missing/invalid → `main_window_classes`
- `banner_min_height_px > banner_max_height_px` → swap + warning
- JSON parse/type failure → `*.broken-YYYYMMDD-HHMMSS` backup then self-heal defaults
- Broken backups: delete >30 days, keep 10 newest, on every load
- Atomic save (`replace`)
- Mojibake / `�` warning
- Keep unused polling keys for one release (no user-setting loss)

`LayoutRules::overlay` already exists for golden overrides; full file load belongs in `kakao-app`, not by changing golden schema.

**Commit:** `feat(app): load v11 settings and rules`

---

## Phase 7 — Tray / CLI / single instance

**Files:** Create `rust/crates/kakao-app/src/tray/`, `src/startup/` (health probe only until Phase 8)

Tray menu (Python `ui.py` minimum):

- 차단 On/Off
- 공격 모드
- 시작프로그램
- 복원실패초기화
- 창 열기 (optional small window; tray-first, no Tauri)
- 로그 폴더
- GitHub 릴리스
- 업데이트 확인
- 종료 (restore first)

Rules:

- Tray crash must not leave the engine mutating.
- UI thread does not scan Win32; send commands on a channel.
- Mutex `Local\KakaoTalkLayoutAdBlocker_v11`. Duplicate instance: stderr, exit `0`, no engine/tray.
- `--self-check` / dump flags skip mutex.
- `--minimized` / `--startup-launch` compatible with current Run key.

**Commit:** `feat(ui): add tray/settings`

---

## Phase 8 — Startup, updater, package, default switch

### Startup

Port `StartupManager` in `kakao_adblocker/services.py`:

- HKCU Run, no UAC
- Do not overwrite a custom command
- Source-mode: packaged EXE command `KakaoTalkLayoutAdBlocker_v11.exe --startup-launch --minimized` is healthy; do not replace with a cargo path automatically

### Updater

Port `UpdateService` (`services.py` ~502+):

- `MANIFEST_URL` = `https://github.com/twbeatles/kakaotalk-layout-adblocker/releases/latest/download/update.json`
- Ed25519 public key `UPDATE_PUBLIC_KEY_B64` in `kakao_adblocker/config/paths.py`
- Canonical JSON: `json.dumps(..., ensure_ascii=False, sort_keys=True, separators=(",", ":"))`
- HTTPS only; SHA-256; size cap 512MiB; expiry; artifact URL must be `.../releases/download/v{version}/KakaoTalkLayoutAdBlocker_v11.exe`
- Never run the download before hash+signature succeed
- Stage + atomic replace (PowerShell handoff is acceptable if it matches current behavior)

Tokio/`reqwest` may be introduced **here**, not earlier.

### Package / CI

- Produce `KakaoTalkLayoutAdBlocker_v11.exe` (name kept until a planned rename)
- Keep Python `scripts/build_release.ps1` until Rust is default
- CI already has `rust-core`; add `cargo build --release -p kakao-app` when the bin exists
- Interactive tray smoke stays optional (current workflow)

### Benchmark (required before calling Rust “faster”)

Same PC, Python vs Rust: cold start, RSS, idle CPU/wakeups, ad-create→action latency, package size.

### Default switch (last)

Only after the Definition of Done checklist below is all checked:

1. Make Rust the packaged default
2. Move Python to `legacy/` (do not delete in the same commit as the switch if a rollback is needed)

**Commits:** `feat: startup` → `feat: updater` → `perf: benchmark and tune` → `release: switch default implementation to Rust` → `chore: archive Python legacy`

---

## Suggested remaining commit list

```text
feat(win32): add safe windows-rs wrapper
feat(app): add process/window scanner
feat(app): add shadow mode
feat(win32): add SetWinEventHook
feat(app): add event coalescing and reconciliation
feat(actions): enable hide + restore
feat(actions): enable resize + restore
feat(actions): enable popup close fallback chain
feat(app): load v11 settings and rules
feat(ui): add tray/settings
feat: startup
feat: updater
perf: benchmark and tune
release: switch default implementation to Rust
chore: archive Python legacy
```

---

## Definition of Done

Copy of `kakaotalk_rust_migration_plan.md` §18. All must be true:

- [ ] Python golden vs Rust evaluation still 10/10 (`cargo test -p kakao-core`)
- [ ] Live KakaoTalk: no new false positives vs Python
- [ ] hide / resize / popup dismiss work
- [ ] ON/OFF restore and exit restore work
- [ ] HWND reuse safe
- [ ] Event-driven path + reconciliation work
- [ ] tray / startup / updater work
- [ ] Windows release package exists
- [ ] cargo fmt / clippy / test pass
- [ ] Python vs Rust benchmark written
- [ ] Existing user settings migrate
- [ ] Rust can be the default release
- [ ] Python moved to `legacy/` only after the above

---

## Forbidden (repeat of the contract)

- Delete Python first
- Delete or edit dump fixtures to pass Rust tests
- Ignore all popups to reduce false positives
- Drop restore snapshots for speed
- Identity = HWND only
- Mutate inside WinEvent callback
- Busy-loop
- Require administrator
- hosts/DNS/network blocking
- Inject/hook into KakaoTalk.exe
- Patch KakaoTalk binaries
- Spread `unsafe` into `kakao-core` or `kakao-app`
- Remove polling fallback without measurement
- Change v11 heuristics (“improve” the ad algorithm)

---

## First actions for the next agent

1. `git checkout feat/rust-native-migration` and `git log -5 --oneline` — expect `ae7eb03` (or later remaining commits).
2. Run the baseline command block. If golden parity is not 10/10, **stop and fix that** before Win32.
3. Start Task 2.1. Do not implement `--apply` until Phase 3 shadow exists.
4. Check off boxes in this file as you go. Do not rewrite this file except to mark `[x]` and to append newly discovered constraints.
