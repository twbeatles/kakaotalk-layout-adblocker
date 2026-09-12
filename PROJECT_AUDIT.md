# Project Audit

- **감사 일자:** 2026-09-12 (Asia/Seoul)
- **대상:** KakaoTalk Layout AdBlocker **v11.1.3** (Rust 네이티브) / branch `main` @ `0f007c2`
- **감사 방식:** 문서(README/CLAUDE/GEMINI) → CodeGraph MCP 구조 파악 → Rust 런타임 전 소스 직접 열람 → `cargo test --workspace` 실행 → 반증 확인 후 잔존 이슈만 기록
- **코드 변경:** 없음. 본 문서만 작성했다.
- **이전 감사 문서 처리:** 본 파일은 v11.1.1 시점 감사본을 대체한다. 당시 ISSUE-001~009는 현재 소스에서 모두 수정된 것을 재확인했으며, 요약을 §10에 남겼다.

---

## 0. Remediation status (v11.1.4, 2026-09-12)

본 감사 직후 §4~§7의 지적 사항을 구현했다. **아래 §1~§9 본문은 수정 이전(v11.1.3) 시점의 감사 기록으로 유지한다.**

| 항목 | v11.1.4 상태 | 반영 위치 |
|---|---|---|
| ISSUE-001 로그 회전 1회 | 수정. `RotatingLog`가 기록 중에도 5MB 초과 시 회전. 열린 핸들 문제까지 해소 | `kakao-app/src/config.rs`, `lib.rs` `init_tracing` |
| ISSUE-002 복원 재시도 폭주 | 수정. 지수 백오프(1→2→4→8 tick → 약 60초), 창당 경고 1회, `restore_failures`를 게이지로 재정의 | `kakao-app/src/engine.rs` `restore_stale_hidden` / `StaleState` |
| ISSUE-003 효과 없는 설정 3종 | 수정. `poll_interval_ms`=활성 재확인 주기, `cache_cleanup_interval_ms`=캐시 정리 주기로 실제 연결. `start_minimized`는 미사용임을 README에 명시 | `engine.rs` 워커 루프, `README.md` |
| ISSUE-004 tick당 2배 평가 | 수정. `evaluate_graph_for_apply` 분리. 두 경로 동일성 회귀 테스트 추가 | `kakao-core/src/evaluate.rs`, `tests/apply_path_parity.rs` |
| ISSUE-005 중복 실행 무반응 | 수정. `std::env::args().skip(1)`로 정규화 | `kakao-app/src/lib.rs:208` |
| ISSUE-006 전역 WinEvent 훅 | 수정. 카카오톡 PID로 범위 한정 + PID 변경 시 재설치(`OnceLock` → `Mutex<Option<Sender>>`) | `kakao-win32/src/event_hook.rs` |
| §5 복원 실패 미노출 | 수정. 메뉴 헤더 + `NIF_TIP` 툴팁에 상태 표시 | `kakao-win32/src/tray.rs` |
| §5 `closed_windows` 항상 0 | 수정. 평가 계층은 close 요청 수, 엔진 계층은 소멸 확인 수를 집계 | `evaluate.rs` `send_close`, `engine.rs` `apply_evaluation` |
| §5 popup dismiss 미검증 | 수정. 소멸/거부/미전달을 `DEBUG`로 기록(매 tick 경로라 `WARN` 아님). fallback 동작은 기존과 동일 | `engine.rs` `apply_evaluation` |
| §5 `--self-check` 점검 부족 | 수정. Run 레지스트리 읽기/쓰기, 등록 명령 health, 프로세스 열거 추가 | `kakao-app/src/self_check.rs`, `kakao-win32/src/{startup,process}.rs` |
| §5 strict가 양성 경고에 실패 | 수정. `core_warnings` / `info_warnings` 분리 | `self_check.rs` |
| §5 업데이터 재실행 인자 손실 | 수정. `--relaunch-arg` 전달 + 롤백 오류 로깅 + 교체 직전 SHA-256 재검증(`--sha256`) | `kakao-updater/src/{lib,main}.rs`, `kakao-app/src/updater.rs` |
| §5 트레이 콜백 재진입 | 수정. 커맨드 큐 + 내부 가변성으로 `&mut TrayHost` 별칭 제거 | `kakao-win32/src/tray.rs` |
| §5 설정 동시 쓰기 | 수정. 임시 파일명에 PID/시퀀스 포함 | `config.rs` `atomic_write` |
| §5 `get_run_command` 1023자 상한 | 수정. 크기 선조회 후 버퍼 할당 | `kakao-win32/src/startup.rs` |
| §6 문서 불일치 6건 | 수정. README 설정·CLI·트레이 메뉴 갱신, CLAUDE.md에 "Rust 활성 런타임 동작" 절 신설 | `README.md`, `CLAUDE.md` |

**의도적으로 하지 않은 것:** §7 Phase 3 항목 9의 후반부인 `inspect_candidates`와 `apply_once`의 **공용 순회 통합은 하지 않았다.** CLAUDE.md의 "광고차단 알고리즘 고정 규칙"이 임의 리팩터링을 금지하고 있고, 두 함수를 합치는 작업은 판정 결과를 바꿀 실질적 위험이 있다. 성능 목표는 평가 경로 분리(ISSUE-004)만으로 달성했으며, 두 경로가 갈라지지 않는지는 `apply_path_parity.rs`가 fixture 10종 × 4 tick으로 고정한다.

**검증:** `cargo fmt --all -- --check` / `cargo clippy --all-targets --all-features -- -D warnings` 통과, `cargo test --workspace` **78건 전부 통과**(수정 전 60건). 골든 fixture 10종의 기대값은 변경하지 않았다. 격리된 `%APPDATA%`에서 `--self-check --strict-self-check --json`(종료 코드 0), `--shadow`(실제 실행 중인 카카오톡에서 `EVA_Window_Dblclk` 광고 창 1건을 `strong/hide`로 판정) 스모크를 확인했다. **Python 골든(`pytest tests/`)은 이번에도 실행하지 못했다**(환경에 `pytest` 미설치).

---

## 1. Executive Summary

**전체 상태: Acceptable. 전체 위험도: Medium-Low.**

핵심 광고 차단 경로(메인 윈도우 확정 → legacy signature hide → aggressive subtree hide → guarded popup dismiss → empty `EVA_ChildWindow` close)와 복원 경로(스냅샷 저장 → OFF/종료/신호 소멸 시 원복 → 실패 시 스냅샷 보존)는 일관되게 구현되어 있고, 워크스페이스 테스트 60건이 전부 통과한다. 이전 감사(v11.1.1)에서 지적된 9건의 High-Risk 이슈는 현재 코드에서 실제로 해소되었다.

남은 문제는 **기능 오작동보다 "장기 상주 프로세스의 자원 관리"와 "문서·설정이 약속한 동작이 실제로는 없음"** 쪽에 몰려 있다.

가장 중요한 문제 4개:

1. **[ISSUE-001] 로그 회전이 프로세스 시작 시 1회만 수행된다.** 시작프로그램으로 등록해 몇 주간 상주하는 것이 기본 사용 패턴인데, 세션 중에는 5MB 임계값을 다시 검사하지 않는다.
2. **[ISSUE-002] 복원에 계속 실패하는 창을 매 reconciliation tick마다 무한 재시도하며, 그때마다 `warn!` 1줄과 `restore_failures += 1`을 남긴다.** ISSUE-001과 결합하면 단일 세션에서 로그가 수백 MB까지 자랄 수 있고, 실패 카운터도 실제 실패 창 수와 무관한 값이 된다.
3. **[ISSUE-003] 문서화된 설정 3개(`poll_interval_ms`, `cache_cleanup_interval_ms`, `start_minimized`)가 파싱만 되고 런타임에서 전혀 사용되지 않는다.** README는 `poll_interval_ms`를 "활성 상태 탐지 주기"로 안내하지만 Windows 실경로에서는 값이 무시된다.
4. **[ISSUE-004] hot loop가 매 tick마다 후보 평가를 두 번 수행한다.** `evaluate_graph_with_states`가 상태 맵 전체를 clone한 뒤 진단 전용 `inspect_candidates` 전체 순회를 돌리는데, apply 경로에서는 그 결과를 버린다. README가 내세우는 "유휴 CPU 0.01%" 주장과 정면으로 배치되는 낭비다.

**데이터 손상/유실 가능성: 낮음.** 설정/규칙 저장은 임시 파일 + `fs::rename` 원자 교체이고, 파손 JSON은 백업 후 기본값으로 self-heal하며, 업데이터는 `.exe.old` 백업 + 실패 시 롤백을 수행한다. 사용자 데이터를 보관하지 않으며 DB도 없다. 남은 리스크는 §5의 동시 쓰기 정도이고 영향은 "설정 1회 손실" 수준이다.

**가장 먼저 손봐야 할 영역:** 엔진 워커 루프의 로깅/재시도 수명 관리(ISSUE-001, ISSUE-002)와, 설정·문서가 약속한 동작을 실제 코드와 일치시키는 일(ISSUE-003, §6).

---

## 2. Project Understanding

### 목적

카카오톡 Windows 클라이언트의 광고 영역을 **레이아웃 조정과 창 은닉만으로** 제거한다. `hosts`/DNS/네트워크 차단, 메모리 후킹, AdFit 레지스트리 조작을 일절 하지 않고, UAC 없이 사용자 권한으로만 동작한다. 레지스트리는 선택적 시작프로그램(`HKCU\...\Run` + `StartupApproved`)에만 쓴다.

### 주요 entrypoint

| 경로 | 역할 |
|---|---|
| `rust/crates/kakao-app/src/main.rs` | 실제 진입점. GUI 서브시스템(release), 진단 플래그일 때만 부모 콘솔 attach 후 `run_with_args` |
| `rust/crates/kakao-app/src/lib.rs` `run_with_args` | CLI 분기, 설정/규칙 로드, 단일 인스턴스 mutex, 워커 spawn, 트레이 메시지 루프, 업데이트 후처리 |
| `rust/crates/kakao-updater/src/main.rs` | 자동 업데이트 헬퍼(별도 EXE). 부모 종료 대기 → 백업 → 교체 → 재실행 → 실패 시 롤백 |
| `kakaotalk_layout_adblock_v11.py` (루트) | 사용중단 안내만 출력 |
| `legacy/python-v11/` | 골든 fixture/회귀용 Python 참고 구현 (활성 런타임 아님) |

### 핵심 모듈

| 크레이트 | 책임 |
|---|---|
| `kakao-core` | 순수 판정 로직. `model`(윈도우/신호/결정 타입), `rules`(JSON 규칙 + 필드 단위 overlay), `signals`(legacy/aggressive/popup/empty-EVA 판정), `layout`(토큰 매칭·배너 geometry·뷰 리사이즈 계획), `graph`(WindowGraph), `evaluate`(tick 단위 종합 평가) |
| `kakao-win32` | Win32 경계. `real`(실제 API), `fake`(덤프 기반 테스트 더블), `tray`, `event_hook`(SetWinEventHook), `process`(Toolhelp32 PID 스캔), `startup`(Run/StartupApproved 레지스트리), `single_instance`(named mutex) |
| `kakao-app` | 조립. `config`(설정/규칙 로드·self-heal·원자 저장), `graph_build`(Win32 → WindowGraph), `engine`(워커 루프·스냅샷·복원), `dump`, `self_check`, `startup`(Run 명령 health 판정), `updater`(매니페스트 Ed25519 검증·다운로드·staging) |
| `kakao-updater` | EXE 교체/롤백 전용 라이브러리 + 바이너리 |

### 데이터 저장 방식

DB 없음. 파일 3개 모두 `%APPDATA%\KakaoTalkAdBlockerLayout\`:

- `layout_settings_v11.json` — 원자 교체(`atomic_write` = tmp 생성 → `fs::rename`)
- `layout_rules_v11.json` — 동일
- `layout_adblock.log` — `tracing` append, 5MB 초과 시 `.log.1`로 회전(단 프로세스 시작 시 1회, → ISSUE-001)
- 파손 시 `*.broken-<unix-epoch>` 백업, 30일 초과 또는 10개 초과분 자동 정리

런타임 상태(`snapshots` / `states` / `stale_miss`)는 **프로세스 메모리 한정**이며 cross-process 영속화는 의도적으로 없다.

### 외부 의존성

`windows`(Win32 바인딩), `clap`, `serde`/`serde_json`, `tracing`/`tracing-subscriber`, `crossbeam-channel`, `ureq`(HTTP), `ed25519-dalek`, `sha2`, `base64`, `hex`, `thiserror`. 네트워크는 GitHub Releases의 `update.json` + 아티팩트 다운로드 단 2곳뿐이다.

### 핵심 실행 흐름

**(A) 일반 차단 흐름 (가장 중요한 사용자 경로)**

```
EXE 실행 (--startup-launch --minimized)
  → runtime_paths() / ensure_runtime_files()        [APPDATA 부트스트랩]
  → load_settings() + load_rules()                  [파손 시 백업 후 self-heal]
  → InstanceMutex::acquire()                        [중복 실행 차단]
  → run_on_startup이면 Run 명령 health 판정·복구
  → spawn_worker()  ──────────────────────────┐
  → tray::run_loop_with_ready()               │ (메인 스레드: 메시지 루프)
                                              ▼ (워커 스레드)
    EventHook::install()
    loop {
      kakaotalk_pids() / is_process_alive()   [PID 캐시 + 5초 full sync]
      hook.drain() → pid 필터
      wait_message(idle) 또는 burst sleep
      tick():
        build_graph(api, pids)                [EnumWindows → GetParent 직계 자식 트리]
        evaluate_graph_with_states()          [main 확정 → 후보 → 결정 → ActionLog]
        prune_gone_identities()
        apply_evaluation()                    [close → hide(+스냅샷) → set_pos]
        restore_stale_hidden()                [2 tick 연속 미매치 시 원복]
    }
  → 종료(트레이 "종료"/업데이트): stopping=true → worker.join() → restore_all()
```

**(B) 자동 업데이트 흐름**

```
트레이 "업데이트 확인"
  → try_begin_update()                        [single-flight CAS]
  → GET update.json (30/60/90s timeout)
  → parse_and_verify_manifest()               [Ed25519 서명 → tag/version 일치 → artifact_url 화이트리스트
                                               → sha256 형식 → size 범위 → expires_at → is_newer]
  → 사용자 Yes/No
  → prepare_update(): resolve_helper(MZ 검사) → stage_helper(%TEMP%)
                      → download_and_verify(size + sha256) → %TEMP% 저장
  → stopping=true → tray request_exit → worker.join() → restore_all()
  → launch_helper(kakao-updater.exe --pid --current --replacement)
  → 앱 프로세스 종료
       → 헬퍼: WaitForSingleObject(부모) → current→.exe.old 백업
               → replacement→current (rename, 실패 시 copy)
               → 재실행 → 실패 시 롤백 → .exe.old 삭제
```

**(C) 진단 흐름** — `--self-check` / `--dump-tree` / `--dump-tree-series` / `--shadow`는 mutex 밖에서 동작하고, `--shadow`는 `flags.apply=false`로 1회 tick만 돌려 판정 결과를 출력한다.

---

## 3. Audit Coverage & Limitations

### 실제로 확인한 모듈 (전부 직접 열람)

`kakao-app`: `lib.rs`, `main.rs`, `config.rs`, `engine.rs`, `graph_build.rs`, `dump.rs`, `self_check.rs`, `startup.rs`, `updater.rs`
`kakao-core`: `evaluate.rs`, `signals.rs`, `layout.rs`, `rules.rs`, `model.rs`, `graph.rs`
`kakao-win32`: `real.rs`, `tray.rs`, `event_hook.rs`, `process.rs`, `startup.rs`, `single_instance.rs`
`kakao-updater`: `lib.rs`, `main.rs`
기타: `README.md`, `CLAUDE.md`, `packaging/windows_version_info.txt`, `scripts/build_release.ps1`(버전 동기화/스모크 구간), `scripts/dev_check.ps1`, `.github/workflows/windows-ci.yml`

### CodeGraph로 분석한 호출 관계

CodeGraph MCP(`.codegraph/` 인덱스)를 프롬프트 진입 시 사용해 `runtime_paths`의 caller 4곳(`self_check.rs`, `lib.rs`)과 legacy Python `UpdateService` 계열의 caller/테스트 매핑, `WindowGraph`/`FixtureAPI`/`Win32API` 심볼 위치를 확보했다. 이후 Rust 활성 런타임은 파일이 작고(총 7,596 LOC) 호출 그래프가 단선적이어서 전 소스 직접 열람으로 보완했다. 특히 다음 관계는 grep + 직접 열람으로 교차 확인했다:

- `api.enum_windows` / `api.enum_child_windows`의 **유일한** 호출자는 `graph_build.rs` 2곳 (RefCell 재진입 가능성 반증용)
- `settings.*_interval_ms` 필드별 실제 참조 지점 (ISSUE-003 근거)
- `log.closed` 증가 지점 (§6 근거, 0건)
- `should_attach_parent_console` 호출 2지점의 인자 차이 (ISSUE-005 근거)

### 실행한 테스트

```
cd rust; cargo test --workspace   →  전부 통과 (60 tests)
```

| 대상 | 결과 |
|---|---|
| `kakao-app` unit (lib.rs) | 17 passed |
| `config_migration` | 6 passed |
| `engine_idle_cpu_test` | 2 passed |
| `graph_direct_children` | 1 passed |
| `restore_regression` | 8 passed |
| `scan_parity` | 3 passed |
| `self_check_report` | 2 passed |
| `kakao-core` `evaluate_weak_parity` / `golden_parity` | 1 + 1 passed |
| `kakao-updater` unit + `updater_tests` | 4 passed |
| `kakao-win32` unit + `smoke` | 14 + 1 passed |

### 확인하지 못한 것

- **Python 골든 회귀(`pytest tests/`)를 실행하지 못했다.** 이 환경의 Python에 `pytest`가 없고, 감사에 불필요한 의존성 설치는 하지 않았다. `tests/` 18개 파일은 정적으로만 확인했다.
- **실제 카카오톡(`kakaotalk.exe`)을 띄운 end-to-end 검증을 하지 않았다.** 광고 판정 정확도, `SetWinEventHook` 실측 이벤트량, `IsWindowVisible` 복원 실패 재현은 전부 코드 근거 기반이다. 따라서 ISSUE-002·ISSUE-006은 `Confirmed`가 아니라 `Likely`로 둔다.
- `--self-check` / `--dump-tree` 등 진단 CLI를 실행하지 않았다. 실행 시 `%APPDATA%`의 실제 사용자 설정 파일을 생성·치유(`heal_default`)할 수 있어 "production 데이터 변경 금지" 제약에 걸린다.
- 릴리스 빌드(`scripts/build_release.ps1`), 서명, GitHub Actions 실행은 스크립트 정적 확인만 했다.
- 업데이트 서버(`releases/latest/download/update.json`) 실제 응답은 확인하지 않았다.

### 분석상의 한계

- CodeGraph는 `legacy/` 심볼도 함께 노출한다. 본 감사는 CLAUDE.md 지침대로 **활성 런타임을 `rust/`로 한정**해 판단했고, `legacy/python-v11`은 계약 참조용으로만 읽었다.
- Win32 부작용(예: 위치 변화 없는 `SetWindowPos`가 `EVENT_OBJECT_LOCATIONCHANGE`를 발생시키는지)은 문서만으로 확정할 수 없어 해당 추론은 §5에 `추정`으로 분리했다.

---

## 4. High-Risk Issues

### [ISSUE-001] 로그 회전이 프로세스 시작 시 1회만 수행되어 상주 세션 중 로그가 무제한 증가

- **위치:** `rust/crates/kakao-app/src/config.rs:275` `rotate_log_if_needed`, 호출부 `rust/crates/kakao-app/src/lib.rs:89`, `rust/crates/kakao-app/src/lib.rs:117`
- **우선순위:** Medium
- **신뢰도:** Confirmed
- **문제:** `rotate_log_if_needed`는 5MB(`LOG_ROTATE_BYTES`) 초과 시 `.log.1`로 회전하지만, 호출은 `run_with_args` 초기 1회뿐이다. 그 뒤 `init_tracing`이 파일 핸들을 열어 append layer로 등록하고, 프로세스가 살아 있는 동안 크기를 다시 검사하는 지점이 없다.
- **발생 조건:** 시작프로그램 등록 상태로 재부팅 없이 며칠~몇 주 상주 + 로그를 꾸준히 생성하는 경로(ISSUE-002의 tick 단위 `warn!`, PID 스캔 경고 등)가 활성일 때.
- **영향:** `%APPDATA%\KakaoTalkAdBlockerLayout\layout_adblock.log`가 수백 MB 규모로 성장 가능. 트레이 "로그 폴더 열기" → 로그 확인이라는 문서화된 문제 해결 절차가 사실상 불가능해지고, 저용량 SSD에서는 디스크 압박으로 이어진다. 재시작하면 회전되므로 영구 손상은 아니다.
- **근거:** `lib.rs:89`(self-check 경로), `lib.rs:117`(일반 실행 경로) 두 곳이 `rotate_log_if_needed`의 전 호출자다(grep 확인). 워커 루프(`engine.rs:337-473`)와 트레이 루프(`tray.rs:267-270`) 어디에도 재검사가 없다. `tracing_subscriber`의 파일 layer는 `OpenOptions::append`로 고정 핸들을 쥐고 있어 외부 회전에도 대응하지 않는다.
- **반증 확인:**
  - `tracing-appender`의 rolling writer를 쓰는지 확인 → 쓰지 않는다. `lib.rs:594-601`은 평범한 `File`을 `with_writer`에 넘긴다.
  - OS나 라이브러리가 크기를 제한하는지 → 없다.
  - 평상시 로그량이 무시할 수준인지 → `tick()`의 per-candidate `info!`는 shadow 경로(`flags.apply == false`)에서만 실행되므로 정상 운영 시 조용한 것은 맞다. 그러나 `warn!` 경로(ISSUE-002)가 열리면 tick당 1줄, 즉 약 5줄/초가 되어 제한이 사라진다. "조용할 때가 많다"는 것이 상한의 부재를 상쇄하지 못한다.
- **호출/영향 범위:** `run_with_args` → `rotate_log_if_needed` / `init_tracing`. 영향 모듈은 로깅을 쓰는 전 구간(`engine`, `updater`, `startup`, `config` 경고).
- **권장 수정 방향:** 워커 루프의 저빈도 분기(예: PID full sync 5초 주기)에서 `rotate_log_if_needed`를 재호출하거나, `tracing-appender`의 size/시간 기반 rolling writer로 교체한다. 후자가 열린 핸들 문제까지 함께 해결한다.
- **필요한 회귀 테스트:** 임시 디렉터리에 5MB+1 바이트 로그를 만들고 워커 루프를 회전 주기 이상 돌린 뒤 `layout_adblock.log`가 `LOG_ROTATE_BYTES` 이하이고 `layout_adblock.log.1`이 생성되었는지 확인.

---

### [ISSUE-002] 복원에 실패한 창을 매 tick 무한 재시도하며 경고 로그와 실패 카운터를 무한 증가

- **위치:** `rust/crates/kakao-app/src/engine.rs:203` `restore_stale_hidden` (특히 `engine.rs:233-235`), 호출부 `engine.rs:265-269`
- **우선순위:** Medium
- **신뢰도:** Likely
- **문제:** 복원에 실패하면 스냅샷을 다시 `snapshots`에 넣고 `stale_miss`를 `RESTORE_MISS_THRESHOLD`(2)로 되돌린다. 다음 tick에 해당 identity가 여전히 미매치면 `misses = 2 + 1 = 3 >= 2`가 되어 **즉시 다시 재시도**한다. 백오프도, 시도 횟수 상한도 없다. 재시도가 실패할 때마다 `flags.restore_failures.fetch_add(1)`과 `warn!` 1줄이 누적된다.
- **발생 조건:** 살아 있는(= `identity_matches`가 참인) 창인데 `restore_snapshot`이 실패를 반환하는 상태가 지속될 때. 가장 현실적인 시나리오는 **카카오톡이 트레이로 닫힌(메인 윈도우 hidden) 상태에서 사용자가 공격 모드를 끄는 경우**다. aggressive 매치가 사라져 stale 복원이 시작되지만, 자식 창에 `SW_SHOW`를 보내도 조상이 hidden이라 `IsWindowVisible`이 계속 false를 반환하고(`engine.rs:193-199`), 사용자가 카카오톡 창을 다시 열 때까지 실패가 지속된다.
- **영향:** ① reconciliation 주기(기본 200ms)마다 경고 1줄 → 약 5줄/초 → 시간당 약 2MB. ISSUE-001과 결합하면 로그가 통제 불능으로 자란다. ② `restore_failures`가 "복원 실패 창 수"가 아니라 "실패 시도 누적 횟수"가 되어 의미를 잃는다. ③ tick마다 불필요한 `SetWindowPos`/`ShowWindow` 호출이 추가된다.
- **근거:** `engine.rs:217-219`에서 `misses`를 saturating 증가시킨 뒤 `*misses < RESTORE_MISS_THRESHOLD`만 검사한다. 실패 경로(`engine.rs:233-235`)가 `stale_miss`를 임계값으로 되돌리므로 다음 tick에 조건이 항상 성립한다. 성공/소멸 경로(`engine.rs:214-215`, `226-227`, `230-231`)만 `stale_miss`에서 제거된다. `restore_snapshot`의 실패 판정 두 갈래(`set_window_pos` false, `SW_SHOW` 후 `IsWindowVisible` false) 중 후자는 조상 hidden 시 Win32 정의상 항상 false다.
- **반증 확인:**
  - *상위에서 이미 걸러지는가* → `tick`은 `!snapshots.is_empty()`일 때만 호출하는데, 실패 스냅샷은 계속 남으므로 조건이 계속 참이다. 걸러지지 않는다.
  - *카카오톡 종료 시 자연 정리되는가* → 그렇다. `engine.rs:403-410`에서 PID가 비면 `restore_all`이 돌고, 창이 사라졌으면 `identity_matches`가 false라 스냅샷이 버려진다. **이 경로는 반증된다.** 따라서 문제는 "창은 살아 있는데 복원이 거부되는" 구간에 한정된다.
  - *hidden 광고 창이 애초에 미매치가 되는가* → 정상 운영 중에는 되지 않는다. `legacy_signature_kind`/`subtree_contains_ad_token`은 텍스트와 rect만 보므로 숨긴 뒤에도 계속 매치되고, `matched_identities`에 포함되어 stale 복원이 발동하지 않는다. 즉 **평상시에는 안전**하며, 매치가 끊기는 전이(공격 모드 OFF, 규칙 변경, 광고 토큰 소멸)에서만 발동한다. 이 점이 신뢰도를 `Confirmed`가 아닌 `Likely`로 낮추는 근거다.
  - *dead code인가* → 아니다. `restore_stale_hidden`은 apply + enabled 상태의 정상 tick 경로에서 호출된다.
- **호출/영향 범위:** `spawn_worker` → `tick` → `restore_stale_hidden` → `restore_snapshot` → `Win32Api::{set_window_pos, show_window, is_window_visible}`. 영향: 로그 파일 크기(ISSUE-001), 트레이 "복원 실패 초기화" 의미, 워커 CPU.
- **권장 수정 방향:** identity별 재시도 횟수 상한(예: 5회)과 지수 백오프를 두고, 상한 도달 시 경고를 1회만 남긴 뒤 주기를 크게 늘린다. `restore_failures`는 "실패 시도 누적"이 아니라 "현재 복원 실패 상태인 창 수"(= `snapshots` 중 실패 표시 개수)로 의미를 바꾸는 편이 트레이 UX와 맞는다. 부모가 hidden이라 `IsWindowVisible`이 false인 경우는 실패가 아니라 "보류"로 분류하는 것도 검토 대상이다.
- **필요한 회귀 테스트:** `FakeWin32`에 "`SW_SHOW`를 받아도 `is_window_visible`이 false를 유지하는 창"을 추가하고, 10 tick 실행 후 ①`restore_failures`가 무한 증가하지 않고 상한에 수렴할 것 ②`set_window_pos`/`show_window` 호출 횟수가 tick 수에 선형 비례하지 않을 것을 검증.

---

### [ISSUE-003] 문서화된 설정 3개가 런타임에서 전혀 사용되지 않음 (`poll_interval_ms` 포함)

- **위치:** `rust/crates/kakao-app/src/config.rs:24,26,32` (필드 정의), `rust/crates/kakao-app/src/engine.rs:400` (`active_ms` 산출), `rust/crates/kakao-app/src/lib.rs:127` (`start_minimized` 대입)
- **우선순위:** Medium
- **신뢰도:** Confirmed
- **문제:** README가 사용자 조정 대상으로 안내하는 설정 중 3개가 실제 동작에 영향을 주지 않는다.
  - `poll_interval_ms`: `engine.rs:400`에서 `active_ms`로 계산되지만, 이를 쓰는 유일한 지점은 `engine.rs:433`의 `thread::sleep(active_ms)`이고 그 줄은 **`EventHook::install()`이 실패해 `hook`이 `None`일 때만** 도달한다. 정상 Windows 환경에서는 항상 `engine.rs:426-431`의 `hook.wait_message(...)` 분기에서 `continue`되며, 대기 시간은 전부 `idle_poll_interval_ms` 기준이다.
  - `cache_cleanup_interval_ms`: 구조체 필드와 테스트 단언 외에 참조가 0건.
  - `start_minimized`: `lib.rs:127`에서 `true`로 대입되지만 이후 어디서도 읽지 않는다. Rust 앱은 트레이 전용이라 숨길 메인 창 자체가 없다.
- **발생 조건:** 사용자가 README §"설정 및 규칙 커스터마이징"을 보고 `layout_settings_v11.json`을 편집하는 순간.
- **영향:** 사용자가 반응 속도를 높이려 `poll_interval_ms`를 10으로 낮춰도 아무 변화가 없고, 원인을 알 방법이 없다(경고도 로그도 없다). 실제 탐지 주기는 항상 `max(idle_poll_interval_ms, 200)`이다. 기능 장애는 아니지만 **문서가 보증한 제어 수단의 부재**이며, 향후 "반응이 느리다" 류 이슈의 오진 원인이 된다.
- **근거:** grep 결과 — `cache_cleanup_interval_ms`는 `config.rs:32`와 `tests/config_migration.rs:25` 2곳, `start_minimized`는 `config.rs:24` / `lib.rs:127` / `tests/config_migration.rs:21` 3곳, `poll_interval_ms`의 런타임 참조는 `engine.rs:400` 단 1곳뿐이며 그 소비처는 `engine.rs:433` 한 줄이다. `engine.rs:424-435` 블록을 보면 Windows + hook 정상 시 433행에 도달할 수 없다.
- **반증 확인:**
  - *`--dump-tree` 등 다른 경로에서 쓰이는가* → `dump.rs`는 `LayoutSettings`(= `enabled`, `aggressive_mode` 2필드)만 받는다. 쓰이지 않는다.
  - *하위 호환을 위해 일부러 남긴 필드인가* → 그럴 수 있으나, README가 현재형으로 "기본값 50ms 탐지 주기"라고 안내하는 이상 사용자 관점에서는 동작 불일치다. `tests/config_migration.rs`가 파싱만 단언하고 효과는 단언하지 않는 점도 이 구멍을 테스트가 못 잡는 이유다.
  - *dead code 과장 아닌가* → 필드 자체는 직렬화·역직렬화되어 실제 파일에 쓰이므로 dead code가 아니라 "표면은 살아 있고 효과만 없는" 상태다. 사용자에게 노출되는 계약이라 과장이 아니다.
- **호출/영향 범위:** `load_settings` → `AppSettings` → `spawn_worker(settings)` → 워커 루프. `save_settings`를 통해 파일에도 계속 기록된다.
- **권장 수정 방향:** 둘 중 하나로 정리한다. (a) `wait_message` 대기 상한을 `poll_interval_ms`로 묶어 이벤트 응답 주기를 실제로 제어하게 만들고 `cache_cleanup_interval_ms`로 `states`/`stale_miss` 정리 주기를 실제 스로틀링에 연결한다. (b) 효과가 없는 필드는 README에서 제거하거나 "(현재 미사용)"으로 명시하고, 로드 시 경고를 남긴다.
- **필요한 회귀 테스트:** `poll_interval_ms`를 극단값(예: 1000)으로 설정한 뒤 hook이 설치된 워커의 tick 간격이 그 값을 따르는지 측정(현재는 실패해야 정상). 반대로 (b)를 택한다면 README와 구조체의 필드 집합이 일치하는지 확인하는 문서 동기화 테스트를 둔다.

---

### [ISSUE-004] hot loop가 매 tick마다 상태 맵 전체를 clone하고 폐기될 후보 평가를 한 번 더 수행

- **위치:** `rust/crates/kakao-core/src/evaluate.rs:699-731` (`evaluate_graph_with_states`), 소비부 `rust/crates/kakao-app/src/engine.rs:258`, `engine.rs:456`
- **우선순위:** Medium
- **신뢰도:** Confirmed
- **문제:** `evaluate_graph_with_states`는 `let mut preview_states = states.clone();` 후 `inspect_candidates(...)`로 전체 후보 순회를 한 번 돌고, 이어서 `apply_once(...)`로 **거의 같은 순회를 다시** 돈다. `inspect_candidates`의 산출물(`candidate_payloads`)은 진단용이며, 프로덕션 apply 경로에서는 `tick`의 반환값이 `engine.rs:456`에서 `let _ =`로 버려진다. `evaluation.candidates`를 실제로 읽는 곳은 shadow 분기(`engine.rs:272-281`) 하나뿐인데, 이 분기는 `flags.apply == false`일 때만 실행되고 워커에서는 `apply`가 항상 true다.
- **발생 조건:** 항상. 차단이 켜진 상태의 모든 tick(약 5회/초, 버스트 시 최대 50회/초).
- **영향:** tick당 작업량이 약 2배가 된다. 중복되는 작업에는 `subtree_contains_ad_token`(깊이 8 재귀), `class_name_starts_with`(깊이 8 재귀), `find_popup_matches`, `legacy_signature_kind`(깊이 8 재귀 × `chrome_legacy_title_contains` 토큰 수)가 포함되어 그래프 크기에 비례한 재귀 순회가 통째로 두 번 돈다. 여기에 `states.clone()`이 tick마다 `HashMap<WindowIdentity, CandidateState>`(문자열 키 포함) 전체를 힙 복제한다. README가 1급 셀링 포인트로 내세우는 "유휴 CPU 0.01%", "메모리 3~8MB" 주장과 직접 충돌한다. 판정 결과 자체는 틀리지 않으므로 기능 오류는 아니다.
- **근거:** `evaluate.rs:713` `let mut preview_states = states.clone();`, `evaluate.rs:714-722` `inspect_candidates(..., &mut preview_states)`, `evaluate.rs:723-731` `apply_once(..., states)`. `preview_states`는 이후 어디에도 반영되지 않고 함수 종료 시 폐기된다. `inspect_candidates`(`evaluate.rs:282`~)와 `apply_once`(`evaluate.rs:482`~)는 main child 순회, candidate 순회, popup 순회 세 블록이 사실상 동일하다.
- **반증 확인:**
  - *`preview_states`가 결과에 영향을 주는가* → 주지 않는다. `store_update`가 `preview_states`를 변경하지만 그 맵은 반환되지도, `states`에 병합되지도 않는다. `candidates` 페이로드의 `match_streak`/`miss_streak` 표시용으로만 쓰인다.
  - *apply 경로에서 `candidates`를 누가 읽는가* → `engine.rs`에서는 shadow 분기뿐. `dump.rs:42`(진단), `--shadow`(`lib.rs:231`)는 별개 경로라 필요하다. 즉 **진단 경로에서는 정당한 작업이고, 워커 경로에서만 낭비**다.
  - *컴파일러가 제거해 주는가* → 아니다. `inspect_candidates`는 `Vec<CandidatePayload>`를 만들어 `Evaluation`에 담아 반환하므로 관측 가능한 값이며 최적화로 사라지지 않는다.
  - *성능 측정 없이 과장하는 것 아닌가* → 실측 벤치는 하지 않았다. 다만 "동일 재귀 순회 2회 + 맵 clone 1회를 매 tick 수행하고 그중 하나는 폐기된다"는 사실 자체는 코드로 확정된다. 체감 영향의 크기는 미측정임을 명시한다.
- **호출/영향 범위:** `tick` → `evaluate_graph_with_states` → {`inspect_candidates`, `apply_once`} → `signals::*` 재귀 함수군. `dump_payload_with_states`와 `--shadow`도 같은 함수를 쓰므로 수정 시 진단 출력이 깨지지 않도록 해야 한다.
- **권장 수정 방향:** `evaluate_graph_with_states`에 후보 페이로드 생성 여부 플래그(또는 `evaluate_graph_for_apply` 전용 진입점)를 두고, 워커 tick에서는 `inspect_candidates`와 `states.clone()`을 건너뛴다. 진단 경로(`dump`, `--shadow`)만 기존 동작을 유지한다.
- **필요한 회귀 테스트:** ① 골든 fixture로 apply-only 경로와 기존 경로의 `actions`/`state`가 **완전히 동일**함을 단언(`tests/golden_parity.rs` 확장). ② `--dump-tree` 경로의 `candidates` 출력이 변하지 않음을 단언. ③ `engine_idle_cpu_test`에 tick당 `Win32Api` 호출 횟수 상한 단언을 추가해 회귀를 막는다.

---

### [ISSUE-005] 중복 실행 시 안내 메시지 박스가 절대 표시되지 않아 두 번째 실행이 완전히 무반응

- **위치:** `rust/crates/kakao-app/src/lib.rs:208` (`should_attach_parent_console(std::env::args())`), 비교 대상 `rust/crates/kakao-app/src/main.rs:8` (`std::env::args().skip(1)`)
- **우선순위:** Medium
- **신뢰도:** Confirmed
- **문제:** 단일 인스턴스 mutex 획득 실패 시 "프로그램이 이미 실행 중입니다." 메시지 박스를 띄우려 하지만, 가드 조건이 `!should_attach_parent_console(std::env::args())`다. `std::env::args()`의 첫 원소는 실행 파일 경로(argv[0])이고, 이 문자열은 `--minimized` / `--startup-launch` / `--apply` 어디에도 해당하지 않고 `--startup-trace` / `--exit-after-startup-ms` 접두사도 아니므로 클로저가 즉시 `true`를 반환한다. 따라서 `any(...)`는 **인자 구성과 무관하게 항상 true**이고, 가드는 항상 false가 되어 메시지 박스 호출에 도달할 수 없다.
- **발생 조건:** 이미 트레이에 상주 중인 상태에서 사용자가 EXE를 다시 더블클릭하거나 바로 가기를 다시 실행할 때(가장 흔한 오조작).
- **영향:** release 빌드는 `windows_subsystem = "windows"`이고 이 경로에서는 콘솔도 attach되지 않으므로 `eprintln!("already running")`도 어디에도 보이지 않는다. 결과적으로 **아무 반응 없이 종료 코드 0으로 조용히 끝난다.** 사용자는 실행이 실패한 것인지 이미 돌고 있는 것인지 구분할 수 없고, 반복 더블클릭으로 이어진다.
- **근거:** `main.rs:8`은 같은 함수를 `.skip(1)`로 호출해 argv[0]을 명시적으로 제외한다. 동일 함수를 `lib.rs:208`에서는 `.skip(1)` 없이 호출한다. 이 비대칭이 버그의 본체다. `should_attach_parent_console`의 단위 테스트(`lib.rs:628-644`)는 `["--minimized"]`처럼 argv[0]이 없는 벡터만 넣어 이 호출부를 전혀 커버하지 않는다.
- **반증 확인:**
  - *Rust에서 argv[0]이 정말 포함되는가* → `std::env::args()` 문서상 "The first element is traditionally the path of the executable"이며 Windows에서도 동일하다. 포함된다.
  - *다른 경로에서 사용자에게 알리는가* → `lib.rs:205`의 `eprintln!`뿐이고, GUI 서브시스템 + 콘솔 미연결이라 소실된다. 대안 통지 없음.
  - *진단 CLI에서는 의도적으로 박스를 숨기려는 것 아닌가* → 맞다. 의도는 "진단 CLI면 콘솔 출력, 아니면 박스"이며, 그 의도가 인자 목록에 argv[0]이 섞이면서 무너졌다. 의도 자체는 코드 주석(`lib.rs:60-61`)에 명시돼 있다.
  - *실제로 도달 가능한 코드인가* → mutex 실패는 `diagnostic == false`인 일반 실행에서만 평가되며, 두 번째 실행은 반드시 이 경로를 탄다. 도달 가능하다.
- **호출/영향 범위:** `run_with_args` → `InstanceMutex::acquire` 실패 분기 → `show_info_box`(미도달). 영향은 UX에 한정되며 차단 기능이나 데이터에는 영향이 없다.
- **권장 수정 방향:** `lib.rs:208`을 `should_attach_parent_console(std::env::args().skip(1))`로 맞추거나, 파싱된 `Args`에서 직접 진단 여부를 판정한다(중복 판정 로직 제거가 더 낫다).
- **필요한 회귀 테스트:** `should_attach_parent_console`에 **argv[0]을 포함한** 입력 케이스를 추가: `["C:\App\KakaoTalkLayoutAdBlocker_v11.exe", "--startup-launch", "--minimized"]` → `false`, `["C:\App\KakaoTalkLayoutAdBlocker_v11.exe", "--self-check"]` → `true`. 나아가 호출부가 동일한 정규화를 쓰도록 헬퍼를 하나로 묶고 그 헬퍼를 테스트한다.

---

### [ISSUE-006] 전역 WinEvent 훅이 데스크톱 전체 이벤트를 수집해 채널 포화 시 카카오톡 이벤트를 유실

- **위치:** `rust/crates/kakao-win32/src/event_hook.rs:55-78` (`EventHook::install`), 소비부 `rust/crates/kakao-app/src/engine.rs:385-397`
- **우선순위:** Medium
- **신뢰도:** Likely
- **문제:** `SetWinEventHook(min, max, None, Some(hook_proc), 0, 0, WINEVENT_OUTOFCONTEXT)`에서 `idProcess`/`idThread`가 모두 0이므로 **세션 내 모든 프로세스의 모든 스레드** 이벤트를 받는다. 범위도 `EVENT_OBJECT_CREATE ..= EVENT_OBJECT_NAMECHANGE`로, `EVENT_OBJECT_LOCATIONCHANGE`(창 이동/크기 변경 시 연속 발생)와 `EVENT_OBJECT_STATECHANGE`, `EVENT_OBJECT_REORDER` 등 고빈도 이벤트를 전부 포함한다. 수신 채널은 `bounded(1024)`이고 `hook_proc`은 `try_send`라 가득 차면 **조용히 버린다**. PID 필터링은 채널에서 꺼낸 뒤(`engine.rs:390-393`)에야 수행되므로, 무관한 프로세스의 이벤트가 큐를 점유해 카카오톡 이벤트를 밀어낼 수 있다.
- **발생 조건:** 다른 창을 드래그/리사이즈하거나, 애니메이션이 있는 앱·브라우저를 쓰는 동안(= 일상적인 데스크톱 사용) 카카오톡 광고 창이 생성될 때.
- **영향:** ① 이벤트 유실 시 즉시 반응이 사라지고 최대 `idle_poll_interval_ms`(기본 200ms)의 reconciliation까지 광고가 노출된다. README가 내세우는 "딜레이 없이 즉각 제거"가 상황에 따라 성립하지 않는다. ② 유실되지 않는 경우에도 이벤트 1건마다 `api.get_window_thread_process_id`(커널 전이) 호출이 발생해, 데스크톱이 바쁠수록 이 앱의 CPU가 같이 오른다.
- **근거:** `event_hook.rs:65`의 `idProcess=0, idThread=0`, `event_hook.rs:56`의 `bounded(1024)`, `event_hook.rs:41`의 `try_send`(실패 무시), `engine.rs:390-393`의 사후 PID 필터. 이벤트 범위 상수는 `event_hook.rs:59`.
- **반증 확인:**
  - *상위에서 범위를 좁히는가* → 좁히지 않는다. `install()`은 인자를 받지 않는 고정 구현이다.
  - *유실이 기능 장애로 이어지는가* → 완전한 장애는 아니다. `engine.rs:422`의 `due_recon`(idle 주기 reconciliation)이 안전망이라 광고는 결국 제거된다. 이 안전망 때문에 심각도를 High가 아니라 Medium으로 둔다.
  - *실제로 1024개가 찰 만큼 이벤트가 오는가* → 실측하지 않았다. `EVENT_OBJECT_LOCATIONCHANGE`가 드래그 중 초당 수십~수백 건 발생한다는 것은 Win32의 일반적 동작이지만, 이 환경에서 카운트를 재지 않았으므로 `Likely`로 둔다.
  - *dead code인가* → 아니다. 워커 루프의 상시 경로다.
- **호출/영향 범위:** `spawn_worker` → `EventHook::install` / `drain` / `wait_message` → `tick` 트리거. 영향: 광고 제거 지연 시간, 워커 CPU.
- **권장 수정 방향:** ① 카카오톡 PID가 확정된 뒤 해당 PID로 `idProcess`를 지정해 훅을 재설치한다(PID 변경 시 재설치). ② 최소한 `EVENT_OBJECT_LOCATIONCHANGE`를 별도 범위로 분리해 필요 없으면 제외한다. ③ `hook_proc` 안에서 관심 이벤트만 보내도록 1차 필터를 둔다. 부수적으로 `EVENT_TX: OnceLock`은 재설치를 지원하지 못하므로(두 번째 `set`은 조용히 실패) 재설치 도입 시 함께 구조를 바꿔야 한다.
- **필요한 회귀 테스트:** 이벤트 채널을 주입 가능하게 만들고, ①무관 PID 이벤트 2000건을 밀어 넣은 뒤 카카오톡 PID 이벤트 1건이 소실되지 않는지 ②훅 재설치 후에도 `drain`이 새 이벤트를 받는지(현재는 `OnceLock` 때문에 실패해야 정상)를 검증.

---

## 5. Potential Functional Gaps

### Confirmed Gap — 복원 실패 상태를 사용자가 볼 방법이 없다

트레이 메뉴에 "복원 실패 초기화"(`ID_RESET_RESTORE`) 항목이 있고 `flags.restore_failures`가 실제로 증가하지만, **이 값을 표시하는 UI가 없다.** `tray.rs:344-388`의 `show_menu`가 만드는 메뉴에는 상태 문자열도, 카운터도, 마지막 오류도 없고 헤더는 고정 문자열 `"KakaoTalk Layout AdBlocker"`(`tray.rs:348`)뿐이다. 사용자는 자신이 리셋해야 할 실패가 있는지조차 알 수 없다. CLAUDE.md가 기술하는 상태 문자열(`메인윈도우`, `누적 숨김/닫힘/리사이즈`, 마지막 오류/갱신시각)은 Python 구현의 계약이고 Rust 트레이에는 이식되지 않았다(§6 참조). 메뉴 헤더나 툴팁(`NIF_TIP`)에 상태를 싣는 것이 가장 저비용이다.

### Confirmed Gap — `closed_windows` 카운터가 영구히 0

`MutationLog.closed`는 `evaluate.rs:128`에서 0으로 초기화된 뒤 **증가하는 코드가 없다**(grep 확인). `send_close`(`evaluate.rs:155-157`)는 `close` 벡터에만 push하고 카운터는 건드리지 않으며, `send_popup_close`는 `popup_close_requests`만 올린다. 따라서 `EngineStatePayload.closed_windows`는 항상 0이다. CLAUDE.md는 "대상 창 소멸을 확인한 경우에만 `closed_windows`를 증가"한다고 기술한다. 현재는 진단 출력(`--shadow`, `dump`)에도 노출되지 않아 실사용 영향은 없지만, 알고리즘 변경 시 근거로 쓰라고 규정된 진단 데이터의 신뢰도를 깎는다.

### Confirmed Gap — popup dismiss 결과를 검증하지 않는다

`apply_evaluation`은 `let _ = api.send_message_timeout(*hwnd, WM_CLOSE, 0, 0, 500);`(`engine.rs:104`)로 반환값을 버리고, 성공 여부와 무관하게 이어서 hide와 zero-size를 적용한다. CLAUDE.md는 Python 계약으로 "실제 close/hide/zero-size 성공 여부를 검증하며 실패 시 상태(`last_error`)와 로그에 반영"을 규정한다. Rust는 hide/zero-size fallback을 항상 적용하므로 **광고가 남는 실패는 발생하지 않지만**, 창이 `WM_CLOSE`를 거부했다는 사실이 로그에 전혀 남지 않아 카카오톡 UI 변경 시 원인 추적이 어렵다. 관련해서 `dismiss_popup`(`evaluate.rs:672-683`)의 `hidden_ok`는 바로 앞 `log.hide_window`가 항상 `visible=false`를 기록하므로 **항상 true**이며, `popup_hide_fallbacks`와 `popup_zero_size_fallbacks`가 늘 함께 증가해 두 카운터를 구분할 수 없다.

### Confirmed Gap — `--self-check`가 README가 약속한 항목을 점검하지 않는다

`self_check::run`(`self_check.rs:7-44`)이 실제로 보는 것은 `cfg!(windows)`, APPDATA 디렉터리 존재/쓰기 가능, 설정·규칙 로드 경고뿐이다. README는 "레지스트리, 프로세스, 설정 파일 상태 점검" / "시스템 권한, 프로세스 탐색, 설정 파일 무결성"이라고 안내한다. **Run 레지스트리 접근 점검도, 프로세스 열거 점검도 없다.** 진단 도구가 문제를 놓치는 방향의 갭이라 사용자 자가 진단 가치가 낮다. `kakao_win32::startup::get_run_command()`와 `process::kakaotalk_pids()`를 호출해 결과를 페이로드에 싣는 것만으로 대부분 메워진다.

### Likely Gap — `--strict-self-check`가 양성 경고에도 실패해 릴리스 빌드를 깨뜨릴 수 있다

`self_check.rs:26-28`은 `strict && !warnings.is_empty()`이면 `core_ok = false`로 만든다. 경고에는 "banner 높이 범위 자동 교정", "필드 타입이 올바르지 않아 기본값 유지" 같은 **정상 복구 사례**가 포함된다. `scripts/build_release.ps1`은 기본값으로 빌드된 EXE에 `--self-check --strict-self-check --json`을 돌리고 core failure를 빌드 실패로 취급하므로, 빌드 머신의 `%APPDATA%` 설정 상태(예: 과거에 손으로 편집한 규칙 파일)만으로 릴리스가 실패할 수 있다. 깨끗한 CI 러너에서는 재현되지 않아 로컬에서만 터지는 종류다. 치명적 경고와 정보성 경고를 분리하는 것이 맞다.

### Likely Gap — 업데이트 재실행이 원래 명령줄을 잃는다

`kakao-updater/src/lib.rs:147`의 `Command::new(current).spawn()`은 인자 없이 실행한다. 원래 프로세스가 `--startup-launch --minimized`로 떠 있었더라도 새 인스턴스는 인자 없이 시작한다. 현재 Rust 앱은 트레이 전용이고 `start_minimized`가 사용되지 않아(ISSUE-003) **지금은 체감 차이가 없지만**, 향후 창 UI나 인자 기반 동작이 추가되면 조용히 깨진다. 같은 함수의 롤백 경로(`lib.rs:150-151`)가 두 `fs::rename` 오류를 모두 `let _ =`로 버리는 점도 함께 손보는 편이 좋다 — 두 rename이 모두 실패하면 설치본이 깨진 채 `RelaunchFailed`만 보고된다.

### Likely Gap — 헬퍼가 교체 직전에 아티팩트를 재검증하지 않는다

`prepare_update`는 앱 프로세스에서 size + SHA-256을 검증한 뒤 `%TEMP%`에 저장하고(`updater.rs:312-343`), 헬퍼는 나중에 별도 프로세스로 떠서 `exists()`와 `len() != 0`만 확인한 뒤 교체한다(`kakao-updater/src/lib.rs:91-101`). 그 사이의 TOCTOU 구간이 존재한다. `%TEMP%`는 사용자별 디렉터리이고 같은 사용자 권한의 코드는 이미 EXE를 직접 바꿀 수 있으므로 **권한 상승은 아니다.** 다만 `--current`/`--replacement`를 인자로 받는 범용 교체 도구가 앱 폴더에 함께 배포된다는 점을 고려하면, 헬퍼에 기대 해시를 인자로 넘겨 교체 직전 재검증하는 것이 방어적으로 낫다.

### Likely Gap — 트레이 콜백이 모달 메시지 박스를 띄워 `wnd_proc` 재진입 가능

`wnd_proc`(`tray.rs:299`)은 `(&mut *host).on_command)(cmd)` 형태로 `TrayHost`에 대한 가변 참조를 만든다. `TrayCommand::CheckUpdate` 처리에서 `try_begin_update()`가 실패하면 `show_info_box`(`lib.rs:368`)가 **동기 `MessageBoxW`**를 띄우는데, `MessageBoxW`는 자체 모달 메시지 루프를 돌리므로 그 동안 같은 창의 `WM_COMMAND`가 재진입 디스패치될 수 있다. 그러면 동일 `TrayHost`에 대한 `&mut`가 중첩되어 Rust의 aliasing 규칙상 UB다. 현재 `TrayHost` 필드가 단순(플래그 Arc, `NOTIFYICONDATAW`, bool)이라 실제 오작동을 관측하지는 못했고 재현도 하지 못했으므로 **구조적 부채**로 기록한다. 콜백을 큐잉해 모달 밖에서 처리하거나, 재진입 가드 플래그를 두면 해소된다.

### 추정 — hidden 팝업에 대한 tick 단위 재적용이 자기 유발 이벤트를 만들 가능성

`keep_hidden_popup`(`evaluate.rs:685-688`)은 이미 숨겨진 팝업에도 매 tick `SW_HIDE` + `SetWindowPos(0,0,0,0)`를 재발행한다. `SetWindowPos`가 위치 변화 없이도 `EVENT_OBJECT_LOCATIONCHANGE`를 유발한다면, 그 이벤트는 카카오톡 PID 소속이라 필터를 통과해 다음 tick을 즉시 트리거하는 자기 유발 루프가 된다. **Win32가 변화 없는 `SetWindowPos`에서 해당 이벤트를 억제하는지 확인하지 못했으므로 추정에 머문다.** `SW_HIDE` 쪽은 이미 숨겨진 창에서 no-op이라 이벤트를 만들지 않는다. 검증 방법은 §8의 Platform-specific 테스트에 적었다.

### 추정 — 진단 CLI와 트레이 앱의 설정 파일 동시 쓰기

`atomic_write`(`config.rs:237-248`)는 `path.with_extension("tmp")`라는 **고정 임시 파일명**을 쓴다. 단일 인스턴스 mutex는 일반 실행만 보호하고 `--self-check`는 mutex 밖에서 `ensure_runtime_files` / `load_settings`(경우에 따라 `heal_default` → `atomic_write`)를 수행한다. 따라서 트레이에서 설정을 토글하는 순간 콘솔에서 `--self-check`를 돌리면 두 프로세스가 같은 `layout_settings_v11.tmp`를 다룬다. 최악의 경우 설정 1회분이 유실되거나 부분 기록된 파일이 rename될 수 있다. 창이 극히 좁고 영향도 "다음 실행에서 기본값으로 self-heal" 수준이라 우선순위는 낮지만, 임시 파일명에 PID를 붙이면 비용 없이 사라진다.

### 추정 — `get_run_command`의 1023자 상한

`kakao-win32/src/startup.rs:35`는 `vec![0u16; 1024]` 고정 버퍼를 쓰고 `ERROR_MORE_DATA`를 별도로 처리하지 않아, 더 긴 값은 `None`(= "missing")으로 보고된다. 그 결과 `should_repair_registration("missing")`이 참이 되어 기존 값을 덮어쓴다. 다만 덮어쓰는 대상은 **이 앱 자신의 값 이름(`KakaoTalkAdBlockerLayout`)**뿐이고, 1023자를 넘는 Run 명령은 현실적이지 않다.

---

## 6. Documentation Mismatches

실제 불일치가 존재한다. 아래 6건이다.

| # | 문서 | 문서의 서술 | 실제 구현 | 근거 |
|---|---|---|---|---|
| 1 | README §설정 | `poll_interval_ms`: "카카오톡 활성 상태에서의 탐지 주기 (기본값 50ms)" | Windows + WinEvent 훅 정상 설치 시 값이 사용되지 않는다. 실제 주기는 `idle_poll_interval_ms` | `engine.rs:400` → `engine.rs:433`(hook `None`일 때만 도달) |
| 2 | README §설정 | 예시 JSON에 `cache_cleanup_interval_ms`, `start_minimized` 포함 | 런타임 참조 0건 / 대입만 하고 읽지 않음 | `config.rs:32`, `config.rs:24`, `lib.rs:127` |
| 3 | README §CLI | `--self-check`: "레지스트리, 프로세스, 설정 파일 상태 점검", "시스템 권한, 프로세스 탐색" | 레지스트리·프로세스 점검 없음. Windows 여부 + APPDATA 존재/쓰기 + 설정 로드 경고만 | `self_check.rs:7-44` |
| 4 | CLAUDE.md §설정 파일 | 파손 백업 파일명 `*.broken-YYYYMMDD-HHMMSS` | Rust는 `*.broken-<unix-epoch>` (README는 올바르게 `<unix-epoch>`로 기술) | `config.rs:206-211`, `config.rs:331-337` |
| 5 | CLAUDE.md §ui.py, §event_engine | 트레이 상태 문자열에 `메인윈도우`, `누적 숨김/닫힘/리사이즈`, 마지막 오류/갱신시각 노출. `closed_windows`는 창 소멸 확인 시 증가 | Rust 트레이에 상태 표시가 전혀 없고, `closed_windows`는 증가 코드 자체가 없어 항상 0 | `tray.rs:344-388`, `evaluate.rs:112/128`(증가 지점 0건) |
| 6 | CLAUDE.md §event_engine | popup dismiss는 "실제 close/hide/zero-size 성공 여부를 검증하며 실패 시 상태(`last_error`)와 로그에 반영" | `WM_CLOSE` 결과를 버리고 항상 fallback 적용. 실패 로깅 없음 | `engine.rs:104` |

**참고(불일치 아님):** CLAUDE.md의 모듈 설명 상당 부분은 명시적으로 "Python 참고 구현의 알고리즘 계약"으로 선언되어 있다. #5·#6은 그 선언 범위 안에 있으나, Rust가 기본 구현이라는 같은 문서의 서술과 합쳐 읽으면 독자가 Rust 동작으로 오해하기 쉬워 불일치로 올렸다. 버전 표기(`config.rs` `VERSION = "11.1.3"`, `packaging/windows_version_info.txt` `11.1.3.0`, README 배지)는 **일치**한다.

---

## 7. Recommended Fix Plan

### Phase 1 — Immediate

데이터 손상이나 보안 사고로 직결되는 항목은 발견되지 않았다. 이 단계는 "방치 시 사용자 환경을 눈에 띄게 악화시키는 자원 문제"로 구성한다.

1. **ISSUE-001 + ISSUE-002를 한 묶음으로 수정.** 로그 회전을 주기적으로 수행하거나 rolling writer로 교체하고, 동시에 복원 재시도에 상한·백오프·1회성 경고를 도입한다. 둘 중 하나만 고치면 증상이 절반만 사라진다.
2. **ISSUE-005 수정.** `lib.rs:208`의 인자 정규화를 `main.rs`와 일치시킨다. 한 줄 변경이고 사용자 체감이 가장 크다.

### Phase 2 — Stability

3. **ISSUE-003 정리.** `poll_interval_ms` / `cache_cleanup_interval_ms` / `start_minimized`를 실제로 연결하거나, README에서 제거하고 로드 시 "미사용 필드" 경고를 남긴다. 어느 쪽이든 문서와 코드를 같은 커밋에서 맞춘다.
4. **ISSUE-006 완화.** WinEvent 훅을 카카오톡 PID로 한정하고(PID 변경 시 재설치), `EVENT_TX`의 `OnceLock`을 재설치 가능한 구조로 바꾼다. 최소 조치로는 `hook_proc` 내 1차 이벤트 종류 필터만 넣어도 채널 포화가 크게 줄어든다.
5. **popup dismiss 결과 로깅 추가**(§5). `send_message_timeout` 실패와 fallback 적용 사실을 `debug!`/`warn!`으로 남기고, `popup_hide_fallbacks`가 항상 증가하는 `hidden_ok` 로직(`evaluate.rs:675`)을 실제 이전 가시성 기준으로 바로잡는다.
6. **`--self-check` 보강**(§5). Run 레지스트리 읽기/쓰기 접근과 프로세스 열거를 점검 항목에 추가하고, `--strict-self-check`의 실패 판정을 "치명적 경고"로 한정한다.
7. **`atomic_write` 임시 파일명에 PID 삽입**(§5). 진단 CLI와 상주 앱의 동시 쓰기 창을 없앤다.
8. **업데이터 강건화**(§5). 재실행 시 원래 인자 전달, 롤백 경로의 rename 오류 로깅, 헬퍼에 기대 SHA-256 전달 후 교체 직전 재검증.

### Phase 3 — Structural

9. **ISSUE-004 구조 분리.** `evaluate_graph_with_states`를 "apply 전용"과 "진단 페이로드 포함" 두 진입점으로 나눠 hot loop에서 중복 순회와 맵 clone을 제거한다. `inspect_candidates`와 `apply_once`의 중복된 세 블록(main child / candidate / popup)을 공용 순회로 통합하면 두 함수가 갈라져 계약이 어긋나는 위험도 함께 준다.
10. **트레이 상태 노출 경로 신설**(§5). 메뉴 헤더 또는 `NIF_TIP` 툴팁에 확정 메인 윈도우 수·누적 숨김·복원 실패 수·마지막 오류를 싣고, `closed_windows` 증가 로직을 실제 창 소멸 확인 기준으로 구현한다. 그 뒤 CLAUDE.md §5·§6 서술과 코드를 일치시킨다.
11. **트레이 콜백 재진입 제거**(§5). `wnd_proc`에서 직접 사용자 콜백을 호출하지 말고 커맨드를 큐에 넣어 모달 밖에서 처리한다.
12. **문서 계층 정리.** CLAUDE.md에서 "Python 참고 구현 계약"과 "Rust 활성 런타임 동작"을 시각적으로 분리해, §6 #5·#6 같은 오독을 구조적으로 막는다.

---

## 8. Test Recommendations

### Unit

- **`should_attach_parent_console` (ISSUE-005):** 입력 `["C:\App\KakaoTalkLayoutAdBlocker_v11.exe", "--startup-launch", "--minimized"]` → 기대 `false`. 입력 `["C:\App\KakaoTalkLayoutAdBlocker_v11.exe", "--self-check"]` → 기대 `true`. 입력 `["C:\App\app.exe"]`(인자 없음) → 기대 `false`.
- **`rotate_log_if_needed` (ISSUE-001):** 5MB + 1바이트 파일 → 호출 후 원본이 없거나 0바이트이고 `*.log.1`이 존재. 정확히 5MB 파일 → 회전하지 않음(경계값).
- **`restore_stale_hidden` 재시도 상한 (ISSUE-002):** 항상 실패하는 스텁으로 20회 호출 → 누적 실패 증가분이 설정한 상한(예: 5) 이하이고, `warn!` 상당 이벤트가 1회만 기록됨.
- **설정 필드 효력 (ISSUE-003):** `poll_interval_ms`를 바꾼 두 설정으로 워커 대기 시간이 달라지는지 단언. 현재 구현에서는 실패해야 하며, 이 테스트가 곧 수정 완료 기준이 된다.
- **`dismiss_popup` 카운터 (§5):** 직전 `visible == true`인 팝업 → `popup_hide_fallbacks += 1`. 직전 `visible == false`인 팝업 → `popup_hide_fallbacks` 불변, `popup_zero_size_fallbacks += 1`.
- **`get_run_command` 길이 (§5):** 2000자 Run 값을 주입한 fake 레지스트리 → `Some(전체 문자열)` 반환, `None` 아님.

### Integration

- **apply 경로 최적화 등가성 (ISSUE-004):** `tests/fixtures/window_dumps/` 전 fixture에 대해 `evaluate_graph_for_apply`(신규)와 기존 `evaluate_graph_with_states`의 `actions` + `state`가 **바이트 단위로 동일**함을 단언. 동시에 `--dump-tree` 산출 `candidates` 배열이 변하지 않음을 단언.
- **self-check 점검 항목 (§5):** Run 레지스트리 접근 불가를 시뮬레이션한 상태에서 `--self-check --json` → 페이로드에 해당 경고가 포함되고, 비-strict 모드 종료 코드는 0, `--strict-self-check`에서는 1.
- **설정 동시 쓰기 (§5):** 같은 설정 경로를 향해 `save_settings`와 `heal_default`를 서로 다른 임시 파일명으로 동시 수행 → 최종 파일이 항상 유효한 JSON이고 `*.tmp` 잔재가 남지 않음.

### End-to-End

- **실제 카카오톡 기준 광고 제거 (미보유 커버리지):** 카카오톡 실행 → `--dump-tree-series --dump-series-duration-ms 2000 --dump-series-interval-ms 50` 수집 → `owned_popups`에 `EVA_Window_Dblclk`(빈 텍스트) + `Chrome_WidgetWin_1`/`Chrome Legacy Window` 호스트가 존재하고, 엔진 실행 후 해당 hwnd가 `actions.hide`에 포함되며 화면에서 배너가 사라짐을 확인. 결과 덤프를 새 골든 fixture로 고정.
- **업데이트 전체 왕복:** 테스트용 Ed25519 키로 서명한 로컬 매니페스트 + 아티팩트 → `check_for_update` → `prepare_update` → `launch_helper` → 교체 후 새 프로세스가 뜨고 `.exe.old`가 정리되는지. 아티팩트 1바이트 변조 시 `업데이트 파일 해시가 일치하지 않습니다.`로 중단되고 현재 EXE가 **그대로 유지**되는지.
- **중복 실행 (ISSUE-005):** 트레이 상주 중 EXE 재실행 → 메시지 박스가 표시되고 종료 코드 0이며, 기존 인스턴스의 차단 동작이 중단되지 않음.

### Concurrency

- **종료 중 재은닉 방지:** `stopping = true` 직후 `apply_evaluation`을 호출 → `hide`/`close`/`set_pos`가 단 한 건도 발행되지 않음(`engine.rs:81-98`의 precheck 보증 회귀).
- **차단 OFF ↔ ON 빠른 토글:** 100ms 간격으로 `enabled`를 20회 뒤집은 뒤 최종 상태가 ON이면 광고가 숨겨져 있고, OFF면 모든 스냅샷이 복원되어 `snapshots`가 비어 있음.
- **업데이트 single-flight:** `try_begin_update`를 8개 스레드에서 동시 호출 → 정확히 1개만 true. 이미 `updater.rs:538-545`에 단일 스레드 버전이 있으므로 멀티스레드로 확장.
- **WinEvent 채널 포화 (ISSUE-006):** 무관 PID 이벤트 2000건을 주입한 뒤 카카오톡 PID 이벤트 1건이 `drain` 결과에 반드시 포함되는지.

### Regression

- **복원 실패 카운터 의미:** 실패 창 2개가 5 tick 동안 지속될 때 `restore_failures`가 10이 아니라 2에 수렴하는지(ISSUE-002 수정 기준).
- **기존 골든 고정 유지:** `tests/fixtures/window_dumps/owned_popup_legacy_ad.json`의 owned popup 경로, `normal_main_window.json`의 "메인 뷰 리사이즈는 스냅샷에 저장하지 않음" 단언은 어떤 리팩터링에서도 깨지지 않아야 한다(현재 `restore_regression.rs` 8건이 이를 지키고 있음).
- **Python 골든 병행:** 이번 감사에서 실행하지 못한 `pytest tests/`를 CI 외 로컬에서도 돌릴 수 있도록 `requirements-dev.txt` 설치 절차를 README 개발 섹션에 명시하고, Rust `golden_parity`와 Python 골든이 같은 fixture를 참조하는지 확인.

### Platform-specific

- **`SetWindowPos` 이벤트 유발 여부 (§5 추정 검증):** Windows에서 테스트 창을 0,0,0,0에 둔 뒤 동일 좌표로 `SetWindowPos(SWP_NOZORDER|SWP_NOACTIVATE)`를 100회 호출하며 `SetWinEventHook`으로 `EVENT_OBJECT_LOCATIONCHANGE` 수신 횟수를 센다. 0이면 자기 유발 루프 가설은 반증되고, >0이면 `keep_hidden_popup`의 무조건 재적용을 조건부로 바꿔야 한다.
- **조상 hidden 상태의 복원 (ISSUE-002 핵심 조건):** 부모 창을 `SW_HIDE`한 뒤 자식에 `SW_SHOW` → `IsWindowVisible(자식)`이 false임을 확인해, 복원 실패 판정이 구조적으로 불가피함을 고정.
- **비Windows 빌드:** `cargo test --workspace`가 Linux/macOS에서도 컴파일되는지(현재 `#![cfg(windows)]` 모듈이 다수이고 `lib.rs:132-140`에 non-windows 폴백이 있으므로 의도된 경로). 실행 시 종료 코드 2 반환도 함께 확인.
- **로그온 직후 트레이 등록:** 실제 재부팅 후 `--startup-launch --minimized`로 자동 실행 → `Shell_TrayWnd` 대기와 NIM_ADD 재시도를 거쳐 아이콘이 결국 등록되는지(`--startup-trace`로 파일 기록). v11.1.3의 핵심 수정 지점이라 릴리스마다 수동 확인 가치가 있다.

---

## 9. Final Assessment

| 항목 | 평가 | 근거 |
|---|---|---|
| **Functional Correctness** | **Good** | 광고 판정 계약(legacy signature / aggressive token / guarded popup / empty EVA close)이 규칙 문서와 일치하고, 골든 fixture 기반 회귀가 이를 고정한다. 이번 감사에서 판정 로직 자체의 오류는 발견하지 못했다. 감점 요소는 효과 없는 설정 필드(ISSUE-003)와 항상 0인 카운터로, 판정 결과가 아니라 주변 계약의 문제다. |
| **Runtime Stability** | **Needs Work** | 크래시나 교착은 발견되지 않았고 종료 경로(stopping → join → restore)도 정연하다. 그러나 장기 상주를 전제로 한 제품에서 로그 회전이 1회뿐이고(ISSUE-001) 복원 재시도에 상한이 없다(ISSUE-002). 며칠 단위 세션에서 자원이 단조 증가할 수 있는 경로가 열려 있다. |
| **Data Integrity** | **Good** | 설정/규칙은 tmp + rename 원자 교체, 파손 시 백업 후 self-heal, 백업 보존 정책까지 갖췄다. 업데이터는 `.exe.old` 백업과 교체 실패 롤백을 구현했고 `updater_tests` 4건이 "실패 시 현재 EXE 보존"을 고정한다. 남은 것은 고정 tmp 파일명으로 인한 좁은 동시 쓰기 창 하나뿐이다. |
| **Error Resilience** | **Acceptable** | 설정 파손, 트레이 생성 실패, NIM_ADD 실패, 업데이트 검증 실패, 헬퍼 부재 등 주요 실패 경로가 모두 명시적으로 처리되고 사용자에게 보고된다. 반면 `WM_CLOSE` 결과를 버리는 지점(§5), 롤백 rename 오류를 버리는 지점(§5), 복원 실패가 무한 반복되는 지점(ISSUE-002)처럼 **실패를 조용히 삼키거나 영원히 재시도하는** 패턴이 남아 있다. |
| **Cross-platform Robustness** | **Good** | Windows 전용을 명시적 설계 목표로 선언하고 비Windows에서 종료 코드 2로 fail-fast한다. `#![cfg(windows)]`와 non-windows 폴백(`FakeWin32`)으로 크레이트가 다른 OS에서도 컴파일·테스트되도록 경계가 잘 잡혀 있다. 경로 처리도 `\\?\` 접두사 정규화까지 고려했다. 목표 대비 적합하다. |
| **Test Confidence** | **Acceptable** | 60건이 전부 통과하고, 복원/그래프 직계 자식/설정 마이그레이션/업데이터 실패 보존 등 **과거에 실제로 터졌던 회귀**를 정확히 겨냥하고 있다. 다만 ①실제 카카오톡 E2E 부재 ②Win32 부작용(이벤트량, `IsWindowVisible` 전파) 미커버 ③설정 필드가 "파싱되는지"만 검증하고 "효과가 있는지"는 검증하지 않아 ISSUE-003 같은 구멍을 구조적으로 놓친다. 통과를 실제 동작 보증으로 읽을 수 없다. |

### 실제로 먼저 수정할 문제 3개

1. **[ISSUE-001 + ISSUE-002] 로그 회전 주기화 + 복원 재시도 상한/백오프.** 두 문제가 서로를 증폭하므로 한 커밋에서 함께 고쳐야 한다. 장기 상주라는 이 제품의 기본 사용 방식에서 유일하게 자원이 단조 증가하는 경로다.
2. **[ISSUE-005] 중복 실행 안내 메시지 복구.** `std::env::args()` → `.skip(1)` 한 줄. 사용자 체감 대비 수정 비용이 가장 낮고, 동일 헬퍼를 두 호출부가 다르게 쓰는 구조도 함께 정리할 수 있다.
3. **[ISSUE-003] 효과 없는 설정 3종 정리.** 문서가 보증한 제어 수단이 실제로는 없는 상태다. 연결하든 문서에서 걷어내든, 코드와 README를 같은 커밋에서 일치시켜야 이후 "반응 속도" 관련 제보를 오진하지 않는다.

> ISSUE-004(hot loop 2배 작업)는 영향이 성능에 한정되고 수정 범위가 `evaluate.rs` 구조 변경이라 위 3개 다음 순번으로 둔다. 다만 README의 핵심 홍보 문구와 직결되므로 Phase 3에서 반드시 다뤄야 한다.

---

## 10. 부록 — 이전 감사(v11.1.1) 이슈의 현재 상태

v11.1.1 감사본의 High-Risk 9건을 현재 소스에서 재확인한 결과 **전부 해소되어 있다.** 본 감사에서는 재보고하지 않는다.

| 이전 이슈 | 현재 상태 | 확인 근거 |
|---|---|---|
| ISSUE-001 Win32 자손을 직계 자식으로 저장 | 해소 | `graph_build.rs:68` `api.get_parent(child) == hwnd` 필터 + `real.rs:65-74` 이중 필터, `tests/graph_direct_children.rs` |
| ISSUE-002 숨김 fallback 팝업 반복 재노출 | 해소 | `find_popup_matches(..., require_visible=false)`(`evaluate.rs:641`)와 `keep_hidden_popup`(`evaluate.rs:685`) |
| ISSUE-003 복원 실패 스냅샷 소실 | 해소 | `engine.rs:167`, `engine.rs:234`에서 실패 시 재삽입 (단, 재시도 정책은 ISSUE-002로 신규 지적) |
| ISSUE-004 JSON 타입 오류 panic/전체 초기화 | 해소 | `merge_typed_settings`(`config.rs:137-177`), `merge_typed_overlay`(`rules.rs:99-138`) 필드 단위 병합 + 경고, `tests/config_migration.rs` 6건 |
| ISSUE-005 트레이 실패 시 제어 불가 | 해소 | `lib.rs:441-461` stop → join → 종료 코드 1 |
| ISSUE-006 헬퍼 미포함 설치 | 해소 | `resolve_helper`(`updater.rs:345-364`)가 같은 폴더 우선 탐색 + MZ 검증, README 안내 |
| ISSUE-007 업데이트 종료가 복원을 기다리지 않음 | 해소 | `lib.rs:477-500` worker.join() 이후에만 `launch_helper` |
| ISSUE-008 중복 업데이트/staging 충돌 | 해소 | `try_begin_update` CAS(`updater.rs:291-295`) + `unique_staging_path`(pid/시각/시퀀스, `updater.rs:373-381`) |
| ISSUE-009 진단 보고서 허위 성공 | 해소 | `self_check.rs:45-70` 보고서 I/O 실패 시 exit code·core 라벨 갱신, `tests/self_check_report.rs` |
