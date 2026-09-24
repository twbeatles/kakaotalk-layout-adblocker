# AI Context: KakaoTalk Layout AdBlocker v11

## Project Snapshot

- Platform: Windows
- Runtime: Rust native (`kakao-adblock-rs`); Python v11 reference in `legacy/python-v11`
- Version line: `v11`
- Scope: Layout-only ad blocking (no hosts, no DNS flush, no AdFit registry writes)
- Non-Windows execution: fail-fast with message and exit code `2`
- Polling model: adaptive (active 50ms / idle 200ms by default; after 10s without KakaoTalk window events the idle interval backs off to `idle_backoff_max_ms`, 1s by default, while the WinEvent hook is installed)
- Rust runtime specifics (v11.1.5): see `CLAUDE.md` "Rust 활성 런타임 동작" — BOM-tolerant JSON, `WS_VISIBLE` restore check, hung-window guard + 3s shutdown join, KakaoTalk-absent PID scan backoff (1s after 5s, 2s after 30s), tick panic containment

## Ad-Blocking Algorithm Contract

- Treat the v11 ad-blocking algorithm as a fixed maintenance contract, not as a free-form optimization target.
- Keep the strategy `layout-only`; do not reintroduce hosts, DNS, registry, or network-level blocking behavior.
- Preserve the current blurfx-aligned semantics:
  - main-window identification
  - legacy-signature top-level candidate hide
  - subtree-token aggressive hide
  - guarded popup dismiss
  - confirmed-ad-signal empty `EVA_ChildWindow` close
- Keep the empty `EVA_ChildWindow` custom-scroll guard scoped to the candidate child subtree, not the whole main window.
- Keep token-less bottom `Chrome_WidgetWin_*` geometry-only hides disabled by default; only allow them through `hide_bottom_banner_without_token=true`.
- Keep non-empty popup hosts blocked by default unless they match `popup_host_text_contains`.
- If engine logic must change, require live dump evidence (`--dump-tree` / `--dump-tree-series`), a regression fixture/test, and matching `.md` updates in the same change.
- Prefer rules/fixture/test updates first; treat core engine-algorithm changes as a last resort.

## Runtime Entry

- Main binary: `dist/KakaoTalkLayoutAdBlocker_v11.exe` (`rust/crates/kakao-app`)
- HKCU Run startup: wait for `Shell_TrayWnd`, retry `NIM_ADD`, keep the tray message loop if the icon is still missing, then re-add on `TaskbarCreated`/timer. Repair missing/stale/missing-target Run commands and re-enable `StartupApproved` when `run_on_startup` is set.
- Python reference: `legacy/python-v11/kakaotalk_layout_adblock_v11.py`
- Legacy script: `legacy/카카오톡 광고제거 v10.0.py` (deprecated notice only)
- `--dump-tree` runs in a lightweight path without UI/tray module import. Child trees go in `windows`; owned popup ad hosts go in `owned_popups`. Graph child edges are direct children (`GetParent`), not flattened `EnumChildWindows` descendants.
- `--self-check` runs diagnostics only (no UI/tray/engine start)
- default `--self-check` treats tray import failure as optional; `--strict-self-check` upgrades it to core failure for packaging/release validation
- `--self-check --json` emits structured diagnostics, and packaged smoke can persist the same payload via an internal report path
- normal UI launch is single-instance guarded by the Windows named mutex `Local\KakaoTalkLayoutAdBlocker_v11`; duplicate UI launches print an already-running message to stderr and exit `0` before Tk/tray/engine startup
- `--self-check`, `--dump-tree`, and `--dump-tree-series` remain diagnostic paths and do not acquire the single-instance mutex
- dump/report/startup-trace write failures return stderr plus exit `1`; `--dump-series-duration-ms` is capped at `10000` and `--dump-series-interval-ms` is floored at `10`
- package `kakao_adblocker` exports are lazy-resolved via `__getattr__`
- static analysis baseline is fixed by root `pyrightconfig.json`; active scope is `kakao_adblocker`, `tests`, and `kakaotalk_layout_adblock_v11.py`
- preferred local verification entrypoint is `.\scripts\dev_check.ps1` (`-SkipTests` runs format/clippy/pyright only)
- `scripts/dev_check.ps1` covers Rust `fmt --check`, `clippy`, `cargo test` alongside Python `pyright` and `pytest`, ensuring local checks match `windows-ci.yml`
- `.githooks/pre-commit` enforces `.\scripts\dev_check.ps1 -SkipTests` to prevent CI failures from slipping into commits
- `scripts/dev_check.ps1` / `scripts/smoke_check.ps1` use `--basetemp .pytest_tmp` and clean the workspace-local pytest temp directory when possible

## Architecture

- `app/`
  - `main`, CLI parser, self-check, startup trace helpers
  - package facade preserves existing `kakao_adblocker.app` import surface and test monkeypatch points
  - internal implementation is split into `cli.py`, `self_check.py`, `startup.py`
- `config/`
  - `LayoutSettingsV11`, `LayoutRulesV11`
  - AppData path: `%APPDATA%\KakaoTalkAdBlockerLayout`
  - runtime path resolution is lazy via `resolve_app_data_dir()` and `get_runtime_paths()`
  - compatibility aliases (`APPDATA_DIR`, `SETTINGS_FILE`, `RULES_FILE`, `LOG_FILE`) stay exported for callers, but internal runtime logic uses the helper lookups
  - advanced perf knobs: `idle_poll_interval_ms`, `idle_backoff_max_ms` (Rust only), `pid_scan_interval_ms`, `cache_cleanup_interval_ms`
  - burst scan knobs: `burst_scan_iterations`, `burst_scan_interval_ms`
  - missing new perf fields are backfilled with safe defaults
  - new rules flags: `hide_bottom_banner_without_token=false`, `close_empty_eva_child_requires_ad_signal=true`
  - weak/restore tuning: `weak_signal_confirm_ticks=2`, `hidden_restore_grace_ms=250`
  - new rules keys: `popup_ad_classes=["AdFitWebView"]`, `popup_search_depth=2`, `popup_host_text_contains=[]`, `popup_host_require_empty_text=true`
  - rules loader falls back `ad_candidate_classes` to `main_window_classes` when missing/invalid
  - malformed/non-object JSON input is backed up as `*.broken-YYYYMMDD-HHMMSS` and then self-healed with default JSON
  - inverted banner bounds (`banner_min_height_px > banner_max_height_px`) are auto-normalized
  - broken-backup cleanup policy is enforced on every load (`>30 days` purge + keep latest `10`)
  - first-run runtime bootstrap for settings/rules/log now uses create-if-missing semantics so existing files are not overwritten
  - rules string integrity self-check warns on mojibake signatures / replacement char (`�`)
  - `consume_load_warnings()` exposes startup warnings to app layer
- `event_engine/`
  - `LayoutOnlyEngine`: single watch+apply polling loop
  - packageized internals: `controller.py`, `scanner.py`, `signals.py`, `actions.py`, `dump.py`, `models.py`
  - when blocking is OFF, watch/apply both pause and loop waits in low-cost mode (`1.0s`)
  - main window detection uses `main_window_classes` from rules
  - candidate and confirmed main-window counts are tracked separately; apply uses confirmed handles only
  - ad candidate filtering uses `ad_candidate_classes` (default: `EVA_Window_Dblclk`, `EVA_Window`) + legacy exact/substring signatures
  - non-main top-level KakaoTalk windows are scanned for popup descendants up to `popup_search_depth`; default popup handling still requires an empty host title or an allowlisted title substring before dismissing `AdFitWebView`-style popups
  - popup dismiss uses `SendMessageTimeoutW` for `WM_CLOSE` with the default 500ms timeout, validates actual close/hide/zero-size success, and reports failures into `last_error` / log
  - if a popup survives `WM_CLOSE` and hide/zero-size fallback is applied, the fallback is tracked as a `popup` hidden snapshot and restored on OFF/stop or when the popup signal disappears
  - empty `EVA_ChildWindow` close keeps its custom-scroll guard scoped to the candidate child identity/subtree and recalculates it per apply tick, not from a persistent cache
  - main windows keep the strong `top-level + main class + child signature` guard; title mismatch can still confirm via child signature fallback (`OnlineMainView` / `LockModeView`)
  - synchronous warm-up scan/apply on engine start runs only when enabled
  - empty-string text cache uses short TTL refresh to reduce startup detection lag
  - hidden/moved windows are restored when blocking is disabled or engine stops
  - scan/apply and OFF/aggressive-OFF/stop restore paths are serialized by a single-flight lock
  - once stop begins, new hide/close/apply work is blocked so a timed-out join does not re-hide windows after restore
  - aggressive-hide windows are restored immediately when aggressive mode is turned OFF, followed by an immediate rescan/reapply
  - hidden windows are automatically restored when they no longer match aggressive/legacy signatures, preventing stale hides
  - stop join timeout (`2.0s`) emits state/log warning and proceeds with shutdown flow
  - restore failures keep snapshots for retry on next restore cycle
  - restore-failure retry snapshots are process-local memory only; cross-process snapshot persistence after app restart is intentionally not implemented
  - `EngineState` includes `restore_failures` / `last_restore_error`
  - `WindowIdentity(hwnd,pid,class)` keyed caches protect against HWND reuse side effects
  - hidden/candidate aggressive subtree checks bypass stale non-empty text cache with fresh text reads so token disappearance can restore after `hidden_restore_grace_ms`
  - empty `EVA_ChildWindow` close is counted only after the target window actually disappears, not merely after a successful `SendMessageTimeoutW` request
  - watch scan path avoids geometry/visibility calls; dump-tree path still collects full geometry
  - `--dump-tree-series` stores frame-by-frame candidate decision previews alongside the tree dump, including both popup host and matched popup descendant candidates
  - process-id scan and cache cleanup are interval-throttled for idle CPU savings
  - ultra-fast process liveness check (`is_process_alive`, <0.001ms) gates full process snapshot scans (5s interval when alive)
  - zero-allocation UTF-16 slice comparison (`eq_wide_ascii_case`) eliminates heap churn during process table enumeration
  - non-KakaoTalk window event filtering suppresses spurious worker loop wakeups
  - evaluation engine passes precomputed `confirmed_set` handles for O(1) checks, avoiding repeated full main-window candidate scans and string allocations
  - process scan warnings (psutil failure, tasklist fallback/failure) are propagated to status/log (`last_error`)
  - default idle->active detection target is <= 200ms
  - `report_warning()` allows startup warning propagation to tray status context, and the prioritized startup warning is applied after engine start so it remains visible
- `layout_engine.py`
  - Main/lock view resize formulas
  - aggressive detection separates token signals from geometry-only bottom-banner heuristics
  - token-less bottom `Chrome_WidgetWin_*` panels are not hidden by default; subtree token signals can still trigger aggressive hide
  - short ASCII ad tokens are word-boundary matched to reduce false positives
  - main child resize uses `SWP_NOMOVE | SWP_NOZORDER | SWP_NOACTIVATE` to avoid z-order and activation side effects
- `protocols.py`
  - structural typing boundaries for Win32 API / joinable thread / UI root / engine state
  - keeps runtime module contracts compatible with test doubles
- `ui.py`
  - `TrayController` (status, toggle, aggressive mode, startup, restore-failure reset, logs, release page, exit)
  - startup notice is skipped when launching minimized
  - minimized-start requests are ignored when tray modules are unavailable
  - tray readiness is confirmed via startup signal; startup timeout (`1.5s`) disables tray mode
  - tray startup failure and unexpected tray runtime exit both schedule fixed recovery retries (`3` attempts, `3000ms` interval)
  - unexpected tray runtime exit disables tray mode and restores main window visibility
  - startup fallback visibility is tracked so successful tray recovery can auto-hide the window again only during startup-minimized recovery
  - window close action switches from hide to shutdown when tray is unavailable
  - startup setting is synchronized from registry on app start
  - when Run registration is enabled, startup command health is probed and stale/missing command lines are auto-repaired when possible
  - in source mode, an existing Run registration that points to an existing `KakaoTalkLayoutAdBlocker_v11.exe --startup-launch --minimized` packaged EXE is treated as healthy and is not auto-overwritten with a source-script command
  - custom/unknown Run commands are left unchanged by automatic sync and reported as `custom command left unchanged`; explicit UI toggles still write the current-mode standard command
  - startup toggle rolls registry back on settings-save failure
  - setting save failures roll back values (`enabled`, `run_on_startup`, `aggressive_mode`)
  - aggressive mode toggle is pushed into the engine immediately after a successful save
  - `로그 폴더 열기` / `GitHub 릴리스 열기` failures surface as short UI warnings instead of failing silently
  - status text includes last error and last tick context
  - status text shows confirmed main-window count and appends candidate count only when larger
  - status text labels cumulative counters explicitly (`누적 숨김`, `누적 닫힘`, `누적 리사이즈`)
  - popup close request / hide fallback / zero-size fallback counters are tracked separately and shown only when nonzero
  - status text includes restore failure count/context when present
  - controller-local UI warnings (`tray unavailable`, startup registry rollback issues) surface when engine error is absent
  - pystray/Pillow are loaded lazily and retried after TTL (30s) when import fails
  - tray callbacks are queued and drained on Tk main thread
  - `_safe_after()` does not call Tk/root methods from tray or worker threads; root liveness and callback execution are checked only during Tk main-thread drain
  - status tick scheduling (`root.after`) also swallows shutdown-race errors
  - startup load-warning propagation uses priority (`heal failure > auto-heal > others`)
- `services.py`
  - process scan, startup registry, shell/open-url helpers
  - psutil process scan uses per-process exception isolation
  - psutil init/loop failure falls back to `tasklist` scan
  - `ProcessInspector.consume_last_warning()` provides scan diagnostics to the engine
  - `StartupManager.probe_access()` validates both Run-registry read and write access
  - startup Run command parsing prefers Windows `CommandLineToArgvW` plus environment-variable expansion, with the older parser retained as fallback
  - `StartupManager.registration_health()` classifies `not_registered`, `healthy`, `stale_command`, `missing_target`, `custom_command`
  - source-mode registration health accepts the compatible packaged-EXE Run command above; frozen mode still requires the current EXE command
  - diagnostics helpers: `ProcessInspector.probe_tasklist()`, `StartupManager.probe_access()`, `StartupManager.probe_registration_command()`
- `win32_api.py`
  - user32 API bindings explicitly define `argtypes/restype`
  - `get_window_text_result()` reads text with a `GetWindowTextLengthW`-sized dynamic buffer and reports known/truncated/error state; `get_window_text()` remains the compatible string-returning wrapper
  - exposes `get_last_error()` for debug telemetry on ShowWindow/SetWindowPos failures

## Key Resize Rules

- `OnlineMainView*`:
  - width = `parent_width - 2`
  - height = `parent_height - 31`
- `LockModeView*`:
  - width = `parent_width - 2`
  - height = `parent_height`

## Important Files

- `layout_settings_v11.json`
- `layout_rules_v11.json`
- `kakaotalk_adblock.spec`
- `legacy/specs/kakaotalk_adblock_v10.spec` (legacy filename compatibility shim for the active v11 spec surface)

## Build Notes

- `kakaotalk_adblock.spec` resolves entry script and data files from project-root absolute paths for stable `pyinstaller` invocation.
- `kakaotalk_adblock.spec` also validates the packaged `.ico` asset and stamps the EXE with the tray-derived application icon.
- `kakaotalk_adblock.spec` explicitly includes runtime modules (`kakao_adblocker.app`, `kakao_adblocker.config`, `kakao_adblocker.event_engine`, `kakao_adblocker.layout_engine`, `kakao_adblocker.logging_setup`, `kakao_adblocker.services`, `kakao_adblocker.ui`, `kakao_adblocker.win32_api`, `pystray`, `PIL`, `tkinter`) in `hiddenimports`.
- `kakaotalk_adblock.spec` also includes `kakao_adblocker.protocols` to keep typed runtime imports explicit in onefile packaging.
- `kakaotalk_adblock.spec` also includes `collect_submodules("pystray")` and `collect_submodules("PIL")` to avoid onefile runtime import misses.
- `kakaotalk_adblock.spec` also includes `collect_submodules("kakao_adblocker.app")`, `collect_submodules("kakao_adblocker.config")`, and `collect_submodules("kakao_adblocker.event_engine")` so packageized runtime internals are bundled in onefile builds.
- `kakaotalk_adblock.spec` includes package root `kakao_adblocker` so lazy exports remain importable in onefile builds and tooling paths.
- `kakaotalk_adblock.spec` excludes `pywinauto` and `comtypes` so archived legacy/UIA-only dependencies do not leak into the active v11 onefile bundle.
- single-instance mutex handling, dynamic Win32 text-result reads, and Startup Run command parsing are stdlib `ctypes` calls into kernel32/user32/shell32, so they do not require additional PyInstaller hidden imports.
- popup parity (`popup_ad_classes` / `AdFitWebView`), `SendMessageTimeoutW` close timeout, popup fallback restore tracking, popup host guards, and logging fallback/probe stay inside existing modules, so no extra hidden-import or hook change is required.
- the empty `EVA_ChildWindow` subtree custom-scroll guard remains tick-local inside `event_engine`, so current hidden-import coverage remains sufficient.
- `--self-check` / `--strict-self-check` exercise dynamic Tk diagnostics and logging bootstrap probe, so explicit `tkinter` hidden imports keep onefile packaging deterministic.
- `scripts/build_release.ps1` verifies `kakao_adblocker.config.VERSION` matches `packaging/windows_version_info.txt`, then runs a packaged `--self-check --strict-self-check --json` smoke by default after building with a temporary `%APPDATA%`; only `core` failures fail the build.
- when an interactive shell is available, `scripts/build_release.ps1` also runs a packaged startup smoke with `--startup-launch --minimized --startup-trace ... --exit-after-startup-ms ...`; the smoke is bounded by a 60-second timeout and kills the child process on timeout. Otherwise it records a skipped startup smoke and continues.
- `-StrictStartupSmoke` only upgrades tray-unavailable / tray-start-warning startup smoke results to a build failure when the interactive startup smoke actually ran.

## CI

- GitHub Actions workflow `.github/workflows/windows-ci.yml` runs on `push` and `pull_request` with hosted `windows-latest`.
- CI covers `python -m pyright`, `pytest -q --basetemp .pytest_tmp`, `python kakaotalk_layout_adblock_v11.py --self-check --json`, and `scripts/build_release.ps1 -NoSign`.
- Hosted CI runs the built EXE packaged strict self-check through the release script. Interactive tray/startup validation is still skipped by the release script's non-interactive/CI detection and remains a local/manual or release-host check.

## Legacy Archive

Legacy code/assets were moved under `legacy/`:

- `legacy/kakao_adblocker/legacy.py`
- `legacy/backup/*`
- `legacy/tools/*`
- `legacy/configs/*`
- `legacy/scripts/*`
- `legacy/카카오톡 광고제거 v10.0.py`
- archived legacy files stay outside the active repo-wide `pyright` scope; existing per-file directives remain for ad hoc maintenance
- CodeGraph broad queries can still surface `legacy/` symbols, so active v11 analysis should be narrowed to `kakao_adblocker/`, `tests/`, and `kakaotalk_layout_adblock_v11.py`

<!-- SPECKIT-AGENT-GUIDE:START -->

## Spec Kit / Spec-Driven Development (AI 에이전트 필독)

> 이 블록은 GitHub Spec Kit 활성화 및 기능 명세 작업 결과를 AI 에이전트가 바로 쓰도록 정리한 안내입니다.
> 수정 시 마커 주석을 유지하세요. 스크립트/후속 세션이 이 구간을 갱신합니다.

### 이 저장소 상태

- **프로젝트**: `kakaotalk-layout-adblocker`
- **Spec Kit 초기화**: `.specify/ 있음`
- **에이전트 스킬**: Grok=True, Claude=True, Codex/Agy(.agents)=True
- **활성 기능**: 아직 `specs/` 기능 명세 없음 — `.specify/` 만 준비된 상태

### 에이전트가 먼저 읽을 파일

1. `.specify/` 및 `.grok/skills` / `.claude/skills` / `.agents/skills` 의 `speckit-*`
2. 기능 작업 시작 시 `/speckit-specify` 로 `specs/00N-...` 생성

### 권장 워크플로 (스킬 / 슬래시 커맨드)

| 단계 | 커맨드 (Grok/Claude 등) | 산출 |
|------|-------------------------|------|
| 원칙 | `/speckit-constitution` | `.specify/memory/constitution.md` |
| 명세 | `/speckit-specify` | `specs/<id>/spec.md` |
| 계획 | `/speckit-plan` | `plan.md`, `research.md`, `data-model.md`, `contracts/`, `quickstart.md` |
| 작업 | `/speckit-tasks` | `tasks.md` |
| 구현 | `/speckit-implement` | 코드 (tasks 순서) |
| 갭점검 | `/speckit-converge` | `tasks.md` 에 Phase Convergence **append-only** |

- Codex skills 모드: `$speckit-specify` 형태일 수 있음
- 스킬 파일: `.grok/skills/speckit-*/SKILL.md`, `.claude/skills/speckit-*/SKILL.md`

### 작업 규칙 (에이전트)

1. **새 기능/큰 변경 전** 활성 `spec.md`·`tasks.md` 를 읽고, 없으면 specify→plan→tasks 순으로 만든다.
2. **구현은 tasks.md 체크리스트**를 따른다. 완료 시 `- [ ]` → `- [x]`.
3. **`/speckit-converge` 는 tasks.md 를 rewrite 하지 않는다** — 잔여 갭만 하단 Phase 로 append.
4. brownfield 프로젝트는 상당 기능이 이미 있을 수 있다. 중복 구현 전에 코드·`[x]` 태스크를 확인한다.
5. 웹/데스크톱 패리티 등 **out-of-scope Assumptions** 는 새 feature 로 분리하는 것을 선호한다.
6. 기본 integration 은 **grok** 이며, 동일 레포에 claude / codex / agy 스킬도 multi-install 되어 있을 수 있다.

### 관련 링크

- Spec Kit: https://github.com/github/spec-kit
- 로컬 CLI: `specify` (uv tool, 버전은 `specify version`)

<!-- SPECKIT-AGENT-GUIDE:END -->
