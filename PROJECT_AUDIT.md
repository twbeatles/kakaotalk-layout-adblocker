# Project Audit

- **감사 일자:** 2026-09-24 (Asia/Seoul)
- **대상:** KakaoTalk Layout AdBlocker **v11.1.4** (Rust 네이티브) / branch `main` @ `08da70c` (Rust 장문 파일 모듈 분할 직후)
- **범위:** 기능 구현·런타임 안정성 + **성능/최적화** (사용자 추가 요청)
- **감사 방식:** 문서(README/CLAUDE/BENCHMARK) → CodeGraph MCP로 호출 흐름 파악 → Rust 런타임 소스 직접 열람 → 테스트 실행 → **실행 중인 실제 카카오톡/차단기 프로세스에 대한 읽기 전용 실측** → 반증 확인 후 남은 이슈만 기록
- **코드 변경:** 없음. 본 문서만 작성했다.
- **이전 감사 문서 처리:** 2026-09-12(v11.1.3) 감사본을 대체한다. 원문은 git 이력(`ce9a223`)에 남아 있다. 당시 ISSUE-001~006이 현재 소스에서 해소되었음을 재확인했고 요약을 §10에 남겼다.

---

## 0. Remediation status (v11.1.5, 2026-09-24)

본 감사 직후 지적 사항을 구현해 **v11.1.5**로 릴리스했다. 명세·계획·작업 목록은 `specs/002-audit-2026-09-24-remediation/`에 있다. **아래 §1~§9 본문은 수정 이전(v11.1.4 @ `08da70c`) 시점의 감사 기록으로 유지한다.**

| 항목 | 상태 | 반영 위치 / 검증 |
|---|---|---|
| ISSUE-001 BOM 설정 덮어쓰기 | 수정 | `config/storage.rs` `load_json_value`. `config_migration.rs` BOM 테스트 3건 |
| ISSUE-002 / P1 부재 시 200ms 스캔 | 수정 | `engine/schedule.rs` `pid_scan_interval`(5s 후 1s, 30s 후 2s), 생존 시 재동기화 30s(P8), `process::PidWatch`(P10) |
| ISSUE-003 hung 종료 대기 | 수정 | `Win32Api::is_hung_app_window` 가드(apply·restore), `worker::join_with_timeout` 3s. `hung_window_is_neither_hidden_nor_restored`, `join_with_timeout_gives_up_on_a_stuck_thread` |
| ISSUE-004 거짓 복원 실패 | 수정 | `Win32Api::has_visible_style`, top-level 우선 복원. Fake opt-in 조상 가시성. `child_restored_under_a_hidden_parent_is_not_a_failure` |
| ISSUE-005 오래된 `last_error` | 수정 | `SharedFlags::report_restore_failures`. `restore_error_clears_itself_after_natural_recovery` |
| §5 시작 경고 미노출 | 수정 | `observability::startup_warning_summary` → `last_error` |
| §5 Run ↔ 설정 동기화 | 수정 | `startup_repair::adopt_registry_startup_state` |
| §5 업데이트 실패 시 미재실행 | 수정 | `kakao_updater::{should_relaunch_previous, relaunch_previous, discard_replacement}`, app `discard_staged` |
| §5 워커 감시 | 수정 | `guarded_tick`(`catch_unwind`, 스냅샷 유지) + `WorkerExitGuard`. `a_panicking_tick_keeps_snapshots_and_reports_the_error` |
| §5 fsync 없음 | 수정 | `atomic_write` `sync_all` |
| §5 토글이 외부 편집 덮어씀 | 수정 | `config::update_settings`. 테스트 2건 |
| §5 OFF 중 WinEvent 적체 | 수정 | OFF 전환 시 훅 해제 |
| P2 적응형 유휴 주기 | 수정 | `schedule::reconcile_interval`, 새 설정 `idle_backoff_max_ms`(기본 1000) |
| P3 그래프 수집 O(N) | 수정 | `graph_build.rs` + `Win32Api::enum_descendant_windows`. `graph_build_parity.rs`(fixture 전체 + 실데스크톱 584노드 동일) |
| P4 top-level 캐시 | **보류** | `perf_probe` 실측 결과 `EnumWindows` 순회는 45µs/tick(build_graph 307µs의 15%). 절감 효과가 작고, 캐시 무효화 누락 시 주 광고 호스트(owned top-level)를 놓칠 위험이 큼 |
| P5 스냅샷 재캡처 | 수정 | `apply.rs` `remember_snapshot`. `hidden_window_snapshot_is_captured_once` |
| P6 평가 계층 할당 | 수정 | 빈 텍스트 조기 반환, 토큰·needle lowercase 1회, `WindowGraph::children_of`. 골든/패리티 불변 |
| P7 이벤트 병합 | 수정 | `EventHook::pump_for` |
| P9 릴리스 프로파일 | 수정 | `lto`, `codegen-units=1`, `strip`(panic=unwind 유지). EXE 4,510,720 → 4,261,376 bytes |
| §6 문서 불일치 D1–D7 | 수정 | README/BENCHMARK 재측정값으로 교체, CLAUDE.md "Rust 활성 런타임 동작" 갱신, `restore.rs` 주석 |

**실측 (같은 PC, 같은 카카오톡 26.8.1.5315, 워밍업 35초 후 60초, ARM64 에뮬레이션):**

| 빌드 | 60초 CPU | 코어 1개 대비 | Working Set / Private |
|---|---:|---:|---:|
| 수정 전 (dist v11.1.4) | 406.2ms | 0.677% | 24.5MB / 11.0MB |
| 수정 후 | 93.8ms | **0.156% (−77%)** | 21.1MB / 12.0MB |

마이크로벤치(`perf_probe`, 릴리스): build_graph 307µs, 평가 44µs, Toolhelp 스캔 6.5ms. 감사 본문의 "스냅샷 3.3ms"는 Python으로 스냅샷 생성만 잰 값이었다. Rust 스캔(열거 포함)은 6.5ms이므로, 수정 전 카카오톡 부재 시 비용은 본문 추정(약 1.6%)보다 큰 약 3.2%/코어였다.

**검증:** `cargo fmt --check`, `cargo clippy --all-targets --all-features -D warnings`, `cargo test --workspace` 112 passed / 0 failed / 2 ignored(실데스크톱 패리티·perf 프로브, 수동 실행 시 통과). pytest는 52 passed이고, 나머지 11건은 이전과 같이 `cryptography` 미설치로 인한 수집 실패다.

**감사 이후 확인한 실사용 증거:** 사용자 로그 `2026-09-23T00:41:35Z WARN … restore show failed hwnd=67730`은 ISSUE-004 유형의 거짓 복원 실패와 일치한다.

---

## 1. Executive Summary

**전체 상태: Acceptable (양호에 가까움). 전체 위험도: Low–Medium.**

핵심 광고 차단 경로(메인 윈도우 확정 → legacy signature hide → aggressive subtree hide → guarded popup dismiss → empty `EVA_ChildWindow` close)와 복원 경로(스냅샷 → OFF/종료/신호 소멸 시 원복 → 실패 시 백오프 재시도)는 일관되게 구현되어 있다. `cargo test --workspace` 85건이 전부 통과했다. 실제 카카오톡 `26.8.1.5315`를 대상으로 한 `--dump-tree`에서도 광고 호스트(owned `EVA_Window_Dblclk`)가 `strong/hide`로 판정되었다. 이전 감사의 High-Risk 6건은 모두 해소되어 있다.

이번에 남은 문제는 **입력 파일의 인코딩 처리**, **카카오톡이 응답 없음일 때의 종료 경로**, **"유휴 시 사실상 0%"라는 성능 주장과 실제 스케줄링의 차이**에 몰려 있다.

가장 중요한 문제:

1. **[ISSUE-001] UTF-8 BOM이 붙은 설정/규칙 JSON을 "손상"으로 판정해 기본값으로 덮어쓴다.** (Medium, Confirmed — 격리 APPDATA에서 재현) README가 규칙 JSON 직접 편집을 권장하므로 현실적인 경로다. 사용자 설정(`aggressive_mode`, `run_on_startup` 등)이 조용히 초기화된다.
2. **[ISSUE-002] 카카오톡이 실행 중이 아닐 때 전체 프로세스 스냅샷을 200ms마다 반복한다.** (Medium/성능, Confirmed) 카카오톡이 실행 중일 때(5초 주기)보다 미실행일 때 CPU를 더 쓴다. 이 기기에서 스냅샷 1회에 약 3.3ms가 걸려 코어 1개 기준 약 1.6%다.
3. **[ISSUE-003] 카카오톡 UI 스레드가 응답 없음이면 종료가 무기한 멈출 수 있다.** (Medium, Likely) 종료 시 복원이 cross-thread 동기 `ShowWindow`/`SetWindowPos`를 부르고 `worker.join()`에 타임아웃이 없다. 트레이 아이콘은 사라졌는데 프로세스가 단일 인스턴스 mutex를 계속 쥐게 된다.
4. **[ISSUE-004] 복원 성공 여부를 `IsWindowVisible`로 판정해 조상 창이 숨겨진 자식 창은 성공해도 "복원 실패"로 집계한다.** (Low, Likely) 트레이에 거짓 "복원 실패 N건"이 뜨고, 차단 OFF 상태에서는 사라지지 않는다.
5. **성능 문서 불일치(§6).** 실측 유휴 CPU는 코어 1개 기준 0.65%(8코어 합산 약 0.08%)로, README의 "0.01%대"나 BENCHMARK의 "0.0%"가 아니다. EXE도 4.5MB로 문서상 1.8MB와 다르다. 릴리스 프로파일(LTO 등)이 아예 없다.

**데이터 손상/유실 가능성: 낮음.** DB가 없고 사용자 데이터를 보관하지 않는다. 유일한 "유실"은 ISSUE-001의 설정 초기화인데, 원본은 `*.broken-<epoch>` 백업으로 남는다(30일 또는 최신 10개까지 보존). 설정 저장은 임시 파일 + rename 원자 교체이고, 업데이터는 서명·해시 재검증과 롤백을 갖췄다.

**가장 먼저 수정할 영역:** 설정/규칙 로더의 BOM 허용(ISSUE-001), 종료 경로의 bounded join과 hung-window 가드(ISSUE-003), 카카오톡 미실행 시 PID 스캔 백오프(ISSUE-002).

---

## 2. Project Understanding

### 목적
카카오톡 Windows 클라이언트의 광고 영역을 **레이아웃 조정(숨김/리사이즈/닫기)만으로** 제거하는 트레이 상주 앱이다. hosts/DNS/네트워크 차단, DLL 인젝션, 메모리 패치를 쓰지 않는다. 레지스트리는 `HKCU Run` 시작프로그램 등록에만 쓴다. Windows 전용이며 비Windows에서는 종료 코드 `2`로 즉시 끝난다.

### 주요 entrypoint
| Entry | 위치 | 비고 |
|---|---|---|
| 트레이 앱(기본) | `kakao-app/src/main.rs` → `lib.rs::run_with_args` | named mutex `Local\KakaoTalkLayoutAdBlocker_v11` |
| `--self-check [--strict-self-check] [--json]` | `self_check.rs::run` | mutex 밖. 설정 self-heal을 수행하므로 **파일을 쓸 수 있다** |
| `--dump-tree` / `--dump-tree-series` | `dump_cmd.rs::run_dump_commands` | mutex 밖, 읽기 전용 Win32 조회 |
| `--shadow [--apply]` | `lib.rs:125-139` | 1회 평가만 |
| `--check-update` (hidden) | `updater::check_for_update` | |
| 업데이트 헬퍼 | `kakao-updater/src/main.rs` | 부모 종료 대기 → 해시 재검증 → 백업/교체/롤백 → 재실행 |
| Python 참고 구현 | `legacy/python-v11/` | 골든 fixture/회귀 전용. 루트 `.py`는 안내만 출력 |

### 핵심 모듈
- `kakao-core` (Win32 없음): `graph`(창 트리), `signals`/`layout`(판정 원시함수), `evaluate/{inspect,apply,orchestrate}`(판정·변이 계획), `rules`(규칙 모델 + 필드 단위 overlay)
- `kakao-win32`: `real.rs`(Win32 래퍼), `event_hook.rs`(PID 한정 WinEvent 훅), `process.rs`(Toolhelp PID 스캔, liveness), `tray/*`(메시지 루프·메뉴·툴팁), `startup.rs`(Run/StartupApproved), `single_instance.rs`
- `kakao-app`: `engine/{worker,tick,apply,restore,caches,flags,model}`, `graph_build.rs`, `config/{settings,storage,log,paths}`, `updater/*`, `startup*.rs`, `self_check.rs`
- `kakao-updater`: 교체 헬퍼 바이너리

### 데이터 저장 방식
- `%APPDATA%\KakaoTalkAdBlockerLayout\layout_settings_v11.json`, `layout_rules_v11.json`: JSON. 필드 단위 타입 검증 병합. 파싱 실패 시 `*.broken-<unix-epoch>` 백업 후 기본값으로 self-heal. 저장은 `<name>.<pid>.<seq>.tmp` 작성 → `fs::rename`.
- `layout_adblock.log`: `RotatingLog`가 5MB 초과 시 `.log.1`로 회전(실행 중에도).
- 복원 스냅샷(`EngineCaches.snapshots`)은 **프로세스 메모리에만** 있다(의도된 설계).
- 레지스트리: `HKCU\...\Run\KakaoTalkAdBlockerLayout`, `...\StartupApproved\Run`.

### 외부 의존성
`windows 0.61`, `crossbeam-channel`, `serde/serde_json`, `tracing(-subscriber)`, `clap`, `ureq 2 (tls)`, `ed25519-dalek`, `sha2`, `base64`, `hex`. 네트워크는 업데이트 확인/다운로드(GitHub Releases)에만 쓴다.

### 핵심 실행 흐름

```
[트레이 앱]
main → run_with_args
  → ensure_runtime_files / load_settings / load_rules (self-heal)       [File]
  → InstanceMutex::acquire                                              [Win32 mutex]
  → startup_repair::maybe_repair_startup_registration                   [Registry]
  → spawn_worker ─────────────────────────────────────────────┐
  → tray::run_loop_with_ready (메인 스레드 메시지 루프)          │
       WM_COMMAND → pending 큐 → on_command                      │
         Toggle* → save_settings(atomic) → SharedFlags 갱신      │ [File]
         CheckUpdate → 백그라운드 스레드 → manifest 검증 → staged │ [HTTP]
  ← Exit: stopping=true → worker.join() (무기한) → launch_helper │

[엔진 워커] (worker.rs)                                          ◄┘
loop:
  enabled=false → (직전 ON이면 drain_restore_all) → sleep 1s
  PID: 살아있으면 5s마다, 아니면 pid_scan_interval_ms(200)마다 Toolhelp 스캔
  PID 집합 변경 → EventHook 재설치(PID 한정)
  카카오톡 없음 → 스냅샷 전체 복원 → sleep idle(200ms)
  이벤트 없음 & 주기 미도래 → MsgWait(남은 시간)
  이벤트 있음 → sleep burst_scan_interval_ms(20) → tick
  tick: build_graph(EnumWindows+재귀 EnumChildWindows)          [Win32 read]
        → evaluate_graph_for_apply (순수 판정)                  [core]
        → apply_evaluation: WM_CLOSE(timeout) / SW_HIDE / SetWindowPos  [Win32 write]
        → restore_stale_hidden (2틱 미매칭 → 복원, 실패 시 백오프)
exit: drain_restore_all
```

---

## 3. Audit Coverage & Limitations

### 실제 확인한 주요 모듈 (직접 열람)
- `kakao-app`: `lib.rs`, `args.rs`, `main.rs`, `dialogs.rs`, `dump.rs`, `dump_cmd.rs`, `graph_build.rs`, `observability.rs`, `self_check.rs`(앞부분), `startup.rs`, `startup_repair.rs`, `engine/*` 전부, `config/{settings,storage,log,paths}.rs`, `updater/{http,staging,model,version}.rs`, `updater/manifest.rs`(검증부)
- `kakao-core`: `graph.rs`, `layout.rs`, `signals.rs`, `evaluate/{orchestrate,apply,inspect}.rs`, `rules.rs`(모델·overlay)
- `kakao-win32`: `real.rs`, `event_hook.rs`, `process.rs`, `single_instance.rs`, `startup.rs`(Run 읽기/쓰기), `tray/{host,menu,shell_ready,state,status_text}.rs`, `api.rs`
- `kakao-updater`: `lib.rs`, `main.rs`
- 테스트: `kakao-app/tests/restore_regression.rs`(복원 실패 시나리오), `kakao-win32/src/fake.rs`(`is_window_visible` 의미)
- 빌드/CI: `rust/Cargo.toml`·각 crate `Cargo.toml`, `.github/workflows/windows-ci.yml`(명령 부분), `scripts/build_release.ps1`(cargo 호출부)

### CodeGraph로 분석한 호출 관계
- `codegraph_explore`로 `run_with_args → tick → apply_evaluation` 호출 경로와 `tick`(17 callers), `apply_evaluation`, `restore_stale_hidden`, `SharedFlags`, `install_for_pids`의 blast radius를 확인했다.
- 세부 흐름(워커 스케줄링, 복원 판정, 그래프 수집)은 CodeGraph 결과가 일부 잘려 있어 **대부분 파일 직접 열람으로 확인했다.** CodeGraph는 `legacy/`의 Python 심볼도 노출하므로 활성 런타임 판단은 `rust/`로 한정했다.

### 실행한 테스트
| 명령 | 결과 |
|---|---|
| `cargo test --workspace` | **85 passed, 0 failed** |
| `python -m pytest tests -q --continue-on-collection-errors` | 52 passed, 2 failed, 9 collection errors. **실패/에러 11건 모두 `ModuleNotFoundError: cryptography`** (`requirements.txt`에 있으나 이 환경에 미설치). 감사 목적의 의존성 설치는 하지 않았다 |
| `cargo fmt --check` / `cargo clippy -D warnings` | **실행하지 않음** |

### 실측 (읽기 전용, 실제 사용자 환경)
환경: Windows 11 Home 26200, **Snapdragon X Plus(ARM64)에서 x64 EXE를 에뮬레이션으로 실행**, 8코어, 프로세스 약 396개, 데스크톱 top-level 창 339개, 카카오톡 `26.8.1.5315`(PID 17152) 실행 중, 차단기 `dist\KakaoTalkLayoutAdBlocker_v11.exe`(PID 19976) 약 21분째 상주 중.

| 측정 | 방법 | 결과 |
|---|---|---|
| 차단기 유휴 CPU | 60초 동안 `TotalProcessorTime` 차이 | **390.6ms / 60s = 코어 1개의 0.65%** (8코어 합산 약 0.08%). 프로세스 생애 평균 0.70% |
| 차단기 메모리 | `Get-Process` | Working Set **19.9MB**, Private **11.5MB**, 스레드 2, 핸들 175 |
| 카카오톡 WinEvent 발생량 | PID 한정 `SetWinEventHook`(엔진과 같은 범위·필터) 30초 | **0건** → 엔진은 200ms 유휴 주기로 계속 tick |
| 카카오톡 창 트리 | `--dump-tree` + ctypes 열거 | top-level 34, 전체 노드 **67**, `EnumChildWindows` 반환 항목 합계 103 |
| tick 1회 비용(추정) | CPU 실측 − PID 스냅샷 비용, ÷ 5 tick/s | **약 1.1ms/tick** |
| Toolhelp 스냅샷 1회 | `CreateToolhelp32Snapshot` 20회 중앙값 | **3.3ms** (2.8–4.2ms) |
| `--dump-tree-series` 2s/10ms | 프레임 수 | 112프레임 → 프레임당 약 7.9ms(매 프레임 PID 스냅샷 + JSON 포함) |
| BOM 설정 파일 처리 | 격리 `%APPDATA%` + `--self-check --json` | ISSUE-001 재현 |

### 확인하지 못한 환경/외부 서비스
- 카카오톡을 **응답 없음 상태로 만든 뒤의 종료 동작**(ISSUE-003)은 재현하지 않았다. 사용자 세션의 실제 카카오톡을 조작할 수 없었다.
- 카카오톡 **미실행 상태의 차단기 CPU**(ISSUE-002)는 카카오톡을 종료할 수 없어 종단 간 측정 대신 스냅샷 단가 × 코드상 주기로 산출했다.
- 네이티브 x64 머신 측정 없음. **에뮬레이션은 CPU·Working Set(번역 캐시)을 부풀릴 수 있으므로** 메모리 수치 비교는 참고값이다. EXE 크기와 릴리스 프로파일 부재는 환경과 무관하다.
- GitHub Releases 업데이트 종단 간 흐름, 코드 서명, 실제 로그온 직후 트레이 등록은 실행하지 않았다.

### 분석상의 한계
- `FakeWin32::is_window_visible`은 창 자신의 플래그만 보며 조상 가시성을 반영하지 않는다(`fake.rs:272`). 그래서 ISSUE-004류 문제는 현재 테스트 구조로 드러나지 않는다.
- Win32의 cross-thread 동기 동작(ISSUE-003)은 Microsoft 문서와 알려진 동작에 근거한 판단이다.

---

## 4. High-Risk Issues

### [ISSUE-001] UTF-8 BOM이 붙은 설정/규칙 JSON을 손상으로 판정해 기본값으로 덮어씀

- **위치:** `kakao-app/src/config/storage.rs` `load_json_value`(33), `serde_json::from_str::<Value>(&text)`(40), `heal_default`(75) / 호출: `settings.rs::load_settings`, `storage.rs::load_rules`
- **우선순위:** Medium
- **신뢰도:** Confirmed (재현)
- **문제:** 파일 앞의 U+FEFF(BOM)를 제거하지 않고 `serde_json`에 넘긴다. `serde_json`은 BOM을 허용하지 않아 파싱에 실패하고, 코드는 이를 "JSON 파싱 실패"로 보고 백업을 만든 뒤 **기본값 JSON으로 원본을 덮어쓴다.**
- **발생 조건:** 사용자가 JSON을 BOM을 쓰는 도구로 저장한 경우. 예: Windows PowerShell 5.1의 `Set-Content/Out-File -Encoding utf8`, 메모장의 "UTF-8(BOM)" 저장, 일부 에디터 기본값. README §설정 및 규칙 커스터마이징이 "JSON 규칙 파일 수정만으로 대응"을 권장하므로 현실적인 경로다.
- **영향:** 내용이 정상인 파일인데도 사용자 설정이 기본값으로 초기화된다. 재현에서는 `aggressive_mode:false → true`, `run_on_startup:true → false`로 바뀌었다. 규칙 파일이면 커스텀 규칙이 전부 사라진다. 사용자에게는 로그 경고만 남고 트레이에는 표시되지 않는다(§5 참고). 진단 명령인 `--self-check`만 실행해도 똑같이 덮어쓴다.
- **근거:** 격리 `%APPDATA%`에 `EF BB BF` + 유효 JSON(`enabled:true, aggressive_mode:false, run_on_startup:true`)을 두고 `kakao-adblock-rs.exe --self-check --json`을 실행했다. 결과: `info_warnings`에 "손상 감지: JSON 파싱 실패 … 백업 생성", "자동 복구 성공"이 찍혔고, `layout_settings_v11.json.broken-1790209836`이 생겼으며, 본 파일은 기본값으로 교체되었다.
- **반증 확인:** (1) 상위 호출자(`run_with_args`, `self_check::run`)는 로드 결과를 그대로 쓰며 추가 정규화가 없다. (2) `.broken-*` 백업이 있어 수동 복구는 가능하다. 다만 30일 또는 10개 초과 시 자동 삭제되고, 사용자는 백업이 생긴 사실을 트레이로 알 수 없다. (3) `load_rules`도 같은 `load_json_value`를 쓴다.
- **호출/영향 범위:** `load_json_value` ← `load_settings`, `load_rules` ← `run_with_args`(트레이 시작), `self_check::run`. 결과는 `SharedFlags::from_settings`와 워커 settings/rules 전체에 전파된다.
- **권장 수정 방향:** `read_to_string` 뒤 `text.strip_prefix('\u{feff}')`로 BOM을 제거하고 파싱한다. 알고리즘과는 무관한 입력 정규화다. 저장은 지금처럼 BOM 없는 UTF-8을 유지한다. Python 참고 구현(`utf-8-sig`)과의 정합도 함께 확인한다.
- **필요한 회귀 테스트:** `tests/config_migration.rs`에 추가한다. ① BOM + 유효 settings JSON 로드 시 경고 0건, 필드 값 보존, `.broken-*` 파일 미생성. ② BOM + 유효 rules JSON도 같은 조건. ③ BOM + 실제로 깨진 JSON이면 기존대로 백업·heal.

---

### [ISSUE-002] 카카오톡 미실행 시 전체 프로세스 스냅샷을 200ms마다 반복 (성능)

- **위치:** `kakao-app/src/engine/worker.rs` 58-75(PID 스캔 스케줄), 128-146(카카오톡 없음 분기)
- **우선순위:** Medium (성능·배터리. 기능 오류는 아님)
- **신뢰도:** Confirmed (코드 흐름 + 단가 실측). 종단 간 CPU는 미측정
- **문제:** `pids_alive`는 `!cached_pids.is_empty() && …`라서 카카오톡이 없으면 항상 false다. 그러면 `need_scan = last_pid_scan.elapsed() >= pid_scan_interval`이 되고, `pid_scan_interval = max(pid_scan_interval_ms, 200)`의 기본값이 200ms다. 카카오톡 없음 분기는 `idle_ms`(200ms)만큼 잔 뒤 `continue`하므로, **매 루프(약 200ms)마다 `CreateToolhelp32Snapshot`으로 시스템 전체 프로세스를 열거한다.** 카카오톡이 실행 중이면 이 주기가 5초로 늘어나므로, 카카오톡이 없을 때가 오히려 더 비싸다.
- **발생 조건:** 기본 설정 + 시작프로그램 등록 상태에서 카카오톡을 완전히 종료했거나 아직 실행하지 않은 모든 시간. 상주형 앱의 흔한 상태다.
- **영향:** 이 기기(프로세스 396개)에서 스냅샷 1회에 약 3.3ms가 들어 **약 16ms/s, 코어 1개의 약 1.6%** 가 된다. 초당 5회 타이머 기상으로 CPU 저전력 상태 진입도 방해한다. BENCHMARK의 "카카오톡 미실행 유휴" 수치와 README "유휴 CPU 0.01%대" 주장과 어긋난다.
- **근거:** 코드: `worker.rs:63-73`, `128-146`, `settings.rs` `default_pid_scan() = 200`. 실측: `CreateToolhelp32Snapshot` 중앙값 3.3ms(20회).
- **반증 확인:** (1) `enabled=false`이면 스캔 전에 `continue`하므로 해당 없음이다. OFF 상태는 1초 sleep만 한다. (2) 훅은 카카오톡 PID가 있어야 설치되므로(`worker.rs:91`) 카카오톡이 없을 때 이벤트로 깨울 수단이 없다. 폴링 외 대안이 없는 구조다. (3) `pid_scan_interval_ms`를 사용자가 올리면 완화되지만 기본값과 README 예시가 200이다.
- **호출/영향 범위:** `spawn_worker` 루프 단독. `process::kakaotalk_pids()`는 `dump_cmd`도 쓰지만 진단 경로라 무관하다.
- **권장 수정 방향:** 카카오톡 부재 시 스캔 간격을 별도로 두거나(예: 1–2초), 부재 지속 시간에 따라 백오프한다(200ms → 1s → 2s 상한). 카카오톡 시작 직후의 첫 광고 표시는 네트워크 로딩 후라 1–2초 탐지 지연의 체감 영향은 작다. 다만 "idle→active 복귀 약 200ms" Python 계약과의 관계를 문서에 명시해야 한다. 알고리즘 판정 로직은 건드리지 않는다.
- **필요한 회귀 테스트:** 스캔 스케줄 결정을 순수 함수(`next_pid_scan_due(pids_empty, elapsed, settings, absent_for)`)로 추출하고 단위 테스트한다. ① pids 비어 있음 + 부재 10초 → 다음 스캔까지 ≥1s. ② pids 살아 있음 → 5s. ③ pids 사망 감지 → 즉시 스캔.

---

### [ISSUE-003] 카카오톡이 응답 없음일 때 종료/비활성화가 무기한 멈추고, 보이지 않는 프로세스가 mutex를 점유

- **위치:** `kakao-app/src/engine/restore.rs` `restore_snapshot`(35: `set_window_pos` 43, `show_window(SW_SHOW)` 57) ← `restore_all` ← `EngineCaches::drain_restore_all` ← `worker.rs` 종료 경로(202-205)·OFF 전환(45-48) / `kakao-app/src/lib.rs` `worker.join()`(353, 330) / `_instance_guard`(99)
- **우선순위:** Medium
- **신뢰도:** Likely (런타임 재현 안 함)
- **문제:** 복원은 다른 프로세스(카카오톡) UI 스레드 소유 창에 `SetWindowPos`(SWP_ASYNCWINDOWPOS 없음)와 `ShowWindow(SW_SHOW)`를 **동기로** 호출한다. 두 API는 대상 스레드가 메시지를 처리할 때까지 호출 스레드를 막는다. Microsoft 문서는 이를 피하려면 `ShowWindowAsync`나 `SWP_ASYNCWINDOWPOS`를 쓰라고 안내한다. 메인 스레드는 `worker.join()`을 타임아웃 없이 기다린다.
- **발생 조건:** 숨긴 광고 창이 있는 상태(평상시 항상 1개 이상)에서 카카오톡 UI 스레드가 응답 없음일 때 트레이 "종료"나 업데이트 적용을 누르는 경우. "차단 끄기"도 워커를 같은 방식으로 멈춘다.
- **영향:** `WM_DESTROY`에서 트레이 아이콘은 이미 제거되지만(`host.rs:250-257`), 프로세스는 카카오톡이 회복되거나 죽을 때까지 남는다. 그동안 단일 인스턴스 mutex가 유지되므로 재실행하면 "프로그램이 이미 실행 중입니다"만 뜬다. 업데이트를 수락한 경우 `launch_helper`가 join 이후에 있어 업데이트도 진행되지 않는다. 사용자는 작업 관리자로 강제 종료해야 한다.
- **근거:** `restore.rs:43-60`, `lib.rs:350-377`. Python 계약(CLAUDE.md)에는 "`stop()` join timeout(2.0s) 시 상태/로그 경고 후 종료 절차 계속"이 있지만 Rust에는 없다. CLAUDE.md의 "Rust 활성 런타임 동작" 절에도 이 차이가 적혀 있지 않다.
- **반증 확인:** (1) popup `WM_CLOSE`는 `SendMessageTimeoutW(SMTO_ABORTIFHUNG, 500ms)`로 보호되지만 복원 경로는 아니다. (2) 이미 숨겨진 창에 대한 매 tick `SW_HIDE`는 조기 반환하므로 평시에 블로킹할 가능성은 낮다. 위험은 **상태를 실제로 바꾸는 호출(복원, 첫 숨김, 리사이즈)** 에 몰려 있고, 종료 복원은 그중 확정적으로 실행되는 경로다. (3) 프로세스가 끝나면 막힌 스레드도 함께 정리된다. 하지만 지금은 메인 스레드가 join에서 막혀 프로세스가 끝나지 않는다.
- **호출/영향 범위:** `drain_restore_all` ← `worker.rs`(OFF 전환, 카카오톡 종료, 워커 종료) / `restore_stale_hidden` ← `tick`. `worker.join()` ← `run_with_args` 종료부·트레이 실패부. 업데이트 경로(`pending_update`)도 이 순서에 묶여 있다.
- **권장 수정 방향:** ① 복원·변이 전에 대상 창의 top-level 루트에 `IsHungAppWindow`를 확인하고, hung이면 이번 시도를 실패로 기록한 뒤 건너뛴다(스냅샷 유지). ② 메인 스레드는 `JoinHandle`을 채널/`park_timeout`으로 감싸 bounded join(예: 3초)을 하고, 초과하면 경고 로그를 남기고 mutex를 풀고 프로세스를 종료한다(업데이트 헬퍼는 그 후 실행). ③ 비동기 API(`ShowWindowAsync`)로 바꾸는 방법도 있으나, 복원 검증 로직이 동기 결과에 의존하므로 ①+②가 변경 범위가 작다.
- **필요한 회귀 테스트:** `FakeWin32`에 "지정 hwnd의 `show_window`/`set_window_pos`가 N초 블록" 훅을 추가한다. ① 종료 시퀀스(`stopping=true` → join)가 3초 안에 반환되고 경고 로그가 남는지. ② hung으로 표시된 창은 복원 시도 없이 스냅샷이 유지되는지. ③ pending update가 있으면 bounded join 후 `launch_helper`가 호출되는지.

---

### [ISSUE-004] 복원 성공 판정에 `IsWindowVisible`을 써서, 조상이 숨겨진 자식 창의 정상 복원을 "실패"로 집계

- **위치:** `kakao-app/src/engine/restore.rs` `restore_snapshot` 56-61
- **우선순위:** Low
- **신뢰도:** Likely (Win32 문서 근거, 런타임 재현 안 함)
- **문제:** `ShowWindow(SW_SHOW)` 뒤 `IsWindowVisible(hwnd)`로 성공을 확인한다. `IsWindowVisible`은 **자신과 모든 부모 체인이 `WS_VISIBLE`일 때만** 참이다. 부모가 숨겨진 자식 창은 `WS_VISIBLE`이 정상적으로 켜져도 거짓이 되고, 코드는 이를 실패로 기록한다. `restore.rs:74-79` 주석("A window whose ancestors are hidden … can refuse to become visible")은 이 현상을 "창이 거부한다"로 해석했지만, 실제로는 판정 함수가 부적절한 것이다.
- **발생 조건:** (a) popup이 `WM_CLOSE`를 거부해 host와 descendant(`AdFitWebView`) 모두 스냅샷된 상태에서 차단 OFF/종료가 일어나는 경우. `restore_all`은 `HashMap::drain` 순서로 복원하므로 자식이 부모보다 먼저 복원되면 비결정적으로 "실패"가 난다. (b) aggressive 모드로 숨긴 메인창 자식이 있는데 카카오톡 메인창이 트레이로 숨겨진 상태에서 OFF가 되는 경우.
- **영향:** 기능상 창은 제대로 복원된다. 그러나 `restore_failures` 게이지와 `last_error`가 켜지고 "restore on disable had failures" 경고가 남는다. 차단 OFF 상태에서는 워커가 재시도하지 않으므로(`worker.rs:50-53`) 트레이의 "복원 실패 N건"이 **다시 켤 때까지 사라지지 않는다.** 불필요한 백오프 재시도(`SW_SHOW`)도 생긴다.
- **근거:** `restore.rs:56-61`, `IsWindowVisible` 문서("If the specified window, its parent window, its parent's parent window, and so forth, have the WS_VISIBLE style, the return value is nonzero"), `fake.rs:272-280`(조상 무시 → 테스트로 검출 불가).
- **반증 확인:** 현재 카카오톡 26.8의 주 광고 호스트는 **owned top-level popup**이다. owner는 부모 체인에 포함되지 않으므로 이 경우는 오판하지 않는다. 그래서 영향은 위 (a)(b) 조건에 한정되어 Low로 둔다.
- **호출/영향 범위:** `restore_snapshot` ← `restore_all`(OFF/종료/카카오톡 종료), `restore_stale_hidden`(tick). 결과는 `SharedFlags.restore_failures`/`last_error` → 트레이 툴팁·메뉴.
- **권장 수정 방향:** 성공 판정을 `GetWindowLongPtrW(GWL_STYLE) & WS_VISIBLE`(창 자신의 스타일)로 바꾼다. `Win32Api`에 `has_visible_style(hwnd)`를 추가하고 `FakeWin32`에서도 자신의 플래그를 반환한다. 부모부터 복원하도록 정렬(top-level 우선)하면 로그 소음도 준다.
- **필요한 회귀 테스트:** `FakeWin32`에 부모 체인을 반영한 `is_window_visible`을 구현한 뒤 추가한다. ① host와 child 모두 숨김 → `restore_all` → 실패 0, 둘 다 `WS_VISIBLE`. ② 부모만 숨긴 상태에서 자식 복원 → 실패 0, `last_error` 빈 문자열.

---

### [ISSUE-005] 복원이 회복된 뒤에도 `last_error`가 남아 트레이에 오래된 오류가 계속 표시됨

- **위치:** `kakao-app/src/engine/tick.rs` 63-70, `restore.rs::report_restore` 138-144, `worker.rs` 39-43
- **우선순위:** Low
- **신뢰도:** Confirmed (코드 흐름)
- **문제:** `set_last_error`는 실패가 있을 때만 호출되고, 실패가 해소되어 `restore_failures`가 0으로 돌아가도 비워지지 않는다. 비우는 곳은 사용자가 "복원 실패 초기화"를 누를 때(`worker.rs:42`)뿐이다.
- **발생 조건:** 복원 실패가 한 번이라도 생긴 뒤 자연 회복된 경우(ISSUE-004의 오판 포함).
- **영향:** 메뉴 헤더에 "오류: restore show failed hwnd=…"가 무기한 표시되고, "복원 실패 초기화" 항목도 계속 활성화된다(`menu.rs:60-66`의 활성 조건이 `last_error` 비어 있지 않음). 정상 상태를 오류 상태로 보이게 한다.
- **근거:** 위 위치. `status_text.rs` `status_menu_lines`가 `last_error`를 그대로 표시한다.
- **반증 확인:** `restore_failures` 게이지는 매 tick 갱신되어 0이 된다. 문제는 `last_error`에만 있다. 다른 경로에서 `set_last_error("")`를 부르는 곳은 없다(grep 확인).
- **호출/영향 범위:** `SharedFlags.last_error` → `TrayStatus.last_error` → 툴팁/메뉴.
- **권장 수정 방향:** 게이지가 0으로 떨어질 때(이번 tick `failures == 0` & 직전 > 0) 복원 계열 `last_error`를 비운다. 오류 출처를 구분하는 enum을 두면 다른 오류를 지우지 않는다.
- **필요한 회귀 테스트:** `restore_regression.rs`의 `recovered_window_clears_the_failure_gauge_and_restores`에 `flags.last_error_text().is_empty()` 단언을 추가한다(`clear_restore_failures` 없이 자연 회복하는 변형 포함).

---

## 4A. Performance & Optimization Review (추가 요청)

> 원칙: 판정 알고리즘(CLAUDE.md "고정 계약")은 바꾸지 않고, **스케줄링·수집·할당** 계층만 대상으로 한다. 절대 비용은 작다. 우선순위는 "상주 앱이 하루 종일 쓰는 비용"을 기준으로 매겼다.

### 실측 요약
| 상태 | 주요 비용원 | 수치 |
|---|---|---|
| 카카오톡 실행·유휴(이벤트 0건) | 200ms 주기 tick(약 1.1ms) + 5초 주기 Toolhelp(3.3ms) | **코어 1개의 0.65%** (8코어 합산 약 0.08%), 초당 약 5회 기상 |
| 카카오톡 미실행 | 200ms 주기 Toolhelp | 산출값 약 1.6% (ISSUE-002) |
| 카카오톡 사용 중(창 이동/리사이즈) | 이벤트 배치마다 20ms sleep + tick | 초당 최대 약 45 tick × 1.1ms ≈ 5% (추정) |

tick 1회 비용 구성(코드 기준, 이 기기): `EnumWindows`가 데스크톱 전체 **339개** top-level을 훑으며 창마다 `GetWindowThreadProcessId`를 호출하고, 카카오톡 노드 67개 × 약 7개 user32 호출(`IsWindow`, `GetClassName`, `GetWindowTextLength/Text`, `GetParent`, `GetWindowRect`, `IsWindowVisible`), 노드마다 `EnumChildWindows` 1회, 열거된 자손 103건마다 `GetParent` 2회(중복)가 더해진다. 그다음 평가(수십 µs 수준)와 apply precheck가 이어진다.

### 최적화 기회 (우선순위순)

| # | 항목 | 근거 | 기대 효과 | 위험/주의 |
|---|---|---|---|---|
| P1 | **카카오톡 부재 시 PID 스캔 백오프** | ISSUE-002 | 미실행 상태 CPU 약 1.6% → 0.2% 이하 | 카카오톡 시작 후 첫 적용이 최대 1–2초 늦어짐 |
| P2 | **유휴 reconciliation 적응형 연장** (예: 액션·이벤트 없는 tick이 N회 이어지면 200ms → 1s, 이벤트/액션 발생 시 즉시 복귀) | 실측: 유휴 카카오톡의 WinEvent 0건/30초인데도 초당 5회 풀스캔 | 유휴 CPU 약 60–80% 절감, 기상 횟수 감소 | 훅 누락 시 복구 지연이 200ms → 1s로 늘어남. Python 계약 "idle→active 약 200ms"와 README 수치를 함께 고쳐야 함 |
| P3 | `build_graph` 자식 수집을 **top-level당 `EnumChildWindows` 1회 + parent map**으로 | `graph_build.rs:62-71`이 노드마다 전체 자손을 다시 열거하고, `real.rs:67`과 `graph_build.rs:68`에서 `GetParent`를 이중 호출 | O(N·depth) → O(N). 현재 N=67이라 절감은 작음(열거 103 → 약 67, GetParent 206 → 약 67) | 순수 수집 계층. `graph_direct_children.rs`·`scan_parity.rs`로 동일성 검증 가능 |
| P4 | `EnumWindows` 전체 순회 대신 **카카오톡 top-level 집합 캐시**(훅의 CREATE/DESTROY나 N tick마다 재동기화) | tick마다 339개 전부를 훑어 34개만 사용 | tick당 user32 호출 약 340회 절감 | 캐시 무효화 누락 시 새 광고 창을 늦게 봄. 주기적 전체 재동기화 필수 |
| P5 | apply 경로의 **중복 스냅샷 캡처 제거** | `apply.rs:87-89`가 이미 스냅샷이 있는 창에도 매 tick `IsWindowVisible`+`GetWindowRect`를 호출한 뒤 `or_insert`로 버림 | 숨긴 창당 tick마다 호출 2회 절감 | `contains_key` 선확인만 하면 됨. 의미 불변 |
| P6 | **평가 계층 할당 제거**: `contains_ad_token`이 노드마다 `aggressive_ad_tokens_lc()`로 `Vec<String>`을 새로 만들고(`layout.rs:7`, `rules.rs:91`), 노드마다 `to_lowercase` + `HashSet` 생성. `has_window_text_contains`는 재귀 단계마다 needle을 다시 lowercase. `legacy_signature_kind`는 top-level 후보마다 `candidate_handles`와 `apply_once`에서 두 번 계산 | 코드 | µs 단위. 우선순위 낮음 | lowercase 토큰은 rules 로드 시 1회 계산해 보관한다(판정 결과 불변). 결과 동일성은 golden/`apply_path_parity`로 고정 |
| P7 | **이벤트 병합(coalescing)이 실제로 동작하지 않음** | `worker.rs:163-179`: `thread::sleep` 중에는 메시지 펌프가 없어 훅 콜백이 실행되지 않으므로 `extra = hook.drain()`은 항상 비어 있다. sleep 중 도착한 이벤트는 다음 `wait_message`에서 전달되어 **tick을 한 번 더 유발한다** | 이벤트 버스트당 tick 1회 절감 | sleep 대신 `wait_message(burst_interval)`로 펌프하며 대기 후 drain |
| P8 | 카카오톡 실행 중 5초 주기 Toolhelp 재동기화 | `worker.rs:60`. 유휴 CPU의 약 10% | 30초로 늘리면 약 8% 절감 | 두 번째 카카오톡 프로세스 탐지 지연. PID liveness는 그대로 유지됨 |
| P9 | **릴리스 프로파일 부재** | `rust/Cargo.toml`에 `[profile.release]`가 없음 → LTO/`codegen-units=1`/`strip` 미적용, EXE 4.5MB | EXE 크기·기동·코드 크기 개선(문서상 1.8MB에 근접 기대) | `panic = "abort"`는 워커 panic 시 프로세스를 종료시키는 **의미 변경**이다(§5 워커 감시와 함께 결정) |
| P10 | 워커 루프마다 PID별 `OpenProcess`/`CloseHandle` liveness 확인 | `worker.rs:63-66` | 미미 | 프로세스 핸들을 보관하고 `WaitForSingleObject(h, 0)` 사용 |

**반응 지연 관련 주의:** 이벤트 수신 후 첫 tick 전에 `burst_scan_interval_ms`(기본 20ms)를 무조건 잔다(`worker.rs:163-166`). 또 훅 콜백은 워커가 `wait_message`로 펌프할 때만 실행된다. 따라서 창 생성 → 숨김 지연의 하한은 약 20ms + tick 시간이다. README의 "5ms 이내" 주장과 맞지 않는다(§6).

**권장 방향:** P1·P2·P5·P7은 판정 의미를 바꾸지 않는 스케줄링·수집 개선이라 먼저 한다. P3·P4는 `scan_parity`/`graph_direct_children` 회귀로 동일성을 고정한 뒤 진행한다. 성능 주장(README/BENCHMARK)은 개선 후 **네이티브 x64에서 재측정**해 갱신한다.

---

## 5. Potential Functional Gaps

### Confirmed Gap — 시작 시 설정 로드 경고(self-heal, 백업 생성)가 사용자에게 보이지 않음
`lib.rs:66-72`는 bootstrap/settings/rules 경고를 `tracing::warn!`으로 로그에만 남긴다. `flags.set_last_error`로 트레이 상태에 올리지 않는다. Python 계약(`report_warning()`, "복구 실패 > 자동 복구 > 기타" 우선순위 1건 노출)과 다르고, CLAUDE.md의 "Rust 활성 런타임 동작" 차이 목록에도 없다. ISSUE-001처럼 설정이 초기화되어도 사용자는 알 수 없다.

### Confirmed Gap — 시작 시 `run_on_startup`을 레지스트리 실제 상태와 동기화하지 않음
`flags.startup`은 설정값으로만 초기화된다(`flags.rs:34`). `startup_repair`는 `run_on_startup=true`일 때만 동작한다. 설정이 기본값으로 heal되었거나(ISSUE-001) 수동으로 바뀌어 `false`인데 Run 등록이 남아 있으면, 트레이 체크 표시는 꺼져 있는데 앱은 로그온 시 자동 실행된다. Python 계약("시작 시 `run_on_startup` 값을 레지스트리 상태로 1회 동기화")과 다르다.

### Likely Gap — 업데이트 교체 실패 시 앱이 재실행되지 않음
`kakao-updater` `update_executable_with`는 백업/교체 실패 시 롤백하고 오류 대화상자를 띄운 뒤 종료한다(`lib.rs:163-196`). 이때 **이전 버전을 다시 실행하지 않는다.** 원래 앱은 이미 종료된 상태라, 쓰기 권한이 없는 위치(예: `C:\Program Files`)에 둔 사용자는 업데이트 실패 후 차단기가 꺼진 채로 남는다. `%TEMP%`에 staged 헬퍼/아티팩트도 남는다.

### Likely Gap — 워커 스레드 생존 감시 없음
워커가 panic하면(현재 코드에서 현실적인 panic 경로는 찾지 못했다) 트레이는 계속 떠 있지만 차단은 멈추고, 이 사실은 종료 시 join에서야 로그로 드러난다. 트레이 상태 타이머(1초)에서 `JoinHandle::is_finished()`를 확인해 `last_error`에 올리거나 재기동하는 장치가 없다. P9에서 `panic="abort"`를 택하면 이 문제는 "프로세스 종료"로 바뀐다.

### 추정 — 설정 저장 시 fsync 없음
`atomic_write`는 `flush()`(OS 버퍼로만 전달) 후 `rename`한다(`storage.rs:107-114`). `sync_all()`이 없어 저장 직후 전원이 끊기면 NTFS에서 빈/부분 파일이 남을 수 있다. 다음 기동 시 self-heal로 기본값이 된다. 발생 확률이 낮고 영향은 설정 1회 초기화다.

### 추정 — 트레이 토글 저장이 외부 편집을 덮어씀
트레이 클로저는 시작 시점의 `AppSettings` 사본을 들고 있다가 토글 때 통째로 저장한다(`lib.rs:178-226`). 실행 중 사용자가 JSON을 직접 고친 값(예: `idle_poll_interval_ms`)과 알 수 없는 키는 다음 토글에서 사라진다. `--startup-launch`로 강제된 `start_minimized=true`도 함께 기록된다(Rust에서는 미사용 필드라 기능 영향 없음).

### 추정 — 차단 OFF 동안 WinEvent 메시지 적체
OFF 상태에서 워커는 훅을 유지한 채 `thread::sleep(1s)`만 하고 메시지를 펌프하지 않는다(`worker.rs:50-53`). out-of-context WinEvent는 스레드 큐에 쌓였다가 재활성화 시 한꺼번에 전달된다. 적체 한도와 영향은 확인하지 못했다. OFF 전환 시 훅을 해제하면 이 불확실성이 사라진다.

---

## 6. Documentation Mismatches

| # | 문서 주장 | 실제 | 근거 |
|---|---|---|---|
| D1 | README 배지·본문 "유휴 CPU **0.01%대/0.01% 미만**", BENCHMARK "10분 유휴 CPU **0.0%**" | 카카오톡 실행·유휴 상태 **코어 1개 0.65%(8코어 합산 약 0.08%)**, 카카오톡 미실행 시 산출값 약 1.6%(ISSUE-002) | §3 실측. 측정은 ARM64 에뮬레이션 환경이라 네이티브 x64는 더 낮을 수 있지만, 초당 5회 풀스캔 구조상 "측정 불가 수준"은 아님 |
| D2 | BENCHMARK "EXE 약 **1.8MB**, release build with **LTO/opt-level 3**" | `dist\KakaoTalkLayoutAdBlocker_v11.exe` **4,510,720 bytes**, `[profile.release]` 없음(LTO 미적용) | `rust/Cargo.toml`, 파일 크기 |
| D3 | README "메모리 **4~7MB**", BENCHMARK Working Set 5.2–7.1MB / Private 3.2MB | Working Set **19.9MB**, Private **11.5MB** | §3 실측. **에뮬레이션 번역 캐시로 부풀었을 수 있음 — 네이티브 재측정 필요** |
| D4 | README·BENCHMARK "창 생성 → 광고 제거 **5ms 이내**" | 이벤트 수신 후 무조건 `burst_scan_interval_ms`(기본 20ms) sleep 후 tick. 하한 약 20ms + tick | `worker.rs:163-166` |
| D5 | README "메인 윈도우 **O(1) 캐싱**" | 메인 윈도우 캐시 없음. 매 tick 그래프를 새로 만들고 `inspect_main_windows`로 다시 식별 | `tick.rs:33`, `evaluate/orchestrate.rs:55` |
| D6 | CLAUDE.md Python 계약: `stop()` join timeout 2.0s, 시작 경고 `last_error` 반영, 시작 시 Run 상태 동기화 | Rust는 셋 다 없다. "Rust 활성 런타임 동작" 차이 목록에도 빠져 있다 | ISSUE-003, §5 |
| D7 | `restore.rs:74-79` 주석 "조상이 숨겨진 창은 다시 보이기를 거부한다" | 실제로는 `IsWindowVisible` 판정 특성 때문이다 | ISSUE-004 |

그 외 CLAUDE.md "Rust 활성 런타임 동작" 절의 항목(백업 파일명, 트레이 상태 노출, 로그 회전, PID 한정 훅, apply 경로 분리, `poll_interval_ms` 의미, self-check 분리)은 코드와 일치함을 확인했다.

---

## 7. Recommended Fix Plan

### Phase 1 — Immediate
1. **ISSUE-001**: `load_json_value`에서 BOM 제거 후 파싱 + 회귀 테스트 3건.
2. **ISSUE-003**: 메인 스레드 bounded join(약 3초) → 초과 시 경고 후 종료. 복원·변이 전 `IsHungAppWindow` 가드.
3. **ISSUE-002 / P1**: 카카오톡 부재 시 PID 스캔 백오프(1–2초 상한).

### Phase 2 — Stability
4. **ISSUE-004**: 복원 성공 판정을 `WS_VISIBLE` 스타일 기준으로 변경, top-level 우선 복원 순서, `FakeWin32` 조상 가시성 반영.
5. **ISSUE-005**: 복원 게이지가 0이 되면 복원 계열 `last_error` 해제.
6. 시작 경고를 트레이 `last_error`에 우선순위 1건 노출(§5), `run_on_startup` ↔ Run 상태 동기화.
7. 업데이트 헬퍼: 교체 실패·롤백 후 이전 EXE 재실행, staged 파일 정리.
8. `atomic_write`에 `sync_all()` 추가. OFF 전환 시 WinEvent 훅 해제.

### Phase 3 — Structural / Performance
9. P2 적응형 유휴 주기, P5 중복 스냅샷 제거, P7 이벤트 병합 수정. 판정 의미가 불변임을 `apply_path_parity`/`scan_parity`로 확인.
10. P3/P4 그래프 수집 O(N)화와 top-level 캐시. 동일성 회귀를 먼저 고정한다.
11. P6 lowercase 토큰 사전 계산(rules 로드 시).
12. P9 `[profile.release]`(lto, codegen-units=1, strip) 도입. `panic` 전략은 워커 감시 설계와 함께 결정.
13. 네이티브 x64에서 성능 재측정 후 README/BENCHMARK 수치 갱신(D1–D5). CLAUDE.md에 Rust/Python 차이(D6) 기록.

---

## 8. Test Recommendations

### Unit
- `load_json_value`: 입력 `EF BB BF` + `{"aggressive_mode": false}` → `aggressive_mode == false`, 경고 0, `.broken-*` 없음. 입력 `EF BB BF` + `{` → 백업 1개, 기본값, 경고 2개.
- PID 스캔 스케줄 순수 함수: `(pids=[], absent_for=10s)` → 다음 스캔 ≥1s; `(pids=[alive])` → 5s; `(pids=[dead])` → 즉시.
- `retry_cooldown_ticks`/`restore_snapshot`: `has_visible_style=true`, `is_window_visible=false`(조상 숨김) → 성공 반환.
- `status_menu_lines`: `restore_failures=0`, `last_error=""` → "오류:" 줄 없음, 초기화 항목 비활성.

### Integration (FakeWin32)
- 조상 가시성을 반영한 fake에서 popup host + `AdFitWebView` child 숨김 → 차단 OFF → `flags.restore_failures == 0`, 두 창 모두 `WS_VISIBLE`. `HashMap` 순서 영향을 없애기 위해 100회 반복.
- 복원 실패 → 자연 회복(`clear_restore_failures` 없이) → `last_error_text()` 빈 문자열.
- `apply_evaluation`을 3 tick 반복할 때 이미 스냅샷된 hwnd에 대한 `get_window_rect` 호출 수가 1회에 머무는지(P5).

### End-to-End
- 격리 `%APPDATA%`에 BOM 파일을 두고 `--self-check --json` → `info_warnings` 비어 있음, 원본 파일 바이트 불변.
- 실제 카카오톡: `--dump-tree` → `candidates`에 owned `EVA_Window_Dblclk` `strong/hide` 존재(현 26.8.1.5315 기준 확인됨). 카카오톡 버전 갱신마다 반복한다.
- 트레이 앱 실행 → 종료 → 5초 안에 프로세스가 사라지고 mutex가 풀리는지(연속 재실행 성공).

### Concurrency
- fake `show_window`가 10초 블록하도록 설정 → `stopping=true` → `run_with_args` 종료 시퀀스가 3초 안에 반환되고 "worker join timed out" 경고가 남는지(ISSUE-003).
- `--self-check`와 트레이 토글 저장을 동시에 100회 → 설정 파일이 항상 유효 JSON이고 `.tmp` 잔여물이 없는지.

### Regression
- 기존 `apply_path_parity.rs`(fixture 10종 × 4 tick)와 `golden_parity.rs`를 P3/P4/P6 변경 전후로 그대로 통과해야 한다(판정 불변 보증).
- `owned_popup_legacy_ad.json` 골든은 수집 계층을 바꿔도 동일 결과여야 한다.

### Platform-specific
- **네이티브 x64**와 **ARM64 에뮬레이션**에서 60초 유휴 CPU/Working Set을 측정해 문서 수치의 근거로 기록한다(카카오톡 실행·미실행 각각).
- 카카오톡 "응답 없음" 재현(디버거로 UI 스레드 일시정지) 상태에서 트레이 종료 → 프로세스 종료 시간 측정.
- 메인창을 트레이로 숨긴 상태에서 차단 OFF → 트레이 "복원 실패" 미표시 확인.

---

## 9. Final Assessment

| 항목 | 평가 | 근거 |
|---|---|---|
| Functional Correctness | **Good** | 핵심 판정/적용/복원 경로가 일관되고 golden·parity 테스트가 통과한다. 실제 카카오톡 26.8에서 광고 호스트 판정 정상 |
| Runtime Stability | **Acceptable** | 로그 회전·복원 백오프·PID 한정 훅은 해결됨. hung 카카오톡 종료 경로(ISSUE-003)가 남음 |
| Data Integrity | **Acceptable** | 원자 저장·백업·서명 업데이트는 양호. BOM 파일을 기본값으로 덮어쓰는 문제(ISSUE-001) |
| Error Resilience | **Acceptable** | 필드 단위 병합, 트레이 재시도, 업데이트 롤백은 좋음. 시작 경고·복원 오류 표시가 부정확함(ISSUE-004/005, §5) |
| Cross-platform Robustness | **Good** (범위 내) | 의도적으로 Windows 전용이며 비Windows는 종료 코드 2로 fail-fast. Windows 내 인코딩(BOM) 처리 약점만 있음 |
| Test Confidence | **Acceptable** | Rust 85건 통과. 다만 `FakeWin32` 가시성 모델이 단순해 ISSUE-004류를 못 잡고, 스케줄링/성능/종료 타임아웃 테스트가 없다. Python 스위트는 이 환경에서 `cryptography` 미설치로 일부만 실행 |
| Performance / Resource Efficiency | **Acceptable** | 절대 비용은 작다(유휴 코어 1개의 0.65%). 그러나 미실행 시 200ms 스냅샷, 고정 5Hz 풀스캔, 릴리스 최적화 부재가 있고 문서 주장이 과장되어 있다 |

### 실제로 먼저 수정할 문제 3개
1. **[ISSUE-001] BOM 설정/규칙 파일 덮어쓰기.** 재현된 사용자 설정 초기화 경로이고 수정이 한 줄 수준이며 위험이 없다.
2. **[ISSUE-003] hung 카카오톡에서 종료 무기한 대기 + mutex 점유.** 발생 시 사용자가 스스로 복구할 수 없다(보이지 않는 프로세스, 재실행 거부, 업데이트 중단).
3. **[ISSUE-002] 카카오톡 미실행 시 200ms 프로세스 스냅샷.** 상주 앱의 가장 흔한 상태에서 CPU를 가장 많이 쓰는 역전 현상이다. 함께 README/BENCHMARK 성능 수치를 실측 기반으로 바로잡는다(D1–D5).

---

## 10. 부록 — 이전 감사(2026-09-12, v11.1.3) 이슈의 현재 상태

| 이전 이슈 | 현재 상태 | 확인 근거 |
|---|---|---|
| ISSUE-001 로그 회전 시작 시 1회 | 해소 | `config/log.rs` `RotatingLog::write_record`가 기록 중 회전, `observability::init_tracing`이 사용 |
| ISSUE-002 복원 재시도 폭주 | 해소 (단, 실패 판정 자체의 오판은 신규 ISSUE-004) | `restore.rs` `retry_cooldown_ticks`, 창당 1회 경고, `restore_failure_count` 게이지 |
| ISSUE-003 효과 없는 설정 3종 | 해소 | `worker.rs:119-125`(`poll_interval_ms` 활성 주기), `tick.rs:47-51`(`cache_cleanup_interval_ms`) |
| ISSUE-004 tick당 2배 평가 | 해소 | `tick.rs:37-41` `evaluate_graph_for_apply`, `apply_path_parity.rs` |
| ISSUE-005 중복 실행 무반응 | 해소 | `lib.rs:110` `std::env::args().skip(1)` |
| ISSUE-006 전역 WinEvent 훅 | 해소 | `event_hook.rs` `install_for_pids`(PID 한정), `worker.rs:89-98` 재설치 |
