# Project Audit

- 감사 일자: **2026-10-15 (Asia/Seoul)**
- 대상: KakaoTalk Layout AdBlocker **11.1.9**, `main` / `e31afc9`
- 초점: 기능·런타임 안정성, 특히 **광고가 0.n초 보였다가 사라지는 현상**
- 변경 범위: 이 보고서만 갱신. 구현·설정·fixture·테스트 코드는 수정하지 않았다.
- 기존 2026-10-04 감사본을 대체한다. 당시 수정된 워커 중단 알림·시작프로그램 실패 알림·종료 복원 순서 개선을 미해결 버그로 다시 세지 않았다.

## 감사 후 조치 — 2026-10-15

아래 본문은 수정 전 e31afc9의 감사 기록이다. 현재 미해결 목록으로 읽지 않는다. [작업 목록](specs/001-audit-runtime-fixes/tasks.md)과 [실측 절차](docs/testing/LATENCY_VALIDATION.md)를 참고한다.

| 항목 | 조치 |
|---|---|
| ISSUE-001 | snapshot→hide/zero-size/resize→close. 건당 50ms/tick 총 100ms close 예산. fake 순서 및 300ms 실제 Win32 WndProc 검증. |
| ISSUE-002 | OFF 200ms~5초 retry·reset 즉시 예약·제어 wake-up. 게이지는 실제 결과로 갱신. |
| ISSUE-003 | 같은 PID 설치 실패도 1~32초 retry. degraded 트레이 표시/복구 시 해소. |
| GAP-01 | 제목 없는 DEBUG 단계별 trace, series elapsed_ms/capture_us. 기존 dump에는 제목이 있음. |
| GAP-02 | 유휴 후 첫 스캔 즉시·후속 cadence 병합/유한 burst. core 판정/golden 불변. |
| GAP-03 / GAP-06 | 선택 low_latency_mode=true: 부재 최소 200ms, 생존 중 추가 PID 최소 2초. false는 이전 절전 정책. |
| GAP-04 | 종료 상한 유지. 영속 HWND 복원은 재사용 위험 때문에 도입하지 않고 카카오톡 재시작 복구를 안내. |
| GAP-05 | 새 헬퍼의 60초 readiness/250ms 생존 확인, 실패 시 소유 child 회수 후 rollback. 구 헬퍼에는 미적용, EXE 두 개 함께 배치 필요. |
| 문서·설정 | README/CLAUDE/호환성의 보장·버전·스케줄 정정. 일부 타입 오류 옆 유효 외부 편집도 토글 저장 시 보존. |
| 남은 검증 | 실제 KakaoTalk p50/p95/max·CPU, Windows 10/ARM64/DPI·혼합 모니터·soak는 NOT RUN. 0프레임 달성을 주장하지 않음. |

최종 자동 검증 결과는 작업 목록에 기록한다. 릴리스 태그를 발행한 작업은 아니다.

## 1. Executive Summary

프로젝트는 광고 판정·적용·복원을 분리하고, 오탐 방지 가드·스냅샷·설정 원자 저장·서명 검증을 갖춘 Windows 전용 도구다. 정적 기능 검증은 양호하지만 **화면 노출 시간과 실제 워커 상태 전환 검증은 부족하다. 전체 위험도는 Medium / Needs Work**다. Critical/High에 해당하는 미반증 이슈는 찾지 못했다.

가장 중요한 발견은 다음과 같다.

1. **팝업 숨김보다 모든 WM_CLOSE 대기가 먼저 실행된다.** 느린 close 하나가 다른 광고의 hide/resize까지 늦춘다. ISSUE-001, Medium/Likely.
2. **차단 OFF 전환의 복원 실패는 OFF 유지 중 재시도되지 않는다.** 카카오톡이 회복해도 숨김·zero-size 상태가 남을 수 있다. ISSUE-002, Medium/Confirmed.
3. **훅 설치가 한 번 실패하면 동일 PID 집합에서는 재설치하지 않는다.** 일시 실패가 지속적인 폴링 지연으로 이어진다. ISSUE-003, Medium/Confirmed.
4. **0.n초 노출을 설명하는 정상 설계 지연도 있다.** 이벤트 처리 전 기본 20ms 병합, 약한 신호 2틱 확인, 카카오톡 부재 후 최대 2초 PID 탐색 간격이다. 이를 오탐 방지 버그로 분류하지 않았다. 사용자가 보는 현상의 직접 원인은 이번 감사에서 화면 실측하지 못했다.

카카오톡 대화·첨부파일·DB를 쓰는 경로는 없다. 해당 사용자 데이터의 손상·유실 경로는 확인되지 않았다. 다만 **일시적 UI 상태의 복구 실패**는 가능하며, 설정 self-heal 및 업데이트 교체의 전원 중단까지 완전 무손실이라고 보증할 수는 없다.

가장 먼저 손댈 영역은 **광고 이벤트→최초 hide 지연 계측과 적용 순서**, 이어서 **OFF 상태 복원 재시도**다. 하단 owned 배너와 독립 AdFit 팝업을 구분해야 한다. 팝업 close 지연을 모든 하단 배너의 원인으로 일반화해서는 안 된다.

## 2. Project Understanding

### 목적·아키텍처·실행

- 목적: Win32 창 hide/resize/WM_CLOSE만으로 카카오톡 광고 UI를 억제한다. hosts/DNS/패킷 차단이나 대상 프로세스 메모리 패치를 하지 않는다.
- 활성 구현: Rust workspace의 `kakao-app`, `kakao-core`, `kakao-win32`, `kakao-updater`. 로컬 참고용 `legacy/` Python 구현은 production 위험 판단에서 제외했다.
- 앱 entrypoint: `rust/crates/kakao-app/src/main.rs:6` → `run_with_args` (`src/lib.rs:28`). 배포 EXE는 `KakaoTalkLayoutAdBlocker_v11.exe`.
- 헬퍼 entrypoint: `rust/crates/kakao-updater/src/main.rs` → `update_executable_with`.
- 개발 실행: `cd rust; cargo run -p kakao-app --release`. 패키징은 `scripts/build_release.ps1`, CI는 `.github/workflows/windows-ci.yml`, `release.yml`.
- Windows 전용: `run_with_args`는 비Windows에서 코드 2로 종료한다. Linux/macOS 미지원은 명시된 제품 범위다.
- 개발 규칙: CLAUDE는 광고 판정 알고리즘을 고정 계약으로 취급하며 변경 시 실덤프·회귀 테스트·문서 근거를 요구한다. 루트 AGENTS.md 실파일은 없다. 대화로 제공된 CodeGraph 우선·main 유지 지시를 적용했다.

| 영역 | 핵심 역할 |
|---|---|
| `kakao-core/src/evaluate/`, `signals.rs`, `layout.rs`, `rules.rs` | 순수 그래프 판정, Strong/Weak 확인, 광고 가드, action 계획 |
| `kakao-app/src/graph_build.rs` | PID 필터 → 창 열거 → parent/owner·텍스트·rect·가시성 수집 |
| `kakao-app/src/engine/` | 워커 스케줄, 적용, 스냅샷, stale 복원, panic 격리 |
| `kakao-win32/src/real.rs`, `event_hook.rs`, `process.rs` | 실제 Win32 API, PID별 out-of-context 이벤트 훅, 프로세스 생존 핸들 |
| `kakao-win32/src/tray/`, 앱 `lib.rs` | 트레이 메시지 루프·명령 큐, 설정 저장 후 atomic flag 갱신 |
| 앱 `config/` | BOM 허용, typed overlay, 파손 백업/self-heal, 원자 저장·로그 회전 |
| 앱 `updater/`, `kakao-updater` | HTTPS, Ed25519·SHA-256·크기·URL·버전 검증, staging, 교체·롤백 |

영속 저장은 `%APPDATA%\KakaoTalkAdBlockerLayout\`의 `layout_settings_v11.json`, `layout_rules_v11.json`, `layout_adblock.log`와 현재 사용자 시작프로그램 레지스트리다. DB·migration·SQL은 없다. 설정 저장은 고유 임시파일→sync_all→rename이다. 복원용 `snapshots`, 판정용 `states`, 재시도용 `stale`은 워커 메모리에만 존재한다.

공유 상태는 SharedFlags의 atomic ON/OFF·공격 모드·종료·카운터 및 mutex 오류 문자열이다. 창 변이와 스냅샷은 워커가 소유한다. 일반 앱 중복 실행은 named mutex로 막고 진단 경로는 별도 실행한다. 주요 의존성은 `windows 0.62`, `ureq 3`, `serde`, `clap`, `crossbeam-channel`, `tracing`, `ed25519-dalek 3`, `sha2`다.

### 핵심 실행 흐름

```text
앱 실행 → CLI 검증 → 설정/rules 로드·경고 → 단일 인스턴스 확인
       → 시작프로그램 상태 동기화 → spawn_worker + 트레이

PID 탐색/생존 확인 → PID별 WinEvent 설치·메시지 펌프
       → 이벤트 병합 또는 주기 도래 → guarded_tick
       → build_graph → 메인/광고 판정·후보 streak 갱신
       → apply_evaluation: PID/identity/hung/종료 가드
       → close → snapshot/hide → zero-size/resize
       → stale 복원·실패 게이지 → 트레이 결과/로그

트레이 토글 → 최신 디스크 설정 재독 → 단일 필드 저장
       → atomic flag 반영 → 워커가 다음 반복에서 적용/복원

업데이트 → 단일 실행 가드 → HTTPS 매니페스트·서명 검증
       → 사용자 동의 → 다운로드·해시 검증·staging
       → 종료·최종 복원·join(3초 제한) → 헬퍼 실행
       → 부모 종료 대기 → 해시 재검증 → EXE 백업·교체 → 재실행/실패 롤백
```

### 광고가 잠깐 보이는 현상의 집중 분석

**화면 노출 시간은 다음 요소의 합이다.** 아래 수치는 기본 코드 스케줄이며 실측값이나 엄밀한 실시간 상한은 아니다. OS 스케줄링·Win32 호출·UI 생성 시간은 별도다.

`노출 → PID 인지 → 이벤트 전달/폴링 → 병합 → 시그니처 완성·확인 → 앞선 Win32 작업 → hide → 화면 반영`

| 상황 | 코드상 지연·근거 | 해석 |
|---|---|---|
| 감시 중인 PID에 새 광고 이벤트 | `worker.rs:275-287`: 기본 20ms pump_for 후 전체 tick | 이벤트가 도착해도 바로 hide하지 않는다. 첫 프레임 무노출 보장은 없다. |
| 카카오톡이 30초 이상 없었다가 실행 | `schedule.rs:16-19,35-48`: PID 스캔 최대 2초 간격, 미감지 동안 훅 없음 | 마지막 스캔 직후 창이 생기면 다음 스캔까지 노출 가능. 기본 200ms 설정보다 부재 백오프가 우선한다. |
| Strong exact legacy / 광고 토큰+하단 기하 | `signals.rs:51-57,75-76`: 첫 판정 확정 | 모든 광고가 2틱을 기다리는 것은 아니다. 기존 실측 문서의 owned 배너는 exact 경로다. |
| Weak substring / 하단 조건 밖 Chrome+토큰 | `signals.rs:231-245`: 기본 2틱 확인 | 일반 이벤트는 burst_left를 재설정하지 않는다(`worker.rs:189-194,227-231`). 후속 이벤트가 없으면 다음 active 재확인 약 50ms를 더 기다린다. 병합과 합쳐 약 70ms 수준의 스케줄 지연이 가능하다. |
| 텍스트·자식 시그니처가 늦게 생성 | build_graph는 매번 읽고 관측 신호로 판정 | 신호가 준비되기 전에는 의도적으로 숨기지 않는다. 광고 클래스 전체를 미리 숨기는 처방은 정상 웹뷰 오탐 위험이 있다. |
| 훅 정상·유휴 백오프 | 이벤트가 메시지 대기를 깨움 | idle_backoff_max_ms=1000이 정상 이벤트의 고정 지연이라는 판단은 틀리다. 누락/필터링된 이벤트의 폴링 복구만 늦어진다. |
| 훅 설치 실패 | 기본 유휴 폴링 200ms | ISSUE-003. 훅이 없으면 1초 idle 백오프를 하지 않는 보호는 있다. |
| 느린 독립 팝업 close | close 전체 처리 후 hide. 호출당 timeout 500ms | ISSUE-001. 같은 tick의 다른 배너 hide도 밀린다. owned legacy 배너 단독 경로에 close 대기가 필수인 것은 아니다. |
| 차단 OFF→ON | OFF 분기는 1초 sleep, 별도 깨우기 없음 | ON 요청 후 다음 반복까지 광고가 남을 수 있다. 즉시 반영 UX의 보완점이다. |

**중요한 반증:** `launch_ad_flash_banner.json`은 이미 완성된 정적 창 트리다. 첫 tick에 숨겨지는 테스트가 실제 UI 생성부터 hide까지의 시간을 검증하지는 않는다. `owned_popup_legacy_ad.json`도 Strong exact이므로 사용자 현상을 일괄적으로 weak_signal_confirm_ticks=2 탓으로 돌릴 근거는 없다.

우선 **광고 판정은 유지하면서 대기를 줄이는 것**을 검토한다. 실제 배너 타입과 SHOW→콜백→판정→hide를 측정하고, 이미 확정된 Strong 후보의 첫 처리 전 20ms 대기 축소를 평가한다. Weak는 2틱 안전장치를 유지한 채 첫 확인 후 짧은 재확인을 예약할 수 있다. 현재 layout-only·out-of-context 감시는 표시 전 차단을 보장하지 않으므로 “0프레임 노출” 보장과 “노출 시간 감소” 목표를 구분해야 한다.

## 3. Audit Coverage & Limitations

### 실제 확인한 범위

- README, CLAUDE, 기존 감사본, Rust README, 호환성·테스트 강화·실측 기록, 기본 설정, Cargo 설정, CI/release workflow, 개발·릴리스 스크립트.
- **CodeGraph MCP를 실제 사용했다.** 핵심 관계는 `run_with_args→spawn_worker→guarded_tick→tick→build_graph/evaluate_graph_for_apply→apply_evaluation/restore_stale_hidden`, `candidate_handles/legacy_hide_decision/update_candidate_state`, `EventHook::install_for_pids→hook_proc→drain/wait_message/pump_for`, PidWatch, 설정 저장 및 헬퍼 교체다.
- 적용 전 identity/PID/hung 가드, close/hide/set_pos 순서, 스냅샷 생명주기·OFF/stop, 설정 self-heal·원자 쓰기, 로그 회전, 트레이 명령 큐와 업데이트 실패 경로를 확인했다.
- CodeGraph가 생략한 워커 말미·앱 종료부·헬퍼 교체 본문·테스트 앞부분은 직접 열람했다. 문서/config 및 테스트 목록은 파일 열람·rg로 보완했다. 일부 테스트 이름 질의는 무관한 심볼을 반환해 직접 확인했으며, “caller 없음”을 테스트 부재의 증거로 쓰지 않았다.

### 이번에 실행한 검증

| 명령 (`rust/`에서 실행) | 결과 |
|---|---|
| `cargo test --workspace --locked --offline` | **157 passed / 0 failed / 2 ignored**, 종료 코드 0 |
| `cargo fmt --all -- --check` | 통과, 종료 코드 0 |
| `cargo clippy --workspace --all-targets --all-features --locked --offline -- -D warnings` | 통과, 종료 코드 0 |

순수 계약/oracle·golden·경계·퍼즈 불변식, fake Win32 전환, 설정/BOM/저장, 복원 실패·HWND 재사용·panic, 업데이트 파일 교체, 자체 생성 테스트 창 roundtrip을 실행했다. 실 Win32 워커 테스트는 apply=false이고 self-check 테스트는 임시 APPDATA를 사용한다. dependency 설치·네트워크 업데이트·실사용 카카오톡 창 변경은 하지 않았다.

미실행 ignored 2건: `single_pass_graph_matches_the_per_node_algorithm_on_the_live_desktop`, `reconciliation_cost_breakdown`. release 빌드·패키징 smoke·실제 업데이트·트레이 클릭 E2E도 이번에는 실행하지 않았다.

### 한계 및 반증

- 실제 화면 녹화·새 덤프·0.n초 노출 재현은 하지 않았다. 사용자 데이터가 포함될 수 있는 실창 덤프도 수집하지 않았다.
- `docs/testing/COMPATIBILITY_EVIDENCE.md`의 2026-10-12 실측은 참고 기록이다. owned 배너 ON/종료 복원 spot 확인이며 전체 UI·DPI·노출 시간 검증은 아니다.
- CodeGraph는 정적 관계이며 Win32 전달 시각·화면 합성·권한·동적 HWND 수명을 검증하지 않는다.
- Windows 10/ARM64/DPI별 실행, Linux/macOS 빌드, 실제 네트워크 장애·전원 중단·보안 제품 환경은 미검증이다.
- 타 PID 자식 변이 후보는 precheck가 실제 PID/identity를 재조회하는 점과 통과한 f20 테스트로 반증했다. HWND 재사용도 PID/class가 다른 경우 가드·회귀 테스트가 있다.
- OFF 중 새 hide는 enabled/apply 가드가 막는다. ISSUE-002는 새 차단이 아니라 실패 복원의 재시도 문제다.
- ShowWindow 반환값은 이전 가시성이라 false 자체를 실패로 지적하지 않았다.
- 메인 뷰 rect 미복원은 과거 화면좌표/부모좌표 혼용 회귀를 막는 의도적 정책이다. 문서 보장 범위와 구분했다.
- 정상 업데이트 경로는 서명·허용 URL·크기·버전·해시를 검사하고 헬퍼에 SHA-256을 전달한다. 임의 URL 실행·쉘 문자열 injection으로 오판하지 않았다.

## 4. High-Risk Issues

요청된 제목을 유지했지만 **Critical/High 이슈는 없다. 아래 3건은 모두 Medium이며 미반증 Confirmed/Likely만 수록했다.**

### [ISSUE-001] 느린 팝업 close가 최초 숨김과 같은 tick의 다른 광고 처리까지 지연시킨다

- **위치:** `rust/crates/kakao-app/src/engine/apply.rs:80-117` / apply_evaluation; `rust/crates/kakao-core/src/evaluate/apply.rs:225-240`.
- **우선순위:** Medium
- **신뢰도:** Likely — close→hide 순서는 코드로 확정. 실제 느린 카카오톡 WndProc의 화면 지연은 이번에 재현하지 않았다.
- **문제:** close 목록 전체를 순회한 뒤 hide를 처리한다. 팝업은 host와 matched descendant 모두 close/hide/zero-size 후보이므로 host가 살아남고 처리가 느리면 timeout이 누적된다. 광고 판정 완료 뒤에도 창이 표시된 상태다.
- **발생 조건:** 가드는 통과하지만 WM_CLOSE가 지연되는 창. 완전히 hung으로 감지되기 전의 느린 UI나 close 핸들러 지연도 해당한다.
- **영향:** 각 실제 timeout이 발생하면 hide 전 대기에 약 500ms씩 추가될 수 있다. 다른 배너 hide·메인 뷰 resize·후속 이벤트도 같은 워커에서 지연된다.
- **근거:** 앱 apply.rs:84의 동기 send_message_timeout(...,500)은 :105 hide 루프보다 앞선다. core dismiss_popup은 hide를 계획하지만 실행 순서는 바꾸지 못한다. RealWin32::send_message_timeout (`real.rs:210-230`)은 실제 SendMessageTimeoutW를 호출한다.
- **반증 확인:** PID/identity/hung 가드와 SMTO_ABORTIFHUNG·timeout은 무한 close 대기를 줄인다. 그러나 느린 창을 전부 예측하거나 timeout 전에 hide하지는 않는다. f04 timeout은 즉시 반환하는 fake이며 `close_restore_transitions.rs:223-225`도 이를 명시한다. 이 테스트 통과는 실제 지연을 반증하지 못한다.
- **호출/영향 범위:** CodeGraph의 spawn_worker→guarded_tick→tick→apply_evaluation, core apply_once→retain_popup→dismiss_popup. 독립 팝업과 같은 tick의 hide/set_pos 대상에 영향. legacy owned 배너 단독 실행에 항상 해당하지는 않는다.
- **권장 수정 방향:** 검증된 대상의 원래 상태를 먼저 확보하고 빠른 은닉과 느린 close를 분리한다. close 총예산·host/descendant 중복 요청·종료 반응도 설계한다. hide 선행 시 부모 은닉 뒤 자식의 원래 가시성을 잘못 캡처하지 않도록 snapshot을 선행해야 한다. 정상 UI 가드는 유지한다.
- **필요한 회귀 테스트:** 한 광고 close가 300ms 지연될 때 다른 광고 hide가 대기 전에 실행되는지 확인. host/child 모두 timeout, 즉시 소멸, 거부, delivery failure, stop을 주입하고 snapshot·집계·복원까지 검증. fake 순서와 자체 테스트 창의 실제 대기는 분리한다.

### [ISSUE-002] OFF에서 복원이 한 번 실패하면 회복해도 OFF 유지 중 재시도하지 않는다

- **위치:** `rust/crates/kakao-app/src/engine/worker.rs:148-170`; `engine/caches.rs:38-53`; `engine/restore.rs:15-32`.
- **우선순위:** Medium
- **신뢰도:** Confirmed — production의 도달 가능한 상태 전환으로 확인. 실사용 카카오톡 hung 재현은 하지 않았다.
- **문제:** 복원은 was_enabled && !enabled_now 전환에서 한 번 실행된다. 실패 snapshot을 보존한 뒤 was_enabled=false가 되고 이후 OFF 반복은 1초 sleep/continue만 한다. 복원 실패 초기화도 bookkeeping만 지우며 OFF 복원을 호출하지 않는다.
- **발생 조건:** ON에서 숨긴/zero-size한 창이 OFF 순간 hung이거나 복원 API 실패 후 응답 상태로 회복한다.
- **영향:** 차단을 껐는데도 숨김/zero-size 상태가 남는다. OFF 중 복원 실패 초기화를 눌러도 복구되지 않고 실패 표시만 0이 될 수 있다. 별도 종료 복원 등 추가 경로가 필요하다.
- **근거:** restore_pending은 실패 snapshot을 다시 저장한다. 그러나 worker.rs:159-170 OFF 유지 중 restore 호출이 없다. 일반 stale 재시도는 tick의 apply 경로에 있어 OFF 분기에서 도달하지 못한다.
- **반증 확인:** snapshot 보존·ON stale backoff·종료 drain_restore_all_final은 존재하지만 OFF 자동 회복을 보장하지 않는다. failed_restore_keeps_snapshot_and_retries는 테스트가 drain_restore_all을 직접 두 번 호출한다. f03도 수동 drain으로 OFF를 흉내 내므로 워커 반복의 빈틈을 검증하지 못한다.
- **호출/영향 범위:** ToggleEnabled→update_settings→flags.enabled=false→spawn_worker OFF 전환→drain_restore_all→restore_all→restore_pending. legacy/aggressive hide 및 popup fallback snapshot에 영향.
- **권장 수정 방향:** OFF에서도 새 차단은 중지하되 pending snapshot 복원만 제한된 주기/백오프로 재시도한다. reset은 OFF에서도 복원을 예약하고 실제 pending 결과로 게이지를 갱신한다.
- **필요한 회귀 테스트:** ON hide→OFF 첫 복원 실패→응답 회복→OFF 유지 순서를 실제 워커 제어 흐름에서 실행. 새 hide/close 없이 기존 창만 복원되고 snapshot·오류·게이지가 해소되는지 확인. OFF 중 reset도 검증한다.

### [ISSUE-003] 훅의 일시적 설치 실패가 동일 PID에서는 영구 폴링 상태로 남는다

- **위치:** `rust/crates/kakao-app/src/engine/worker.rs:200-210`; `kakao-win32/src/event_hook.rs:73-120`.
- **우선순위:** Medium
- **신뢰도:** Confirmed — 실패 이후 재시도 조건 누락은 코드로 확인. 실제 OS 설치 실패를 주입하지 않았다.
- **문제:** install_for_pids가 None이어도 hooked_pids=pid_set으로 기록한다. 다음 설치 조건은 pid_set != hooked_pids뿐이라 동일 프로세스를 계속 감시하면 재시도하지 않는다.
- **발생 조건:** 초기/PID 변경 시 훅 API가 일시 실패한 뒤 환경이 회복. OS가 계속 실패하는 경우를 해결할 수 있다는 주장은 아니다.
- **영향:** 앱/PID 재시작 또는 OFF→ON 전까지 이벤트 처리가 복구되지 않는다. 기본 유휴 폴링 약 200ms와 Weak 확인 지연이 계속 남는다. 트레이에 훅 상태도 없어 성능 저하를 알기 어렵다.
- **근거:** 실패 warn 뒤 성공 여부와 무관하게 PID 집합을 갱신한다. 별도 retry clock이 없다.
- **반증 확인:** 훅이 없으면 idle backoff를 끄는 reconcile_interval(...,false)이 있어 차단 전체가 멈추지는 않는다. PID 변경·OFF→ON도 재설치한다. 그러나 동일 PID의 일시 실패 회복은 없다. 기존 훅 재설치 테스트는 직접 install/drop/install이며 워커 실패 스케줄은 커버하지 않는다.
- **호출/영향 범위:** spawn_worker→EventHook::install_for_pids 실패→reconcile_interval 폴링 fallback→모든 광고 탐지·숨김 지연. 저장 데이터에는 직접 영향이 없다.
- **권장 수정 방향:** hook.is_none()일 때 제한된 backoff 재설치를 수행하고 성공 집합과 시도 집합을 구분한다. 로그 폭주를 막고 degraded 진단을 노출한다.
- **필요한 회귀 테스트:** 같은 PID에서 첫 두 번 실패·세 번째 성공 주입. 자동 복구, OFF 중 재시도 중단, 중복 설치 방지, hook/event sender 수명 확인.

## 5. Potential Functional Gaps

- **GAP-01 / Confirmed Gap — 노출 지연 계측 부족.** WinEvent.time은 수집하지만 워커의 latency 분석에 사용하지 않는다. dump.rs:14-20 timestamp는 초 단위 epoch 문자열이며 series에도 frame별 실제 경과 ms가 없다. 이벤트 수신·판정·최초 hide·tick 소요·대상 분류를 monotonic clock으로 기록해야 20/50/200ms 원인을 구분할 수 있다. 제목/대화 내용은 기록할 필요 없다. 화면 노출 프레임은 영상으로 별도 측정한다.
- **GAP-02 / Confirmed Gap — 일반 이벤트 뒤 제한된 후속 burst 없음.** burst_scan_iterations는 PID 집합 변경 때만 활성화된다. 일반 광고 이벤트 후 시그니처가 늦게 완성되거나 Weak 확인이 필요할 때 짧은 재확인을 예약하지 않는다. 기능 누락보다 지연 개선점이다. Strong 첫 처리 전 병합 축소와 함께 실측 비교하되 판정 가드는 삭제하지 않는다.
- **GAP-03 / Confirmed Gap — cold-start와 OFF→ON 깨우기 정책.** 부재 PID 백오프와 OFF 1초 sleep은 전력 절감 의도와 맞지만 즉시 차단 요구와 충돌한다. 선택적 저지연 모드·프로세스 시작 감지·명령 wake-up을 검토할 수 있다. poll_interval_ms만 낮춰도 해결되지 않으며 활성 최소 50ms·유휴 최소 200ms·PID 최소 200ms 제한도 있다.
- **GAP-04 / Confirmed Gap — 종료 후 복원 snapshot 부재.** 강제 종료·최종 복원 실패·3초 join timeout 뒤 snapshot이 유지되지 않는다. healthy-first 및 경고는 이미 구현됐지만 재시작 후 복구는 없다. 광고 UI 상태 잔존 위험이며 대화 데이터 손실로 확대 해석하지 않는다. 종료 시간 제한을 유지하면서 복구 안내·안전한 복구 전략을 검토한다.
- **GAP-05 / Confirmed Gap — 업데이트 정상 기동 handshake 없음.** update_executable_with:239-259는 spawn 성공 후 백업을 지운다. 새 프로세스가 바로 실패하는 경우는 rollback 판단 밖이다. 현재 보장 범위는 교체/spawn 실패다. enhancement로 분류하며 현재 릴리스의 실제 기동 실패를 주장하지 않는다.
- **GAP-06 / 추정 — 추가 KakaoTalk PID 탐지 지연.** 기존 PID가 모두 살아 있으면 전체 재탐색은 30초마다다. 별도 renderer/새 인스턴스가 top-level 광고를 소유하면 늦게 감지할 수 있다. 기존 fixture의 다른 PID renderer 자식은 host hide로 가려지므로 이를 현재 버그로 확정할 수 없다. 실제 프로세스·창 구조 증거가 필요하다.

기본값에서 tokenless 웹뷰를 숨기지 않거나 제목 있는 미허용 팝업을 닫지 않는 것은 정상 기능 보호 계약이다. 광고 누락 버그로 세지 않았다.

## 6. Documentation Mismatches

| 설명 | 실제 구현·정정 필요점 |
|---|---|
| README의 광고가 즉시 사라짐·정상 기능 100% 보장 | 20ms 병합·PID 탐색·Weak 확인·느린 API 호출이 있다. 무노출·전 환경 보장 근거는 없다. 지원 범위와 지연 조건을 명시해야 한다. |
| README의 모든 조정 창 스냅샷·0.1초 완전 복원 | 메인 뷰 resize는 의도적으로 snapshot하지 않는다(apply.rs:129-135, normal_main_view_resize_not_saved_in_restore_snapshots). OFF 반영도 워커 스케줄에 좌우되며 실패/timeout이 있다. 숨김/zero-size 대상 best-effort 복원과 메인 뷰 자체 재레이아웃을 구분해야 한다. |
| README의 복원 실패 재시도 | ON stale에는 적용되나 OFF 유지에는 미적용(ISSUE-002). 종료 후 영속 retry도 없다. |
| README 배지·CLAUDE 개요 버전 11.1.5 | Cargo 및 config/paths.rs는 11.1.9다. 실행 버전·Windows 리소스 동기화 테스트는 통과하지만 문구는 낡았다. |
| docs/COMPATIBILITY.md의 부재 200ms sleep·공격 OFF 즉시 복원·백업 날짜 형식 | 실제 부재는 1~2초 백오프, aggressive 복원은 2 miss tick, Rust 백업은 epoch 형식. CLAUDE의 Rust 전용 설명이 정확하다. |
| docs/COMPATIBILITY.md의 광범위 PASS | fixture/함수 검증과 실제 플랫폼 E2E를 구분해야 한다. spot 기록만으로 Windows 10/ARM64/DPI별 PASS를 이번에 재확인할 수 없다. |
| f04 주석의 500ms×N 지연 테스트 예산 설명 | fake timeout은 즉시 반환한다. 테스트 통과는 production 지연 상한의 증거가 아니다. |

CLAUDE가 Python 계약과 Rust 차이를 명시한 부분은 불일치로 다시 지적하지 않았다. AGENTS.md 실파일 부재도 런타임 결함으로 세지 않았다.

## 7. Recommended Fix Plan

### Phase 1 — Immediate

Critical/High 데이터 손상·보안 사고 대응이 필요한 발견은 없다. 사용자의 광고 노출 문제를 우선한다.

1. **GAP-01 계측:** owned 배너/독립 팝업, cold-start/탭 복귀/재활성화를 나눠 SHOW→hide 및 화면 노출 p50/p95/max 기록. 원인별 지연을 분리한다.
2. **ISSUE-001 적용 순서:** snapshot·가드 유지하에 느린 close가 최초 hide를 막지 않게 한다. timeout 숫자를 무작정 낮추는 것보다 은닉과 close 예산 분리가 우선이다.
3. **ISSUE-002 OFF retry:** OFF 유지 중 pending restore 회복을 보장하고 reset 뒤 잘못된 0건 표시도 고친다.

### Phase 2 — Stability

- ISSUE-003: 동일 PID 훅 재설치 backoff·degraded 진단.
- GAP-02/03: 실측에 따라 Strong 첫 대기 축소, Weak 짧은 확인 burst, 명령 wake-up, 부재 탐색 저지연 옵션 비교. token·클래스·host 가드는 유지한다.
- 종료/복원 실패의 복구 안내와 상태 일관성 보완. 현재 3초 종료 상한은 유지한다.
- README/호환성을 실제 보장 범위로 정정하고 fixture PASS와 실기 PASS를 구분한다.
- 네트워크 timeout(연결 30초·수신 60초·총 90초)은 이미 있다. 자동 retry 부재를 바로 버그로 보지 않으며 필요 시 제한된 재시도 UX를 검토한다.

### Phase 3 — Structural

- 프로세스 탐색·clock·hook 설치·wake-up을 주입 가능하게 만들어 실제 worker 분기를 결정적으로 검증한다. 순수 tick 테스트는 유지한다.
- latency trace와 화면 측정을 결합해 릴리스 지연 회귀 기준을 만든다. fixed sleep 기반 flaky 테스트로 대체하지 않는다.
- 업데이트 기동 handshake·복구 정책은 별도 범위로 설계한다. 설정 저장/파손 복구는 현재 원자 쓰기를 유지한다.

위 계획은 권고이며 이번 작업에서 구현하지 않았다.

## 8. Test Recommendations

| 유형 | 입력/장애 조건 | 기대 결과 |
|---|---|---|
| Unit / ISSUE-001 | host/child close 지연·timeout, 별도 Strong 배너 동시 존재 | snapshot이 hide 전에 확보되고 첫 hide가 close 완료를 기다리지 않음. 소멸 HWND 후속 변이 없음. |
| Integration / ISSUE-002 | worker ON hide→OFF 첫 복원 실패→응답 회복, OFF 유지 | retry 기한 안에 원래 가시성/크기 복원. 새 hide/close 없음. snapshot·오류·게이지 해소. |
| Regression / ISSUE-002 | 위 상태에서 OFF 중 복원 실패 초기화 | 게이지 초기화뿐 아니라 retry 예약. 계속 실패하면 실제 실패 상태 재노출. |
| Unit / ISSUE-003 | 동일 PID 설치 결과 None, None, Some | backoff 뒤 회복, OFF 중 설치 중단, 성공 후 중복 설치 없음. |
| Integration / GAP-02 | 첫 SHOW에 신호 없음→15/40ms 뒤 자식/텍스트 형성, 후속 이벤트 없는 경우 포함 | Strong은 첫 유효 tick, Weak는 서로 다른 2 tick 뒤 처리. 정상 초기화 웹뷰는 미변이. |
| Regression / 판정 | owned exact, substring-only, tokenless Chrome, 제목 있는 viewer, unknown title, foreign renderer | 기존 oracle 기대값 유지. 지연 단축으로 확인/host/PID 가드가 사라지지 않음. |
| Concurrency | 제어 가능한 느린 Win32 호출 중 OFF/stop | 남은 작업·종료 순서 확인. OFF 뒤 새 차단 시작 금지, snapshot 보존, shutdown 상한 유지. |
| Concurrency / 파일 | 진단 bootstrap과 트레이 저장을 다른 프로세스에서 동시 실행 | JSON 파싱 가능·부분 파일 없음. 완료 설정이 기본값으로 되돌아가지 않음. 외부 편집 보존 범위 명시. |
| End-to-End / cold | blocker 60초 대기→카카오톡 시작, 반대 시작 순서도 각 30회 이상 | PID 감지·최초 hide·노출 프레임을 각각 기록. 변경 전후 p50/p95/max·CPU 비교, 탐색과 UI 지연 분리. |
| End-to-End / warm | 30초 idle 뒤 배너·탭 전환·잠금 해제·최소화 복귀·OFF→ON | 정상 이벤트가 1초 poll까지 대기하지 않음. 제어 wake-up/최초 hide 목표 충족. 정상 채팅·뷰어 미변이. |
| Platform-specific / Win32 | 자체 테스트 창 WndProc 300ms/timeout 지연, 부모·자식·owned 배치 | fake와 실제 호출 순서·지연·취소·snapshot 검증. 실사용 카카오톡 창은 사용하지 않음. |
| Platform-specific / 화면 | Windows 10/11 x64, 가능 시 ARM64, 100/125/150% DPI·혼합 DPI 모니터 | 경계·뷰 크기·포커스·OFF/종료 복원 확인. 미실행 조합은 NOT RUN. |
| Integration / 업데이트 | 해시 불일치, 교체 잠금, cross-volume copy 실패, spawn 실패, spawn 직후 종료 | 파일 보존/rollback·staging 정리·인자 재실행 확인. 마지막 조건은 현재 handshake 부재와 분리해 새 계약 설계. |
| Regression / 설정 | BOM·부분 타입 오류·파손 JSON·읽기/쓰기 거부 | 정상 필드·백업·warning 확인. 실패 토글은 동작/메뉴/디스크 일치. 전용 임시 APPDATA 사용. |

광고 노출 시간은 dump의 interval_ms만으로 계산하지 않는다. frame 실제 경과 ms와 이벤트/적용 trace를 확보한다. 화면은 120fps 이상 녹화 등으로 첫 광고 프레임부터 사라진 프레임까지 세고 측정 오차도 기록한다. warm Strong p95 50ms 같은 값은 **개선 실험의 목표 후보**로 합의할 수 있으나 현재 달성치·보장치가 아니다.

## 9. Final Assessment

| 항목 | 평가 | 근거 |
|---|---|---|
| Functional Correctness | Acceptable | 판정·가드·골든은 통과하나 OFF 복원과 훅 회복에 빈틈. |
| Runtime Stability | Acceptable | panic 격리·hung guard·종료 상한·중단 알림 존재. 동기 close 지연 가능. |
| Data Integrity | Good | 대화/DB 비접근, 원자 저장·백업, 서명/해시/교체 백업. 중단 시 UI snapshot 유실은 별도 제한. |
| Error Resilience | Needs Work | OFF 유지 및 일시 hook 실패에서 자동 회복 단절. |
| Cross-platform Robustness | Acceptable | Windows 전용·비Windows fail-fast 명확. Windows 버전/DPI/ARM64 전체 실증 부족. |
| Test Confidence | Acceptable | 157 passed, fmt/clippy 통과. 실 worker 분기·실제 timeout·최초 노출 시간은 suite가 보장하지 않음. |

**실제로 먼저 수정할 문제 3개:**

1. **최초 은닉의 불필요한 대기:** 계측 후 ISSUE-001 close-before-hide 수정. owned 배너는 20ms 병합·후속 확인 정책을 별도 측정해 조정.
2. **OFF 복원 retry 누락:** ISSUE-002를 고쳐 일시 실패 뒤에도 차단 OFF의 원상 복구 완료.
3. **동일 PID 훅 설치 실패 미복구:** ISSUE-003에 제한된 재설치·진단을 추가해 지속 폴링 지연 감소.
