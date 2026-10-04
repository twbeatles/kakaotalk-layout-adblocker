# Project Audit

- **감사 일자:** 2026-10-04
- **대상:** KakaoTalk Layout AdBlocker **v11.1.5** (Rust 네이티브 단일화 직후) / branch `main` @ `0a76fa5`
- **방식:** 문서 정독 → CodeGraph 호출 흐름 추적 → 원문 열람 → `cargo test --workspace` 실행 → 반증 후 이슈 확정
- **코드 변경:** 없음. 본 문서만 작성했다.
- **이전 감사 처리:** 2026-09-24 감사본을 대체한다. 당시 지적 사항은 v11.1.5에 반영되어 있고(회귀 테스트 존재 확인), 본 감사는 그 위에서 현재 상태를 재감사했다.
- **조치 상태(2026-10-04):** 본 감사 지적 중 ISSUE-001·Gap-01·Gap-02·만료 파싱 로그를 같은 날 구현했다.
  종료 복원은 healthy-first 순서(`restore_all_final`)로 바꾸고 타임아웃 경고를 카운터 포함으로 강화했다.
  워커 감시 스레드가 예상 밖 종료를 1회 다이얼로그로 알린다(스냅샷 없는 재기동은 복원放棄라서 하지 않음).
  시작프로그램 토글 레지스트리 실패에 에러 다이얼로그를 추가했다.
  `update_settings` 테스트는 이미 존재함을 확인하고 추가하지 않았다.
  Gap-03(WM_CLOSE tick 예산)은 관측 근거 없이 엔진 타이밍을 바꾸는 것이라 미적용으로 남긴다.
  검증: `cargo fmt --check` 통과, `cargo clippy --all-targets --all-features -- -D warnings` 통과,
  `cargo test --workspace` 114 passed / 0 failed (신규 3건 포함).

## 1. Executive Summary

프로젝트는 전반적으로 **양호**하다. Rust 단일화 정리(`0a76fa5`) 이후에도 전체 테스트가 통과하고(`111 passed / 0 failed / 2 ignored`), 설정 self-heal·원자적 저장·복원 백오프·업데이트 서명 검증 같은 안정성 장치가 코드와 테스트 양쪽에 실재한다. 과장된 위험은 없다.

- **전체 위험도: 낮음.** Critical/High 이슈는 없다. Medium 1건, Low 3건이다.
- **가장 중요한 문제:**
  1. 종료 시 워커 join timeout이면 진행 중 복원이放棄되고 스냅샷이 함께 사라진다 (Medium/Likely).
  2. 워커 루프 자체가 죽으면 감지만 하고 자동 복구가 없어 사용자가 직접 재시작해야 한다 (Medium/Likely Gap).
  3. 시작프로그램 토글의 레지스트리 실패가 무음으로 무시된다 (Low/Confirmed Gap).
  4. 팝업 연속 `WM_CLOSE`(각 500ms 순차 대기)가 tick을 묶어 둘 수 있다 (Low/Confirmed Gap).
- **데이터 손상/유실 가능성: 사실상 없음.** 설정 파일은 원자적 교체+파손 백업+기본값 재생성, 업데이트는 서명 검증+백업+실패 시 이전 버전 재실행으로 보호된다.
- **가장 먼저 수정해야 할 영역:** 종료 경로의 복원 보장(`lib.rs` shutdown), 워커 루프 사망 감시, 트레이 토글 실패 피드백.

## 2. Project Understanding

- **목적:** 카카오톡 Windows 클라이언트의 광고 영역을 Win32 레이아웃 제어(hide/resize/`WM_CLOSE`)만으로 제거. `hosts`/DNS/메모리 패치 없음.
- **주요 entrypoint:**
  - `dist/KakaoTalkLayoutAdBlocker_v11.exe` (또는 `cargo run -p kakao-app --release`) → `kakao-app/src/main.rs` → `run_with_args` (`lib.rs:28`)
  - 진단 경로(단일 인스턴스 mutex 밖): `--self-check[--strict-self-check][--json]`, `--dump-tree[--series]`, `--shadow`, `--check-update`
  - 업데이트 헬퍼: `kakao-updater.exe` (별도 크레이트, 서명 검증 교체 담당)
- **핵심 모듈:** `kakao-core`(순수 평가: `evaluate::{inspect,apply,orchestrate}`, `rules`, `graph`), `kakao-win32`(Win32 래퍼·`tray/`, hook `startup`, `process`), `kakao-app`(`engine/{tick,worker,apply,restore,schedule,caches,flags}`, `config/`, `updater/`, `lib.rs` 컴포지션), `kakao-updater`(교체 실행).
- **데이터 저장:** `%APPDATA%\KakaoTalkAdBlockerLayout\`의 `layout_settings_v11.json`·`layout_rules_v11.json`(원자적 교체+self-heal), 회전 로그, `HKCU Run` 시작프로그램 등록. DB 없음.
- **외부 의존성:** `windows 0.61`(Win32), `ureq`(업데이트 HTTPS), `ed25519-dalek`(매니페스트 서명), `clap`·`serde`·`tracing`. 네트워크 사용은 업데이트 확인/다운로드뿐이다.
- **핵심 실행 흐름:**
  - 차단 루프: `spawn_worker → loop { PID 스캔 → WinEvent hook 유지 → guarded_tick( tick( build_graph → evaluate_graph_for_apply → apply_evaluation( snapshot → hide/resize/WM_CLOSE ) + restore_stale_hidden ) ) } → stopping 시 drain_restore_all`
  - 토글: `TrayCommand → update_settings(디스크 재독+1필드 변경+원자 저장) → flag 반영 (저장 실패 시 롤백)`
  - 업데이트: `CheckUpdate → check_for_update(서명·버전·URL·sha·크기·만료 검증) → 사용자 동의 → prepare_update(다운로드+sha 검증+staging) → stopping → join(복원) → launch_helper(교체+재실행, 실패 시 이전 버전 재실행)`
  - 종료: `stopping 저장 → join_with_timeout(3s) → (staged 있으면 헬퍼 실행) → exit`

## 3. Audit Coverage & Limitations

- **CodeGraph explore 4회:** entrypoint·엔진 플로우 / 설정 저장·업데이터 / apply·복원·워커·스케줄·시작프로그램·트레이 / 매니페스트 검증·교체·종료·토글. elided 구간은 원문 직접 열람으로 보완했다(`apply.rs:79-153`, `restore.rs:38-95`, `worker.rs:107-321`, `lib.rs:150-427`, `manifest.rs:30-138`, `kakao-updater/lib.rs:129-208`, `version.rs`).
- **실행한 테스트:** `cargo test --workspace` (이 Windows 실기에서 직접 실행) — 종료 코드 `0`, 총 **111 passed / 0 failed / 2 ignored**(무시 2건은 실데스크톱 패리티·perf 프로브의 `--ignored` 대상).
- **확인하지 못한 것:** 실제 카카오톡이 설치된 환경에서의 `--dump-tree`/차단 동작, 트레이 GUI 상호작용, 업데이트 네트워크 호출, 서명된 릴리스 빌드(`build_release.ps1`), 비Windows fail-fast 경로(CI는 Windows 전용).
- **CodeGraph 한계:** 일부 심볼 본문이 elided되어 파일 직접 열람으로 메웠다. 아래 근거에 인용된 행 번호는 감사 시점 원문 기준이다.
- **검토 후 기각한 후보(반증됨):** HWND 재사용 오복원 — `identity_matches`(hwnd+pid+class)로 가드되고 회귀 테스트(`hwnd_reuse_*`, `kakaotalk_restart_ignores_stale_snapshots`)가 있다. Windows `rename` 덮어쓰기 — `config_migration` 저장/로드 테스트가 이 Windows 실기에서 통과한다. 매니페스트 위조 — 서명이 payload 전체를 덮으므로 `expires_at` 파싱 실패 허용·legacy URL 허용은 악용 불가(자사 빌드 산출물만 서명 통과). 버전 비교 — 숫자 컴포넌트 비교가 정확하고 테스트가 있다. tick panic — `guarded_tick`+테스트가 있다. 덤프 동시 실행 덮어쓰기 — 파일명에 타임스탬프가 붙는다.

## 4. High-Risk Issues

### [ISSUE-001] 종료 join timeout 시 진행 중 복원이放棄되고 스냅샷이 소실된다

- **위치:** `rust/crates/kakao-app/src/lib.rs:389-402`, `engine/worker.rs:311-320`, `engine/restore.rs:14-36`
- **우선순위:** Medium
- **신뢰도:** Likely (코드 흐름상 확정적이나 hung 상태의 실기 재현은 못함)
- **문제:** 종료 시 `join_with_timeout(worker, 3s)`가 타임아웃이면 워커 스레드가 보유한 `EngineCaches`(복원 스냅샷 전체)가 스레드와 함께 버려진다. 복원 자체가 워커의 종료 구간(`drain_restore_all`)에서 수행되므로, 타임아웃 순간 복원 중이던 창은 숨겨진 채로 남는다.
- **발생 조건:** 종료(또는 업데이트 재시작) 순간 카카오톡 UI 스레드가 응답 없음(`IsHungAppWindow`) 상태여서 동기 `ShowWindow`/`SetWindowPos`가 3초 안에 돌아오지 않을 때. hung 가드는 복원을 "건너뛰고 다음에 재시도"하지만, 종료 시에는 다음이 없으므로 스냅샷이 소멸한다.
- **영향:** 우리 프로세스가 숨긴 광고창이 카카오톡 재시작 전까지 숨은 채로 남는다. 광고라면 무해하지만, `WM_CLOSE` 거부 팝업의 hide/zero-size fallback으로 숨겨진 정상 팝업이 오탐이었다면 해당 팝업도 복원 없이 남는다(카카오톡 재시작으로 해소).
- **근거:** `lib.rs:395-401` 타임아웃 분기는 경고만 남기고 종료한다. 스냅샷은 워커 스레드 소유라 join 실패 시 회수 경로가 없다. `restore_snapshot`의 hung 스킵(`restore.rs:42-48`)은 "나중에 재시도"를 전제로 하며 종료 시점과 양립하지 않는다.
- **반증 확인:** 무한 대기가 더 나쁘다는 점(보이지 않는 프로세스가 mutex를 점유)은 코드 주석과 3초 제한으로 이미 해결되어 있다. 즉 트레이드오프는 의도적이나, 타임아웃 후 "복원放棄를 사용자에게 알리지 않는다"는 점은 남아 있다.
- **호출/영향 범위:** `run_with_args` 종료 경로 → `spawn_worker` 스레드 → `drain_restore_all` → `restore_all`. 업데이트 재시작 경로(`launch_helper` 전)도 동일하게 영향을 받는다.
- **권장 수정 방향:** 타임아웃 시 (a) 트레이/로그에 "복원되지 않은 창 N개"를 명시하고, (b) 가능하면 hung이 아닌 창만이라도 메인 스레드에서 best-effort 복원을 시도한 뒤 종료한다. 스냅샷 소유권을 워커 종료 전에 메인으로 넘기는 구조가 가장 깔끔하다.
- **필요한 회귀 테스트:** hung fake(`is_hung_app_window=true`, 숨김 스냅샷 보유) 상태에서 `join_with_timeout` 타임아웃을 강제했을 때, 종료 경로가 복원放棄를 기록하고 프로세스가 3초 내에 종료됨을 검증한다.

> Critical/High에 해당하는 이슈는 없다. 위 1건이 본 감사의 유일한 Medium이다.

## 5. Potential Functional Gaps

- **[Gap-01] 워커 루프 사망 시 자동 복구 없음 — Likely Gap (Medium).** `WorkerExitGuard`(`worker.rs:27-37`)는 루프 패닉을 `last_error`에 기록만 한다. tick 패닉은 격리되지만 루프 자체가 죽으면 광고 차단이 멈춘 채로 트레이 메시지만 남고, 사용자가 직접 재시작해야 한다. 감시 스레드 또는 루프 재기동이 없다.
- **[Gap-02] 시작프로그램 토글의 레지스트리 실패가 무음 — Confirmed Gap (Low).** `set_run_command`/`delete_run_command`은 실패 시 `false`만 반환하고(`kakao-win32/src/startup.rs`), `TrayCommand::ToggleStartup`(`lib.rs:244-260`)은 `false`일 때 아무 피드백 없이 무시한다. 메뉴 체크 상태는 그대로라 인지는 가능하지만, 실패 원인을 알 수 없다.
- **[Gap-03] 팝업 연속 `WM_CLOSE`가 tick을 순차 대기 — Confirmed Gap (Low).** `apply.rs:80-104`는 close 대상마다 `send_message_timeout(500ms)`을 동기 호출한다. 대상이 여러 개면 tick이 최대 500ms×N 묶이며, 그 사이 복원·재스캔이 밀린다. 상한이 있어 손상은 없으나, 광고 팝업이 쏟아지는 순간 반응이 느려진다.
- **추정 (Low):** 주기적 자동 업데이트 확인이 없고 트레이 수동 메뉴만 있다. 오탐 발생 시 사용자가 덤프를 떠서 전달하는導線이 문서화되어 있지 않다(`--dump-tree` 존재는 README/CLAUDE에 있으나 신고 경로 언급 없음).

## 6. Documentation Mismatches

실질적인 불일치는 없다. 확인한 내용:

- `README.md`의 개발·빌드·테스트 절차(`cargo test/clippy/fmt`, `build_release.ps1 -NoSign` 산출물 목록)는 실제 스크립트·CI와 일치한다.
- `CLAUDE.md`의 Rust 런타임 동작 절(복원 백오프·BOM hook 범위·스케줄 수치)은 감사한 코드와 일치한다. Python 계약 인용부는 "로컬 전용·Git 미추적"으로 명시되어 있어 오해 소지가 없다.
- 참고: 감사 지시문에 언급된 `AGENTS.md` 파일은 존재하지 않는다(스킬은 `.agents/skills/`에 있음). 기능 문서가 아니므로 이슈로 올리지 않는다.
- 참고: 유지된 구 감사 기록(`PROJECT_AUDIT.md` 2026-09-24본의 `specs/002` 링크)은 본 문서로 대체되며, CHANGELOG의 역사 서술은 그대로 둔다.

## 7. Recommended Fix Plan

### Phase 1 — Immediate

- ISSUE-001: 종료 타임아웃 시 복원放棄 명시 + hung이 아닌 창의 best-effort 복원. 스냅샷 소유권을 워커 종료 전 메인으로 이전하는 구조를 검토한다.
- Gap-01: 워커 루프 사망 감시(heartbeat 또는 join 감시 스레드)와 자동 재기동 또는 "재시작 필요" 트레이 알림을 추가한다.

### Phase 2 — Stability

- Gap-02: 시작프로그램 토글 실패 시 경고 다이얼로그 또는 상태 문자열에 원인을 노출한다.
- Gap-03: `WM_CLOSE` 순차 대기를 tick 예산 안에 묶거나(예: tick당 close 상한), 전송 결과를 모아 tick 끝에 보고한다.
- `update_settings`에 단위 테스트를 추가한다(호출자 2곳 모두 무테스트 — 현재는 `config_migration`의 load/save 경로로만 간접 커버).

### Phase 3 — Structural

- 종료·복원·업데이트 재시작을 "복원 보장 파이프라인"으로 묶어 상태머신화한다(현재는 `lib.rs` 종료 구간에 순차 코드로 분산).
- 만료 파싱 실패 시 허용(`manifest.rs:115-127`)을 로그로 남겨 자사 빌드 이상을 조기에 감지한다(보안 이슈는 아님).
- 실데스크톱 `--ignored` 테스트(graph parity·perf probe)의 수동 실행 기록을 남기는 절차를 문서화한다.

실제 코드는 수정하지 않는다.

## 8. Test Recommendations

- **Unit — 종료 복원放棄 기록:** hung fake + 숨김 스냅샷 보유 상태에서 join 타임아웃을 강제하고, (a) 3초 내 종료, (b) 복원放棄 로그/카운터 기록을 단언한다.
- **Unit — `update_settings`:** 외부 편집된 JSON(유효 필드 1개 변경 + 잘못된 타입 1개)에 토글을 적용하고, 유효 필드 보존·잘못된 필드 기본값 유지·1개 필드만 변경됨을 단언한다.
- **Integration — 워커 사망 보고:** `guarded_tick` 연속 패닉이 `last_error`와 `consecutive_panics` 백오프를 거쳐 상태에 반영됨을 단언한다(일부는 기존 테스트 존재 — 루프 사망 `Drop` 가드의 발화 조건을 추가).
- **Integration — 업데이트 실패 시 이전 버전 재실행:** 기존 `kakao-updater` 테스트에 더해, 교체 실패 후 staged 파일이 삭제되고 이전 EXE가 원래 인자로 재실행됨을 단언한다(코드 주석의 계약 그대로).
- **End-to-End — 토글 롤백:** 설정 파일 쓰기 실패 주입 시 트레이 플래그가 바뀌지 않고 경고가 남음을 단언한다(`ToggleEnabled`/`ToggleAggressive`/`ToggleStartup` 3종).
- **Concurrency — 토글 연타:** 두 종류 토글을 연속 적용해도 각 변경이 유실되지 않음(디스크 재독+단일 필드 변경의 직렬성)을 단언한다.
- **Regression — owned popup 골든:** `owned_popup_legacy_ad.json` 회귀가 `GetParent`-owner 경로 변경에 깨짐을 유지한다(이미 존재 — 유지할 것).
- **Platform-specific — 비Windows fail-fast:** 종료 코드 `2` 단언을 Windows 전용 CI 바깥(예: Linux 잡 또는 단위 수준 `cfg` 테스트)으로 추가한다. 현재 CI는 Windows 전용이라 이 분기가 무테스트다.

## 9. Final Assessment

- **Functional Correctness: Good** — 광고 판정·적용·복원의 핵심 경로가 순수 함수+회귀 테스트로 고정되어 있고, 111개 테스트가 전부 통과한다.
- **Runtime Stability: Acceptable** — tick 격리·백오프·hung 가드가 있으나, 루프 사망 시 자동 복구가 없고 종료 timeout 뒤 복원이放棄된다.
- **Data Integrity: Good** — 설정 원자 저장+자복구, 업데이트 서명+백업+롤백. 손실 경로를 찾지 못했다.
- **Error Resilience: Acceptable** — 대부분 경로에 경고·폴백이 있으나, 토글 실패 무음과 종료 복원放棄 무통보가 남는다.
- **Cross-platform Robustness: Acceptable** — Windows 전용이 명시적 정책이고 fail-fast가 있으나, 비Windows 분기는 무테스트다.
- **Test Confidence: Good** — 핵심 엔진·복원·설정·업데이터에 회귀 테스트가 있고, CodeGraph 영향 범위와 일치한다. `update_settings` 직접 테스트와 실데스크톱 `--ignored` 항목이 빈틈이다.

**실제로 먼저 수정할 문제 3개:**

1. 종료 join timeout 시 복원放棄 기록 + best-effort 복원 (ISSUE-001).
2. 워커 루프 사망 시 자동 복구 또는 명확한 재시작 유도 (Gap-01).
3. 시작프로그램 토글 실패 피드백 (Gap-02).
