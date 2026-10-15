# Runtime audit fixes — validation record

Date: 2026-10-15 (Asia/Seoul). Implementation released as v11.1.10, based on v11.1.9.

## Automated verification

- Workspace: **172 passed, 0 failed, 2 ignored**, `cargo test --workspace --locked --offline -j 1`.
- An earlier final build hit an MSVC LNK1000 / C0000005 internal linker failure before tests started. Serial retry completed successfully; no source workaround or dependency installation was needed.
- fmt/clippy: final results recorded in specs/001-audit-runtime-fixes/tasks.md.
- Core classification and golden fixtures are unchanged.
- Native close test uses only a test-owned off-screen window whose WndProc deliberately waits 300ms. It observes that the window was already hidden before close processing.
- Worker tests cover OFF automatic retry/reset/wake, same-PID hook recovery, and late text without another event. Updater tests use owned temporary executables/child processes, including failed readiness rollback.

## Live read-only verification

- Running KakaoTalk executable version: **26.8.2.5324**.
- Existing running blocker executable version: **11.1.8.0** (dist), preserved throughout verification.
- New debug executable with default rules: `--shadow`, separate temporary APPDATA, exit 0.
- Result: **main=1, candidates=2, one Strong/Hide ad candidate**. No Hide/Resize/Close is executed by shadow mode.
- Candidate belongs to KakaoTalk and has class EVA_Window_Dblclk. A separate read-only IsWindowVisible query confirms it is currently hidden by the existing blocker.
- Existing KakaoTalk and blocker processes remained alive. No live settings or startup registration was changed and no raw window-title dump was stored.

## Validation boundary

Shadow validates recognition of the current tree; it cannot prove the new apply order on real KakaoTalk or measure first-ad-frame latency while the old blocker is hiding the ad. The native test covers apply ordering in a controlled window instead.

Cold-start/warm screen p50/p95/max, new-mode CPU, Windows 10/ARM64/DPI matrix and one-hour soak remain **NOT RUN**. Follow LATENCY_VALIDATION.md in a dedicated session. A direct old/new comparison requires stopping one blocker and starting the other; two apply loops should not run concurrently.

## Live release read-only performance probe

`cargo test --release --locked --offline -j 1 -p kakao-app --test perf_probe -- --ignored --nocapture`: **1 passed** (separate from the default workspace suite).
300-round mean: desktop top-level 357, KakaoTalk top-level 34, nodes 67; EnumWindows pass **115.2 us**, graph **894.5 us**, pure evaluation **43.2 us**, Toolhelp PID scan **18035.6 us**.
These are collection/evaluation costs with the old blocker still running, not screen-visible latency or measured application CPU.
At this cost, 200ms PID polling alone could consume about 9% of one core (arithmetic estimate). Therefore low_latency_mode is **opt-in, default false**; immediate event reconciliation and hide-before-close improvements remain enabled by default.

Release app/helper build and isolated `--self-check --strict-self-check --json`: exit 0, version 11.1.9, core ok, one KakaoTalk process, zero core/info warnings. The final opt-in/default-false policy was rebuilt and strict-self-checked successfully before packaging; the published v11.1.10 binaries were rebuilt by the tag workflow.
