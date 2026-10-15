# Tasks - audit runtime fixes

## Setup
- [x] T001 Record specification and implementation decisions in specs/001-audit-runtime-fixes/spec.md and plan.md.

## US1 - prompt suppression
- [x] T002 [US1] Add pre-hide ordering/snapshot/control regression tests in rust/crates/kakao-app/tests/close_restore_transitions.rs.
- [x] T003 [US1] Suppress before bounded close, preserving guards, in rust/crates/kakao-app/src/engine/apply.rs.
- [x] T004 [US1] Add immediate event scan/follow-up burst and latency policy in engine/worker.rs and config/settings.rs.

## US2/US3 - recovery
- [x] T005 [US2] Test and implement OFF backoff/reset recovery in engine/control.rs and engine/worker.rs.
- [x] T006 [US3] Test and implement same-PID hook retry/runtime seam in engine/runtime.rs and engine/worker.rs.
- [x] T007 [US2] Wake sleeping/message-pumping worker from tray controls in engine/flags.rs and lib.rs.

## US4 - diagnostics
- [x] T008 [US4] Add privacy-safe timings and actual frame timestamps in engine/tick.rs, engine/worker.rs and dump_cmd.rs.

## US5 - updater
- [x] T009 [US5] Add readiness handshake and rollback coverage in kakao-updater and kakao-app.

## Validation / documentation
- [x] T010 Correct README.md, CLAUDE.md, docs/COMPATIBILITY.md and PROJECT_AUDIT.md; add a reproducible manual latency/platform checklist.
- [x] T011 Run workspace tests, fmt, clippy and review changed code/diff.
- [ ] T012 Measure actual KakaoTalk screen latency/cold-start/soak and Windows 10/ARM64/DPI matrix in a dedicated environment (manual, NOT RUN).

Dependencies: T002 -> T003; T005/T006 -> T004/T007; T008/T009 -> T010 -> T011. Manual T012 follows automated validation.

## Validation boundary

T012 is a manual evidence task, not an implementation blocker. No test account, new screen capture, Win10/ARM64/DPI matrix or soak session has been performed in this change. Automation and runbook are ready; do not report this task as complete from fixture results.

Additional correction: update_settings now overlays valid external fields onto the last runtime copy even when another field has an invalid type; config_migration covers this case.

## Final automated results — 2026-10-15

- Default workspace suite after the final opt-in policy: 172 passed / 0 failed / 2 ignored, serial offline build.
- Separate live release perf probe: 1 passed (read-only); Strong ad recognized by shadow with isolated APPDATA.
- fmt --check and clippy --all-targets --all-features -D warnings: pass.
- App/helper release build: pass. Final isolated strict self-check: exit 0/core ok/warnings 0.
- git diff --check: pass. Core classifier and fixtures unchanged.
- An MSVC internal linker failure during an earlier parallel test build was resolved by a serial retry. Incremental-cache hard-link warnings are environment diagnostics, not application lint failures.
- See docs/testing/AUDIT_FIX_VALIDATION.md for exact scope/performance and T012 for remaining manual evidence.
