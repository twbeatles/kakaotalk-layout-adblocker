# Project Audit

- 감사 일자: **2026-10-22 (Asia/Seoul)**
- 대상: KakaoTalk Layout AdBlocker **11.1.12**, `main`, 커밋 `9e33a55`
- 관점: 기능 구현, 실제 배포 경로, 런타임 안정성, 상태 복원 및 업데이트 실패 복구
- 초기 감사 범위: 보고서만 갱신했다. 이후 사용자 요청으로 수행한 소스·테스트·문서 수정은 아래 후속 조치에 별도로 기록한다. production 데이터는 변경하지 않았다.
- 기존 2026-10-15 보고서를 현재 코드 기준으로 대체한다. 과거 보고서에서 이미 조치된 close 순서, OFF 복원 재시도, 동일 PID 훅 재설치 문제는 미해결 이슈로 재등록하지 않았다. 과거 보고서는 Git 이력으로 확인할 수 있다.

## 후속 조치 — 2026-10-23

사용자의 전체 개선 요청에 따라 main에서 구현했다. **아래 1~9절은 `9e33a55` 감사 당시의 분석 기록**이며, 현재 미해결 목록으로 읽지 않는다. 현재 조치와 검증 근거는 [후속 검증 기록](docs/testing/AUDIT_FOLLOWUP_VALIDATION.md) 및 [작업 목록](specs/001-audit-runtime-fixes/tasks.md)에 있다.

| 항목 | 현재 조치 |
|---|---|
| ISSUE-001 | 설치 폴더에서 새 EXE·백업을 sync/검증 후 rename. 설치 경로 직접 copy·선삭제 제거. rollback도 같은 방식, Windows 이미지 잠금 최대 2초 retry. companion helper 명시적 복구와 protocol 2 검사 추가. 준비/commit 경계 중단·공유 잠금·readiness 실패 회귀 통과. 실제 다른 볼륨/전원 차단은 미검증. |
| ISSUE-002 | TaskbarCreated 실패 시 항상 retry timer 예약. 테스트 전용 Win32 창에서 실제 WM_TIMER로 복구 확인. 아이콘 없는 정상 종료 요청 `--exit` 추가. 실제 Explorer 재시작 실기는 미실행. |
| ISSUE-003 | child zero-size에 SWP_NOMOVE 적용. OFF/종료/stale 및 실제 중첩 Win32 child의 non-zero offset 복원 테스트 통과. core 판정/action/golden fixture 불변. |
| 설정·오류 | 프로세스 간 설정 잠금, bootstrap create_new, 고유 파손 백업·backup 실패 시 원본 보존, 토글/역롤백/shell 실패 사용자 표시. startup 역롤백은 원래 command를 보존. |
| 네트워크·helper | 유한한 일시 오류 retry·manifest 수신 제한. 구 helper는 종료 전 거절하고 두 EXE 동시 ZIP 설치를 안내. helper 자체 자동 다운로드는 추가하지 않음. |
| 문서·구조·CI | transaction 모듈과 오류 주입 경계, README/CLAUDE/CHANGELOG 정정, Windows CI가 실제 build_release.ps1 -NoSign/strict smoke를 호출하도록 변경. `v11.1.13` 릴리스 workflow는 테스트·빌드·매니페스트 서명·업로드·검증까지 성공. |

최종 자동 검증: **Cargo suite 196 passed / 0 failed / 2 ignored**, fmt/clippy 통과, 앱·helper 릴리스 빌드 통과, 격리 APPDATA의 packaged strict self-check `exit 0 / core ok / warnings []`, 릴리스 helper protocol/복구 검사 통과. 자식 프로세스가 중복 출력한 테스트 요약은 Cargo suite 집계에서 제외했다.

남은 실기 항목은 T022에 NOT RUN으로 유지한다. Windows/아키텍처/DPI 행렬·실제 카카오톡 사용자 흐름·soak·다른 볼륨 중단/전원 장애는 전용 환경이 필요하다. 영속 HWND 복원은 재사용 위험 때문에 도입하지 않았으며 카카오톡 재시작 복구 제한을 명시했다. 초기 감사의 세 문제는 구현·자동 회귀 범위에서 조치했지만, 모든 플랫폼의 실기 검증이나 무손실 전원 장애 복구가 완료됐다는 뜻은 아니다.

## 1. Executive Summary

프로젝트는 순수 광고 판정, Win32 적용, 워커 상태 관리, 업데이트 헬퍼를 분리하고 있으며, 오탐 방지 가드와 복원 재시도도 갖추고 있다. 핵심 기능이 전반적으로 미구현된 상태는 아니다. 다만 **예외 상황의 복구에는 보완이 필요하다. 전체 평가는 Needs Work, 위험도는 Medium이며 업데이트 중단 경로에는 국소적인 High 위험이 있다.**

반증 후 남긴 이슈는 **3건: High 1건, Medium 2건**이다.

| 우선 순서 | 문제 | 심각도 / 신뢰도 |
|---|---|---|
| 1 | 업데이트 교체 도중 중단되면 정상 EXE로 자동 복귀할 경로가 부족하고, 다른 볼륨에서는 설치 경로에 직접 복사한다 | High / Likely |
| 2 | 정상 등록 후 Explorer 재시작에서 아이콘 재등록이 한 번 실패하면 재시도 타이머가 예약되지 않는다 | Medium / Confirmed |
| 3 | 팝업 자식 창을 `(0,0)`으로 옮겨 zero-size 처리한 후 복원 시 원래 자식 좌표를 되돌리지 않는다 | Medium / Confirmed |

**데이터 손상/유실:** 확인한 활성 경로는 카카오톡 대화 DB·첨부파일을 저장하거나 수정하지 않는다. 해당 사용자 데이터의 손상·유실 경로는 발견하지 못했다. ISSUE-001은 차단기 설치 EXE의 부재/불완전 교체 위험이고, ISSUE-003은 일시적 UI 배치 손상이다. 설정 저장에는 임시파일·`sync_all`·rename 보호가 있지만 전원 장애와 다중 프로세스 변경까지 완전 무손실이라고 보증하지는 않는다.

가장 먼저 **업데이트 설치 트랜잭션과 중단 복구 진입점**, 다음으로 **트레이 재등록 실패의 재시도**, **팝업 자식 좌표 복원**을 수정해야 한다. 광고 판정 규칙을 완화할 필요는 없다.

## 2. Project Understanding

### 목적과 개발 규칙

Windows PC 카카오톡의 광고 창을 숨기거나 닫고 메인 뷰를 리사이즈하는 layout-only 도구다. hosts/DNS 변경, 패킷 차단, 대상 프로세스 메모리 패치 방식은 사용하지 않는다. 업데이트 확인/다운로드에는 별도 HTTPS 통신이 있다.

- 활성 구현: `rust/`의 4개 crate. 저장소 이름에 `py`가 있어도 배포 대상은 Rust다.
- Windows 전용: `run_with_args`가 비Windows에서 종료 코드 `2`로 거절한다. Linux/macOS 앱 미지원은 제품 범위이며 버그로 분류하지 않았다.
- `CLAUDE.md`는 광고 판정 알고리즘을 고정 계약으로 취급하고, 변경 시 실덤프·fixture·회귀 테스트 근거를 요구한다.
- 루트에 **AGENTS.md 실파일은 없었다**. 대화로 제공된 CodeGraph 우선 사용 및 `main` 작업 지시를 적용했다. 별도 브랜치는 만들지 않았다.
- 로컬 전용 `legacy/` Python 구현 및 미추적 Python 테스트는 현재 배포 위험 판단에서 제외했다. CLAUDE의 Python 계약 설명과 Rust 전용 설명을 구분했다.

### 주요 entrypoint와 모듈

| 경로 | 역할 |
|---|---|
| `rust/crates/kakao-app/src/main.rs:6` → `lib.rs:28` | CLI 파싱, 진단 분기, 설정 로드, 단일 인스턴스, 워커·트레이 구성 |
| `kakao-app/src/engine/worker.rs` | PID 탐색, 훅 복구, 이벤트/폴링 스케줄, tick panic 격리, 종료 복원 |
| `kakao-app/src/graph_build.rs` | PID 범위의 top-level/descendant 열거, parent/owner, 텍스트·geometry 수집 |
| `kakao-core/src/evaluate/`, `signals.rs`, `layout.rs`, `rules.rs` | 메인 창/광고 판정, 약한 신호 확인, 보호 가드, mutation 계획 |
| `kakao-app/src/engine/apply.rs`, `restore.rs`, `caches.rs` | Win32 적용, 최초 스냅샷 보존, stale/OFF/종료 복원 |
| `kakao-win32/src/real.rs`, `event_hook.rs`, `process.rs` | 실제 Win32 호출, PID별 WinEvent 훅, 프로세스 핸들 기반 생존 확인 |
| `kakao-win32/src/tray/` 및 앱 `lib.rs` | 숨겨진 호스트 창·메시지 루프·트레이 메뉴, 설정 토글, 업데이트 요청 |
| `kakao-app/src/config/`, `startup*.rs`, `self_check.rs`, `dump*.rs` | 설정·규칙·로그, 시작프로그램, 진단 및 덤프 |
| `kakao-app/src/updater/` | HTTPS 매니페스트, Ed25519, URL·크기·SHA-256·버전 검증 및 staging |
| `kakao-updater/src/main.rs:61` → `lib.rs:229` | 부모 종료 대기, EXE 교체, 새 프로세스 readiness 확인, 실패 롤백 |

### 저장 및 외부 의존성

- `%APPDATA%\KakaoTalkAdBlockerLayout\layout_settings_v11.json`
- 같은 디렉터리의 `layout_rules_v11.json`, 회전 로그 `layout_adblock.log` 및 `.log.1`
- 사용자 레지스트리 `HKCU\...\Run`과 `Explorer\StartupApproved\Run`
- 업데이트 임시 EXE/헬퍼 및 설치 EXE의 `.exe.old` 백업
- `EngineCaches.snapshots/states/stale`는 워커 메모리 전용이다. 강제 종료 후 복원 스냅샷을 새 프로세스로 넘기지 않는다.
- **DB, SQL, migration, DB transaction은 사용하지 않는다.** 해당 범위는 N/A다.
- 주요 라이브러리: `windows 0.62`, `clap 4`, `serde/serde_json`, `crossbeam-channel`, `tracing`, `ureq 3`, `ed25519-dalek 3`, `sha2` 등. Cargo.lock으로 버전을 고정한다.
- 릴리스 외부 서비스: GitHub Releases. 빌드/매니페스트 생성에는 Windows SDK/링커와 Python `cryptography`가 사용된다.
- 실행: `cd rust` 후 `cargo run -p kakao-app --release`. 배포는 `scripts/build_release.ps1`에서 앱과 헬퍼 두 EXE를 생성한다.

### 핵심 사용자 흐름 및 상태 변경

```text
EXE → clap Args → 플랫폼/진단 인자 검증 → runtime paths/config bootstrap
    → 설정/rules typed overlay → 일반 실행 named mutex
    → 시작프로그램 동기화 → spawn_worker + native tray

PID 탐색/생존 확인 → PID별 WinEvent 훅 또는 reconciliation polling
    → guarded_tick → build_graph → evaluate_graph_for_apply
    → Strong/Weak 판정 및 CandidateState 갱신
    → enabled/apply/stopping/PID/identity/hung 가드
    → 최초 snapshot → hide → zero-size/뷰 resize → bounded WM_CLOSE
    → stale 복원/실패 게이지 → 트레이 상태 및 로그

트레이 ON/OFF·공격 모드 → 디스크 최신 설정 재독 → 한 필드 atomic 저장
    → 성공 시 해당 atomic flag 갱신 → notify_control → 워커 적용/복원

시작프로그램 토글 → HKCU Run 변경 → 설정 저장 → flag 반영
    → 저장 실패 시 레지스트리 역변경 시도 및 로그

업데이트 요청 → single-flight → HTTPS 매니페스트·서명·URL·버전 검증
    → 사용자 동의 → helper staging + 다운로드 크기/SHA-256 검증
    → 종료 요청/복원/join(3초 한도) → launch_helper
    → 부모 종료 대기 → staging 해시 재검증 → 기존 EXE 백업 → 새 EXE 설치
    → 재실행 → 준비 응답/250ms 생존 확인 → 백업 정리 또는 롤백

진단 인자 → self-check 또는 dump/series → 파일 기록 → stdout/종료 코드
```

창 변이와 복원 캐시는 워커 하나가 소유한다. UI/워커의 공유 상태는 atomic 플래그·카운터 및 mutex 오류 문자열이고, 업데이트 대기 항목은 `Arc<Mutex<Option<StagedUpdate>>>`다. 트레이 명령은 WndProc에서 큐에 넣고 바깥 메시지 루프에서 실행해 모달 대화상자의 재진입 시 mutable alias를 피한다.

## 3. Audit Coverage & Limitations

### 확인한 범위

README, CLAUDE, 기존 감사본, Rust README, Cargo workspace/각 crate 의존성, Cargo.lock 관련 오류, 루트 JSON 설정, `.gitignore`, Windows CI/release workflow, 배포 스크립트 주요 경로, 매니페스트 생성/검증 스크립트, 호환성 실측 문서를 확인했다. 문서 출력은 UTF-8로 다시 읽어 초기 PowerShell 표시 인코딩 문제를 보정했다.

**CodeGraph MCP를 실제 우선 사용했다.** `.codegraph/` 존재 확인 후 다음 활성 Rust 경로의 소스·caller/callee·영향 범위를 탐색했다.

- `main → run_with_args → spawn_worker/spawn_with_runtime → run_worker → guarded_tick → tick`
- `tick → build_graph → evaluate_graph_for_apply/evaluate_inner → apply_once`
- `apply_once → popup_host_guard_status/find_popup_matches/dismiss_popup` 및 legacy/aggressive/empty-EVA 분기
- `tick → apply_evaluation/capture_snapshot/identity_matches → RealWin32`
- `tick → restore_stale_hidden → restore_snapshot`, OFF/종료의 `restore_all/restore_all_final`
- `run_loop_with_ready → run_loop_inner → wnd_proc`, `TaskbarCreated`와 아이콘 재시도 타이머
- 설정 `load_json_value → typed merge/backup/heal → atomic_write`, 토글 read-modify-write
- `prepare_update → stage_helper/download_and_verify → launch_helper → update_executable_with → recover_interrupted_swap/launch_verified`
- `EventHook::install_for_pids/hook_proc/wait_message`, `PidWatch`, 진단·덤프·로그 회전

CodeGraph 응답에서 잘린 앱 callback/종료부, self-check 출력부, 테스트 assertion은 해당 구간 직접 열람으로 보완했다. 관련 테스트 파일의 visibility 중심 검증, fixture 좌표 및 updater 복구 테스트를 대조했다. 호출 그래프는 best-effort이며 같은 이름의 함수가 여러 개 나오는 경우 경로를 구분했다. 테스트를 찾지 못했다는 CodeGraph 문구만으로 테스트 부재를 확정하지 않았다.

### 실제 실행한 검증

| 검증 | 결과 | 해석 |
|---|---|---|
| `git status --short`, `git branch --show-current`, `git rev-parse --short HEAD` | 시작 시 clean, `main`, `9e33a55` | 감사 기준점 확인 |
| `cd rust; cargo test --workspace --locked --offline` | **실행 시작 전 의존성 해석 실패**, 종료 코드 1 | lock의 `ureq-proto 0.6.5`를 로컬 캐시/인덱스에서 해결하지 못함. 캐시 후보는 0.6.4. Rust 테스트는 실행되지 않았으며 pass/fail 수를 보고하지 않는다. |
| `cd rust; cargo fmt --all -- --check` | **통과**, 종료 코드 0 | 포맷 검사일 뿐 기능 검증이나 컴파일 통과를 의미하지 않는다. |
| Python ctypes로 감사 전용 STATIC parent/child 창 생성 후 Win32 호출 | **좌표 손실 확인** | 생성한 두 창만 조작하고 finally에서 parent를 파괴했다. 카카오톡/Explorer 등 기존 창은 조작하지 않았다. 앱 바이너리 E2E 실행과는 구분한다. |

좌표 probe는 parent 아래 child를 `(25,35,120,80)`으로 만들고 실제 구현과 같은 호출 시퀀스를 적용했다.

```text
SetWindowPos(child, 0,0,0,0, SWP_NOZORDER|SWP_NOACTIVATE)
SetWindowPos(child, ...,120,80, SWP_NOZORDER|SWP_NOACTIVATE|SWP_NOMOVE)

before = [25,35,120,80]
zero   = [0,0,0,0]
after  = [0,0,120,80]    # 크기 복구, 위치 미복구
```

### 제한 및 실행하지 않은 항목

- Rust 전체 suite/컴파일/clippy/릴리스 빌드/패키지 smoke는 완료하지 못했다. 오프라인 실패를 소스 결함이나 CI 실패로 단정하지 않았다. 의존성 설치나 lock 변경으로 우회하지 않았다.
- 실제 카카오톡 차단 ON/OFF, 로그인/로그아웃, 파일 전송, 실광고 노출 시간, 장시간 soak를 실행하지 않았다. 사용자 창·레지스트리·APPDATA 설정을 변경하는 앱 실행도 하지 않았다.
- Explorer 종료/재시작, 실제 업데이트 설치, 다른 볼륨 복사 중 강제 종료, 전원 장애/디스크 고갈은 재현하지 않았다. 해당 결론의 근거와 신뢰도를 ISSUE별로 구분했다.
- GitHub 최신 릴리스/실서비스 응답, 서명 개인키 설정, 네트워크 장애 상황을 실접속 검증하지 않았다. 패키지 취약점 DB 감사도 실행하지 않았다.
- Windows 10, x64/ARM64별 전체 앱 동작, DPI/다중 모니터 행렬은 미검증이다. 이번 native probe가 이 행렬을 대체하지 않는다.
- 저장소 호환성 기록은 2026-10-12의 v11.1.7-debug와 KakaoTalk 26.8.2.5324의 제한된 실측이다. v11.1.12 전체 기능 실측 통과로 전용하지 않았다.

### 반증으로 제외한 판단

- OFF에서 복원이 한 번 실패하면 끝난다는 과거 문제: 현재 `OffRestore::retry`가 200ms~5초 재시도하고 reset/notify가 있다.
- 동일 PID에서 훅 설치 실패 후 영구 미복구: 현재 `HookRetry`가 1~32초 재시도하고 폴링 fallback과 degraded 표시를 제공한다.
- close timeout이 모든 hide보다 선행한다는 과거 문제: 현재 snapshot/hide/set_pos 후 close이며 건당 50ms, tick 총 100ms close 예산이다.
- 숨겨진 부모 아래 child의 `IsWindowVisible=false`를 복원 실패로 오인: 현재 자신의 `WS_VISIBLE`로 성공을 검사한다.
- 일반 UI 중복 워커 및 업데이트 버튼 연타: named mutex와 update single-flight 가드가 있다. 공개되지만 실제 UI caller가 쓰지 않는 `apply_update`만으로 production 버그를 만들지 않았다.
- 미확인 팝업 제목을 빈 제목으로 허용: `text_known=false`는 popup guard blocked다. geometry-only hide는 기본 비활성이다.
- SQL injection/DB migration/서버 transaction 부재: 해당 저장/호출 경로가 없다.
- arbitrary shell command injection: updater는 `Command`의 개별 인자를 사용한다. 로컬 동일 사용자 파일 변조 가능성만으로 권한 상승 취약점을 주장하지 않았다.

## 4. High-Risk Issues

이 절에는 Confirmed 또는 강한 코드 근거가 있는 Likely만 포함한다. 절 제목과 별개로 모든 항목이 High 심각도인 것은 아니다.

### [ISSUE-001] 업데이트 중단 복구가 일반 재실행 경로에 연결되지 않고, 다른 볼륨에서는 설치 EXE에 직접 복사한다

- **위치:** `rust/crates/kakao-updater/src/lib.rs` — `update_executable_with` 244–348행, `recover_interrupted_swap` 184–201행. 앱 `updater/staging.rs::prepare_update`, `unique_staging_path`.
- **우선순위:** High
- **신뢰도:** Likely
- **문제:** 먼저 설치 EXE를 `.exe.old`로 옮긴 후 replacement를 설치 경로로 옮긴다. 두 rename 사이에는 설치 EXE가 없다. `%TEMP%`와 설치 폴더가 다른 볼륨이면 두 번째 rename 실패 후 `fs::copy(replacement, current)`로 설치 경로에 직접 쓴다. 이 구간에서 helper/전원이 중단되면 정상 실행 경로의 EXE 부재 또는 불완전 파일 상태가 가능하다.
- **발생 조건:** 자동 업데이트 중 두 파일 이동 사이에 helper가 중단되거나, 예를 들어 C:의 TEMP에서 D: 설치 폴더로 복사하는 도중 프로세스/시스템이 중단된다. 다른 볼륨 설치는 사용자 선택으로 가능한 정상 배치다. 장애 빈도는 이번 감사에서 측정하지 않았다.
- **영향:** 차단기 일반 실행 및 시작프로그램이 실패할 수 있다. `.exe.old`가 남으면 수동 복구는 가능하므로 영구적인 대화 데이터 유실이나 전체 시스템 손상으로 확대하지 않는다.
- **근거:** `lib.rs:293` current→backup, `299–312` replacement→current 및 직접 copy fallback. 복구 함수는 `current.exists()`이면 즉시 `Ok(false)`여서 불완전 current가 존재하는 경우 복구하지 않는다. current가 없는 경우에도 이 복구 함수는 **helper를 다시 실행해야** 도달한다. 일반 실행 파일 자체가 없으면 그 EXE의 재실행으로 복구 함수를 호출할 수 없다. 앱은 TEMP에 staging한다.
- **반증 확인:** staging SHA-256 재검증, 현재 파일 보존용 backup, 정상 오류 반환 시 rollback, 새 앱 readiness 실패 시 rollback이 존재한다. 이들은 helper가 계속 실행되어 오류 처리에 도달할 때 유효하다. 강제 중단에는 실행되지 않는다. 기존 `interrupted_swap_*` 테스트는 API/helper를 다시 호출해 복구하는 시나리오로, 사용자의 일반 EXE 더블클릭 복구를 보장하지 않는다. 다른 볼륨 복사 실패 시 OS가 일부 파일을 정리하는지에 기대지 않고 **진행 중 중단**을 문제 조건으로 삼았다.
- **호출/영향 범위:** CodeGraph 및 앱 종료부 보완 추적: `run_with_args/CheckUpdate → prepare_update → download_and_verify → pending_update → launch_helper → helper main → update_executable_with → recover_interrupted_swap/launch_verified`. 앱 재실행·HKCU Run 경로·설치 파일/백업에 영향. 서명 검증 우회 이슈는 아니다.
- **권장 수정 방향:** 기존 EXE를 옮기기 전에 설치 폴더의 별도 staging 파일로 복사·flush·크기/해시 확인을 끝낸다. 이후 같은 볼륨에서 교체하며, 중단 상태를 복구할 별도 launcher/helper 진입점 또는 설치 복구 절차를 마련한다. 부분 current 존재도 정상 설치로 간주하지 않도록 검증한다. 백업 정리는 검증된 재기동 뒤에 한다.
- **필요한 회귀 테스트:** 서로 다른 두 볼륨에서 준비 복사 중/백업 직후/최종 교체 직후 helper를 테스트 전용 child로 중단한다. 매 지점에서 기존 또는 새 정상 EXE로 일반 실행/복구가 가능해야 한다. 불완전 current+정상 backup 상태도 테스트하며, 복구 전에 정상 backup을 stale로 삭제해서는 안 된다.

### [ISSUE-002] Explorer 재시작 후 NIM_ADD 실패 시 트레이 재시도가 예약되지 않는다

- **위치:** `rust/crates/kakao-win32/src/tray/host.rs::run_loop_inner` 169–171행, `wnd_proc` 267–293행.
- **우선순위:** Medium
- **신뢰도:** Confirmed — 실패 분기의 타이머 미예약을 코드 흐름으로 확인. Explorer 장애 주입은 실행하지 않았다.
- **문제:** 최초 아이콘 등록 성공 시 재시도 타이머는 없다. `TaskbarCreated` 처리에서 `NIM_ADD`가 실패하면 `icon_added=false`만 저장한다. 성공하면 타이머를 끄지만, 실패했을 때 타이머를 새로 켜는 처리가 없다.
- **발생 조건:** 앱 실행 시 아이콘 등록 성공 → Explorer 재시작 → 해당 `TaskbarCreated`의 첫 재등록 일시 실패 → 추가 `TaskbarCreated` 없음.
- **영향:** 워커는 계속 광고 UI를 변경하지만 트레이의 OFF/종료/설정 메뉴가 사라진다. 앱은 트레이 전용이라 다른 제어 화면이 없다. 일반 EXE 재실행도 singleton으로 종료된다. Explorer를 다시 재시작하거나 프로세스를 종료/재실행해야 할 수 있고 강제 종료하면 복원 스냅샷을 잃을 수 있다.
- **근거:** `169–171`은 초기 `!added`일 때만 `SetTimer(TRAY_RETRY_TIMER_ID)`를 호출한다. `281–291`은 TaskbarCreated 재등록 실패에 `SetTimer`를 호출하지 않는다. `TRAY_STATUS_TIMER_ID`는 tooltip 갱신용이고 `icon_added=false`이면 `refresh_tooltip`이 즉시 반환한다.
- **반증 확인:** 초기 실패에 대한 지연 재시도 및 2초 타이머는 존재한다. 그러나 문제 조건은 최초 등록이 성공해서 타이머가 없는 상태다. 다음 Explorer 재시작 메시지가 오면 재시도할 수 있지만 재시작 없이 자동 회복하지 못한다. `continue_message_loop_without_icon=true`는 프로세스 생존만 보장한다.
- **호출/영향 범위:** CodeGraph의 `run_with_args → run_loop_with_ready → run_loop_inner → wnd_proc`, `try_add_notify_icon/refresh_tooltip` 및 트레이 명령 전달. 광고 판정 자체보다 사용자의 제어·복구 기능에 영향.
- **권장 수정 방향:** 모든 재등록 실패 경로에서 같은 재시도 타이머를 예약하고 성공 시 해제한다. 타이머 생성 실패도 상태/로그에 남긴다. 장기 트레이 장애에서 사용자가 종료/복원을 요청할 대체 경로를 검토한다.
- **필요한 회귀 테스트:** 초기 add=true, TaskbarCreated add=false, 타이머 add=false→true 순서로 shell API를 주입한다. 실패 직후 retry timer 존재, 성공 직후 timer 해제, 단일 아이콘 유지 및 메뉴 복구를 검증한다.

### [ISSUE-003] zero-size 처리한 팝업 자식 창의 원래 위치가 복원되지 않는다

- **위치:** `rust/crates/kakao-core/src/evaluate/apply.rs::dismiss_popup` 226–229행, `kakao-app/src/engine/apply.rs` 120–151행, `engine/restore.rs::restore_snapshot` 82–100행.
- **우선순위:** Medium
- **신뢰도:** Confirmed — production 코드 흐름 및 동일 플래그 시퀀스의 실제 Win32 API probe로 확인. 카카오톡 자체의 시각적 재현은 수행하지 않았다.
- **문제:** 팝업 host와 매칭 child 모두 `set_pos(hwnd,0,0,0,0)` 대상으로 계획된다. 실제 apply는 zero-size에 `SWP_NOMOVE`를 붙이지 않아 child도 부모 기준 `(0,0)`으로 이동한다. 복원은 `top_level=false`인 모든 창에 `SWP_NOMOVE`를 붙여 원래 위치를 되돌리지 않고 크기만 복구한다.
- **발생 조건:** 허용된 popup host 안의 매칭 child가 원래 부모 기준 non-zero offset에 있고, hide/zero-size는 성공하나 WM_CLOSE로 소멸하지 않은 뒤 OFF/종료/신호 소멸로 복원된다. popup 탐색 깊이 2도 허용되므로 중첩 child에서 현실적인 조건이다.
- **영향:** 복원된 child의 UI 위치가 어긋나거나 다른 영역을 덮을 수 있다. 함수는 크기/가시성 성공을 보고 스냅샷을 제거하므로 자동 재시도로 원래 offset을 되찾지 못한다. 메인 뷰 일반 resize 전체가 이 문제라는 뜻은 아니다.
- **근거:** 최초 snapshot에 rect는 보존하지만 child 복원에서 위치 인자를 무시한다. 감사 전용 실제 창에서 `(25,35,120,80) → (0,0,0,0) → (0,0,120,80)`을 확인했다. `SWP_NOMOVE`가 X/Y를 무시하는 동작은 [Microsoft SetWindowPos 문서](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-setwindowpos)와 일치한다.
- **반증 확인:** parent-first 복원, identity 검사, 자신의 WS_VISIBLE 확인, 복원 retry는 존재한다. parent 복원으로 child의 **부모 상대 offset**이 원래 값으로 돌아오지는 않는다. 원래 offset이 `(0,0)`인 fixture에서는 드러나지 않는다. `f03_close_refused_falls_back_to_hide_and_restores_on_off`는 visibility와 실패 수만 검사한다. snapshot의 화면 좌표를 child 위치에 그대로 쓰지 않으려는 SWP_NOMOVE의 목적은 타당하지만, 앞선 zero-size가 위치를 이동시켰다는 점을 해결하지 못한다.
- **호출/영향 범위:** CodeGraph의 `apply_once → retain_popup/dismiss_popup → Evaluation.actions → tick/apply_evaluation → capture_snapshot`, 이어서 `OffRestore/restore_stale_hidden/restore_all_final → restore_snapshot`. close를 거절한 팝업 child 및 중첩 매칭 창에 한정된다.
- **권장 수정 방향:** 자식 zero-size에서는 위치를 유지하거나, parent-client 좌표 및 부모 identity를 별도 캡처하여 복원한다. 일반 메인 뷰 resize와 top-level 팝업 위치 복원은 기존 구분을 유지한다. 화면 좌표를 자식 상대 좌표로 그대로 넘기는 수정은 피한다.
- **필요한 회귀 테스트:** child offset `(25,35)`, 깊이 2, host 이동 및 부모 숨김 조건에서 close 거절 후 OFF/종료/stale 복원을 실행한다. 위치·크기·자신의 WS_VISIBLE이 모두 최초 값과 같아야 하고, 정상 복원 시에만 snapshot이 제거되어야 한다.

## 5. Potential Functional Gaps

아래 항목은 확정 버그 수에 포함하지 않았다.

| 항목 | 판단 | 현재 상태와 보완점 |
|---|---|---|
| 트레이 장기 장애의 대체 제어 | Confirmed Gap | native tray 전용이고 singleton 재실행도 기존 앱을 열어 주지 않는다. 별도 OFF/복원/종료 IPC 또는 장애 안내 경로가 없다. ISSUE-002 해결 후에도 장기 shell 장애에 대한 제품 판단이 필요하다. |
| cross-process 복원 | Confirmed Gap | snapshot은 메모리 전용. 강제 종료/3초 종료 한도 후에는 카카오톡 재시작이 복구 방법이다. README가 제한을 명시하므로 독립적인 새 버그로 세지 않는다. HWND를 무검증 영속화하는 해결은 위험하다. |
| 업데이트 helper의 자동 버전 동기화 | Confirmed Gap | 자동 업데이트 artifact는 메인 EXE 하나이고 helper는 설치된 것을 TEMP로 복사한다. README는 두 EXE를 함께 배치하라고 안내한다. 수동 ZIP 갱신 없이 helper 개선이 전파되지 않는 제약이 있다. helper 버전/프로토콜 호환성 확인을 검토한다. |
| 설정·시작프로그램 토글 실패의 사용자 피드백 | Confirmed Gap | 저장 실패는 flag 반영을 막아 상태를 보존하지만 enabled/aggressive 실패는 주로 로그에만 남는다. startup rollback 실패 반환도 무시한다. ACL/디스크 장애를 사용자에게 표시하고 registry/파일 재동기화 결과를 확인하는 개선이 필요하다. 현실적인 고정 불일치 재현 없이 별도 High 이슈로 세지 않았다. |
| 실기 호환성 및 장시간 검증 | Confirmed Gap | 저장소 실측은 한 버전/제한된 UI 관측이다. 현재 릴리스 Windows 10/ARM64/DPI/미디어 뷰어/로그아웃/soak 결과가 추가로 필요하다. |
| 네트워크 재시도 | Likely Gap | connect/read/global timeout과 사용자 오류 대화상자는 있다. 자동 retry가 없어 일시 장애에는 사용자가 다시 요청한다. 수동 업데이트 도구에서 이것만으로 기능 버그라고 판단하지 않았다. |
| 파손 설정의 backup 실패 처리/동시 작성 | Likely Gap | `backup_broken` 실패 후에도 heal을 시도하며, bootstrap is_file→write 및 토글 read-modify-write에는 프로세스 간 lock이 없다. unique temp와 atomic rename은 torn write를 막지만 트랜잭션 전체를 잠그지는 않는다. 백업 실패와 동시 진단의 fault-injection 검증이 필요하며, 현재 사용자 설정 유실이 재현됐다고 주장하지 않는다. |
| hang 진입 직전의 Win32 호출 | 추정 | IsHungAppWindow 사전 확인은 존재하나 이후 동기 호출 전체의 시간 상한은 아니다. API 확인 후 대상 thread가 멎는 경합은 추가 실험 대상이다. README도 전체 Win32 실시간 상한을 보장하지 않으므로 확정 버그로 세지 않았다. |

## 6. Documentation Mismatches

1. **CI 보장 범위 과장:** `CLAUDE.md:193`은 windows-ci가 no-sign packaging build와 packaged strict self-check를 검증한다고 설명한다. 실제 `.github/workflows/windows-ci.yml`은 `cargo build --release`, EXE 복사, **`--self-check --json`**이며 `--strict-self-check`나 `scripts/build_release.ps1 -NoSign`을 호출하지 않는다. 태그 release workflow가 별도로 build script를 호출하는 사실과 구분해야 한다.
2. **strict self-check의 의미:** `README.md:243`은 경고성 항목까지 실패로 검증한다고 설명한다. 실제 `self_check.rs:17–24,47–53`은 config/self-heal 경고를 info로 분리하여 strict에서도 실패시키지 않는다. `CLAUDE.md:206`의 tray 경고 승격 설명도 현재 Rust self-check에는 대응하는 tray 진단 필드/검사가 없다. 실제 strict 실패 조건은 APPDATA 쓰기, Run 읽기/쓰기, 프로세스 열거 등의 core warning이다.
3. **업데이트의 원자성:** `README.md:373`의 EXE 원자적 교체 설명은 두 rename 사이의 부재 구간 및 cross-volume 직접 copy까지 포괄하지 못한다(ISSUE-001). 정상 오류 rollback과 중단 안전성을 구분해야 한다.
4. **트레이 복구 보장:** `README.md:162`의 아이콘이 증발하지 않는다는 설명과 CLAUDE의 TaskbarCreated/타이머 재등록 설명은 ISSUE-002의 일시 재등록 실패까지 보장하지 않는다.
5. **종료 복원의 절대 표현:** README 메뉴 예시 `189행`은 모든 카카오톡 창 원상 복구 후 종료로 표현하지만 본문 `복구 제한` 설명과 실제 구현은 hide/zero-size snapshot만 복원하며 3초 한도·실패 잔존이 있다. 메뉴 예시도 같은 제한을 반영해야 한다.

Python 참고 구현이라고 명시된 pystray/Tk·`os.replace`·stop 2초 설명 자체는 Rust 불일치 버그로 세지 않았다. layout-only를 네트워크 통신 자체가 전혀 없다는 의미로 해석하지 않았다.

## 7. Recommended Fix Plan

### Phase 1 — Immediate

1. **ISSUE-001:** 설치 디렉터리 staging·검증을 먼저 완료하고 중단 복구 가능한 교체 절차를 만든다. 일반 실행 진입점이 사라진 경우도 복구할 수 있어야 한다. fault-injection 검증 전 원자적/즉시 rollback 보장을 문서에서 제한한다.
2. **ISSUE-002:** TaskbarCreated 재등록 실패에 retry timer를 예약한다. 아이콘을 잃어도 OFF/복원/종료를 요청할 수 있는 최소 대체 경로를 검토한다.
3. **ISSUE-003:** 자식 zero-size 시 위치 보존 또는 정확한 parent-relative 복원을 구현한다. 실제 위치까지 검증하는 회귀 테스트를 함께 추가한다.

### Phase 2 — Stability

- 설정 저장·startup 역롤백 실패를 사용자 상태에 노출하고, 실제 registry/파일/flag 상태를 다시 확인한다.
- backup 실패·동시 self-check·원자 저장 실패를 격리 디렉터리에서 검증한다. 유효한 원본/백업을 지키는 실패 정책을 정한다.
- helper/앱의 업데이트 프로토콜 호환성을 검사하고 두 EXE의 동기화 방법을 문서화한다.
- Windows 10/11, x64/ARM64, DPI/혼합 모니터, Explorer 복구, 실제 KakaoTalk 재실행/soak를 검증한다.
- CI 문서를 실제 gate와 일치시키고 필요하면 strict/package 검사를 실제 workflow에 연결한다.

### Phase 3 — Structural

- shell notify/timer, 파일 교체·복사·sync, 프로세스 시작/readiness를 주입 가능한 경계로 두어 OS 오류를 결정적으로 재현한다.
- snapshot에 적용한 mutation 종류와 좌표계를 명확히 표현해 hide-only/zero-size/top-level/child 복원 의미가 섞이지 않게 한다.
- 기존 core golden/negative 계약은 유지한다. 구조 개선만으로 광고 후보 범위나 정상 UI 보호 규칙을 바꾸지 않는다.

실제 구현 변경은 이번 감사에 포함하지 않았다.

## 8. Test Recommendations

| 종류 / 대상 | 입력·조건 | 기대 결과 |
|---|---|---|
| Unit — ISSUE-002 | 초기 add 성공, TaskbarCreated add 실패, 다음 timer add 성공 | 실패 즉시 retry timer 예약, 성공 후 해제, 아이콘 하나 유지 |
| Unit — ISSUE-003 | parent-relative `(25,35)` child, zero-size 계획, close 거절 | OFF/종료 복원 후 위치·크기·style 모두 최초 값, 실패 시 snapshot 유지 |
| Unit — 설정 | BOM UTF-8, 부분 타입 오류, inverted min/max, 잘못된 최상위 타입 | 유효 필드 유지, 잘못된 필드만 fallback, 범위 교정, 파손 원본 보존 및 경고 |
| Unit — 광고 가드 | Adobe/일반 웹패널/unknown text/다른 PID/제목 있는 미디어 host | 변이 없음. 기본 geometry-only 비활성 및 Weak 별도 tick 확인 유지 |
| Integration — ISSUE-001 | 설치 위치와 TEMP를 서로 다른 테스트 볼륨에 배치 | 설치 경로에 partial file을 노출하지 않고 검증된 새 버전 또는 이전 버전 실행 가능 |
| Integration — 업데이트 중단 | helper child를 staging/백업/교체/ready 단계별로 중단 | 정상 backup 보존, 일반 복구 경로가 작동, 재시도 시 정상 backup을 잘못 삭제하지 않음 |
| Integration — 업데이트 실패 | 잘못된 서명/URL/size/hash, HTTP timeout, relaunch 실패/ready 미응답 | 교체 전 거절 또는 이전 EXE 복원·재실행, single-flight 해제, 반복 시 임시파일 누적 제한 |
| Integration — OFF 복원 | hide 후 대상 hung→OFF→정상 응답, OFF 유지 | ON 전환 없이 200ms~5초 retry로 복구, 실패 게이지 해소, 새 hide 없음 |
| Integration — 훅 | 같은 PID 집합에서 두 번 설치 실패 후 성공 | polling 지속, 1/2/.../32초 retry, 성공 시 degraded 해소 |
| Integration — 진단 | 출력 경로가 디렉터리/쓰기 거절, series duration>10000 및 interval<10 | 명시된 종료 코드·오류, duration 거절/interval clamp, 실제 시각 단조 증가 |
| Concurrency — 단일 실행 | 격리 프로필에서 일반 EXE 두 개 동시에 시작 | apply 워커 하나만 존재, 두 번째 안전 종료, 진단은 변이 없이 동작 |
| Concurrency — 설정/종료 | self-check와 토글 저장 교차, 업데이트 준비 중 사용자 종료 | JSON 파손·설정 유실·이중 helper 없음, 종료 후 재은닉 없음, 잔여 staging 처리 명확 |
| Regression — fixture 보강 | 기존 popup fixture child에 non-zero offset·깊이2·host 이동 추가 | visibility뿐 아니라 screen/client geometry와 parent identity도 보존 |
| Regression — 기존 보호 | owned_popup_legacy_ad 및 guarded viewer, close 지연 300ms | 기존 판정 불변, hide는 bounded close보다 선행, 거절 팝업은 계속 억제 |
| End-to-End — 핵심 기능 | 전용 테스트 계정에서 앱 시작→배너/독립 팝업→OFF/ON→종료 | 광고 억제, 정상 채팅/친구/잠금/미디어 유지, OFF/종료 복구, 미관측 항목은 NOT OBSERVED |
| End-to-End — 시작/업데이트 | 전용 HKCU 프로필에서 시작프로그램 등록, 재로그온, 서명된 테스트 릴리스 | 명령 인용·경로 정확, 한 인스턴스, worker/tray 준비 뒤 ready, 실패 rollback |
| Platform-specific | Win10/11, x64/ARM64, 100/125/150% DPI, 음수 모니터 좌표, 한글/공백/다른 볼륨 경로 | 좌표·경로·UTF-8 보존, 올바른 대상 아키텍처 패키징 및 실행 |
| Platform-specific — shell | 전용 VM에서 Explorer 재시작, 첫 NIM_ADD 실패 후 회복 | 사용자 조작 가능한 아이콘 자동 복구, 워커 중복 없음 |
| Soak | 전용 환경 1시간 이상 창 생성/파괴·ON/OFF·카카오톡 재실행, DEBUG 로그 회전 | snapshot/후보/메모리 누적 없음, CPU/지연 실측, 정상 복원 후 오류 해소 |

위 표는 **제안**이며 이번 감사에서 실행한 테스트 목록이 아니다. Rust suite 재실행 시 먼저 잠금 의존성을 해결한 준비된 환경을 사용하고, 테스트용 APPDATA/HKCU/VM을 격리해야 한다. 원본 사용자 데이터를 수집하는 dump는 공유 전에 익명화한다.

## 9. Final Assessment

| 항목 | 평가 | 근거 |
|---|---|---|
| Functional Correctness | Needs Work | 핵심 판정/보호 경로는 구현되어 있으나 팝업 child 복원과 트레이 재등록 실패 경로에 확인된 오류가 있다. |
| Runtime Stability | Needs Work | 단일 워커·panic 격리·훅/OFF retry는 양호하다. shell 장애 후 제어 상실과 업데이트 중단 복구가 남는다. |
| Data Integrity | Needs Work | 대화/첨부 DB 변이는 없고 설정 atomic 저장도 있다. 설치 EXE의 중단 안전성은 부족하다. |
| Error Resilience | Needs Work | 네트워크 timeout·검증·정상 오류 rollback은 있다. 프로세스 중단 및 shell 재등록 실패를 끝까지 복구하지 못한다. |
| Cross-platform Robustness | Acceptable | Windows 전용 정책과 비Windows fail-fast는 명확하다. Windows 내부 아키텍처/DPI/볼륨 행렬 검증은 제한적이다. |
| Test Confidence | Needs Work | fixture/전이/negative/readiness 테스트가 존재하지만 이번 Rust suite는 의존성 해석에서 막혔다. 실제 좌표·Explorer 재등록·중단 설치 테스트를 보강해야 한다. |

**실제로 먼저 수정할 문제 3개:**

1. **ISSUE-001:** 업데이트 준비를 설치 볼륨에서 끝내고, 중단 후 정상 EXE로 복구하는 진입점을 보장한다.
2. **ISSUE-002:** Explorer 재등록 실패 시 자동 재시도를 반드시 예약한다.
3. **ISSUE-003:** zero-size 처리한 자식 창의 원래 부모 상대 좌표를 보존·복원한다.
