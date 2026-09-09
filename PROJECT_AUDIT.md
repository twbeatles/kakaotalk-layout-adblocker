# Project Audit

감사 일자: 2026-09-06 (Asia/Seoul)
대상: KakaoTalk Layout AdBlocker 11.1.1 / commit `b3f2d18fcca005a590e4a1aa6d3797eb5b6d3a61`
감사 방식: 문서 → CodeGraph MCP → 누락 부분 소스 확인 → 기존 테스트 및 격리 재현 → 반증 확인. 기존 감사 문서를 현재 구현 기준으로 갱신했다. 소스·설정·빌드 스크립트는 수정하지 않았다.

## Remediation status (v11.1.3)

2026-09-09 구현으로 시작프로그램 재부팅 미실행 경로를 보완했다. 아래 11.1.1 감사 본문은 당시 기록으로 유지한다.

| 항목 | v11.1.3 |
|---|---|
| 로그온 NIM_ADD 실패 후 즉시 종료 | 수정. `Shell_TrayWnd` 대기, 수 초 NIM_ADD 재시도, 실패 시에도 메시지 루프 유지 후 `TaskbarCreated`/타이머 재등록. |
| 낡은/누락 EXE Run 경로를 source-compatible로 오인 | 수정. `missing_target`/`stale`은 현재 EXE로 복구. source-compatible는 패키지 EXE가 실제로 있을 때만. |
| Windows `StartupApproved` disabled | 수정. Run 등록 시 enable blob 기록. healthy 등록도 꺼진 시작 앱 상태를 다시 켬. |
| `\\?\` Run 경로 | 수정. `current_exe` 경로에서 접두사 제거. |

## Remediation status (v11.1.2)

2026-09-06 구현으로 아래 항목을 반영했다. 이 문서는 11.1.1 시점의 감사 기록으로 유지한다.

| 이슈 | v11.1.2 |
|---|---|
| ISSUE-001 직계 자식 그래프 | 수정. `GetParent`로 직계 에지를 구성하고 flattening enumerator 회귀 테스트를 추가했다. |
| ISSUE-002 숨김 팝업 재노출 | 수정. 숨긴 팝업도 시그널을 재평가하고 시그널이 있을 때 복원하지 않는다. |
| ISSUE-003 복원 실패 스냅샷 | 수정. 실패 시 스냅샷을 유지하고 재시도한다. |
| ISSUE-004 typed JSON panic/초기화 | 수정. 필드 단위 병합, 경고, 정상 필드 보존. |
| ISSUE-005 트레이 실패 대기 | 수정. stop→restore→join 후 종료 코드 1. |
| ISSUE-006 헬퍼 미포함 설치 | ZIP/같은 폴더 헬퍼 + README/CI. EXE 임베드는 하지 않음. |
| ISSUE-007 업데이트 종료 복원 | 수정. 헬퍼는 워커 join 뒤에만 실행. |
| ISSUE-008 중복 업데이트 | 수정. single-flight와 고유 staging. |
| ISSUE-009 진단 허위 성공 | 수정. 보고서 I/O 실패·strict·트레이 실제 결과. |
| 기타 갭 | HTTP timeout, TaskbarCreated, Run missing 복구, bootstrap, 로그 회전, 후보 정리, dump-series 상태 유지. |
| 남은 항목 | 헬퍼 relaunch handshake/즉시 종료 롤백, 실제 카카오톡 E2E. |

## 1. Executive Summary

**전체 상태: Needs Work. 전체 위험도: High.** 기본 광고 판정과 일반 복원은 구현되어 있고 기존 Rust 테스트 36개 및 Python 골든 테스트 4개가 통과한다. 그러나 실제 Win32 열거 계약, 팝업 차단의 연속 상태 전이, 설정 오류와 트레이 실패 경로에는 기능 장애가 남아 있다. 테스트 통과를 실제 카카오톡 전체 흐름의 보증으로 해석할 수 없다.

가장 중요한 문제는 다음 다섯 가지다.

1. **ISSUE-001:** 모든 자손을 반환하는 Win32 API 결과를 직계 자식 목록으로 저장한다. 팝업 탐색 깊이 제한과 직접 자식 판정의 의미가 바뀐다.
2. **ISSUE-002:** 닫히지 않아 숨김 처리한 팝업이 두 번의 미검출 후 다시 표시된다. 동일 광고가 남아 있어도 숨김·표시를 반복한다.
3. **ISSUE-004:** 문법상 유효한 rules JSON의 필드 타입 오류가 앱 시작을 panic으로 중단시킨다. settings는 필드 하나가 잘못되면 정상 필드까지 조용히 기본값으로 바뀐다.
4. **ISSUE-003:** 복원 실패 스냅샷을 제거하므로 같은 프로세스에서도 재시도할 수 없다.
5. **ISSUE-005:** 트레이 생성 실패 시 종료·복원 없이 워커를 기다려 제어할 수 없는 백그라운드 상태가 된다.

카카오톡 대화 DB나 사용자 문서를 쓰는 경로는 발견하지 않았다. **대화 데이터 유실·파괴를 확인한 것은 아니다.** 다만 설정값의 의도하지 않은 초기화·후속 저장과 복원 메타데이터 소실은 확인했다. 업데이트 중단·실패에 따른 실행 파일 가용성 및 롤백 한계는 별도로 남아 있다. Critical로 확정할 근거는 없다.

먼저 실제 창 트리 계약, 팝업 상태 유지, 설정 타입 오류 처리를 수정해야 한다. 일반적인 기능 추가보다 실패 경로와 연속 프레임 회귀 검증이 우선이다.

## 2. Project Understanding

- **목적:** Windows PC 카카오톡의 광고 창을 Win32 hide/resize/guarded close로 처리하는 layout-only 도구. hosts/DNS/네트워크 차단은 수행하지 않는다. 시작프로그램 기능에는 HKCU Run 레지스트리를 사용한다.
- **규칙:** README와 CLAUDE의 고정 알고리즘 계약을 기준으로 확인했다. 실제 루트 `AGENTS.md` 파일은 없으며, 사용자가 제공한 AGENTS 지침(CodeGraph 우선, 기본 main 작업)을 적용했다. `.specify/memory/constitution.md`는 미완성 템플릿이므로 구체적인 기능 보장으로 취급하지 않았다.
- **엔트리포인트:** `rust/crates/kakao-app/src/main.rs:6` → `run_with_args`; 업데이트 헬퍼는 `rust/crates/kakao-updater/src/main.rs:48`. 루트 Python 스크립트와 `legacy/python-v11`은 활성 Rust 런타임과 분리된 안내/회귀 참고 구현이다.
- **크레이트:** `kakao-core`(그래프·시그널·레이아웃·판정), `kakao-win32`(실제 API/Fake·프로세스·이벤트·뮤텍스·트레이·Run), `kakao-app`(CLI·워커·설정·진단·다운로드), `kakao-updater`(프로세스 대기·교체·재실행). Cargo workspace는 4개다.
- **저장:** `%APPDATA%\KakaoTalkAdBlockerLayout`의 settings/rules JSON과 append 로그. JSON 저장은 임시 파일 후 rename. 복원 스냅샷, weak-signal 상태 및 stale 카운터는 워커의 메모리 HashMap이며, 토글은 Arc/Atomic으로 전달된다. DB, migration transaction, SQL, 외부 데이터 저장 API는 없다.
- **외부 의존성:** Windows User32/Kernel32/Shell/Registry, `windows` 0.61, serde/serde_json, clap, crossbeam-channel, tracing, ureq 2.12.1(Cargo.lock), Ed25519/SHA-256. 업데이트는 GitHub release manifest/EXE에 의존한다. Python은 골든/배포 보조 도구용이다.
- **실행/빌드:** `cd rust; cargo run -p kakao-app --release`. `scripts/build_release.ps1`은 앱과 헬퍼를 빌드·복사한다. release workflow는 앱·헬퍼·서명 manifest를 별도 asset으로 올린다.

핵심 실행 흐름:

```text
CLI → OS/옵션 검사 → APPDATA·로그·설정 로드
  ├─ self-check → 진단 JSON/파일/exit code
  ├─ dump/series/shadow → PID → build_graph → pure evaluation → JSON/출력
  └─ 일반 실행 → Named Mutex → SharedFlags → worker + tray
       worker → PID/liveness + WinEvent/주기적 재검사 → build_graph
       → evaluate_graph_with_states → apply_evaluation
       → PID/HWND/class 재검증 → snapshot → Win32 close/hide/set-position
       → stale restore / OFF 전환 restore / 정상 종료 restore

트레이 토글 → 설정 저장 성공 → Atomic 상태 변경 → 다음 worker 반복에서 반영
시작프로그램 토글 → Run 레지스트리 변경 → JSON 저장 → 실패 시 역변경 시도
업데이트 확인 → manifest 다운로드·Ed25519/버전/URL/크기 검증
→ 확인 대화상자 → EXE 다운로드·크기/SHA-256 검증 → temp 파일
→ helper spawn → 앱 exit → helper wait → backup/replace → spawn → backup 정리
```

정상 종료는 워커를 join하지만 업데이트 종료는 같은 경로를 사용하지 않는다. 진단 분기는 일반 뮤텍스 이전에 실행되므로, 진단 중 설정 복구와 로그 쓰기가 전혀 없다고 볼 수는 없다.

## 3. Audit Coverage & Limitations

### 확인 범위

- README, CLAUDE, 제공된 AGENTS 규칙, Cargo workspace/앱 의존성/lockfile, Python 개발 의존성, 빌드·개발 검증 스크립트, Windows CI 및 release workflow.
- Rust 앱: `lib`, `engine`, `graph_build`, `config`, `dump`, `self_check`, `startup`, `updater`.
- Rust 코어: `graph`, `model`, `rules`, `signals`, `evaluate`, `layout`의 관련 실행 경로.
- Win32: `real`, `fake`, `api`, `process`, `event_hook`, `single_instance`, `startup`, `tray`; 업데이트 헬퍼 및 관련 테스트.
- CodeGraph MCP를 실제 사용했다. `run_with_args → spawn_worker → restore_all`, `tick → build_graph/evaluate_graph_with_states/apply_evaluation`, `enum_children → enum_descendants/find_popup_matches`, `update_executable → helper main/tests` 등의 호출·영향 관계를 확인했다.
- CodeGraph에는 Rust/Python의 동명 심볼 혼재와 일부 잘린 본문, 반복적인 pending-sync 경고가 있었다. Rust 경로로 구분하고 누락/갱신 경고 부분만 직접 읽었다. 콜백·스레드 종료 순서는 실제 caller 본문과 함께 확인했다. 그래프가 보여 준 Python 호출자를 Rust production caller로 간주하지 않았다.

### 실제 실행 결과

| 검증 | 결과 및 범위 |
|---|---|
| `cargo test --workspace --offline --locked` | **36 passed, 0 failed**, doc-test 0. 새 GUID 임시 디렉터리로 TEMP/TMP/APPDATA를 격리했다. |
| `cargo clippy --offline --locked --all-targets --all-features -- -D warnings` | 성공(exit 0). |
| `py -3.14 -m pytest -q -p no:cacheprovider tests/test_golden_decisions_v11.py` | **4 passed**. Python 전체 suite는 실행하지 않았다. |
| 기본 `python -m pytest ...` | Python 3.11 ARM64 환경에 pytest가 없어 실패. 설치하지 않고 기존 Python 3.14로 위 명령을 실행했다. |
| 저장소 밖 임시 Rust 하네스 | 현재 빌드된 라이브러리를 rustc로 링크. ISSUE-001/002/003/004를 모형·Fake 및 실제 loader로 재현했다. 첫 링크 시 파일명 패턴 오류가 있었으며 `lib*.rlib`로 바로잡은 후 실행 성공. |
| debug EXE `--self-check --strict-self-check --json --self-check-report <기존 디렉터리>` | 보고서 저장 불가능 조건인데 exit 0 및 `core=ok`. 실제 사용자 APPDATA 사용 안 함. |
| debug EXE `--self-check --json`, 격리 rules=`{"popup_search_depth":"2"}` | `merge rules` panic, **exit 101**. |

추가 하네스 출력:

```text
popup visibility per tick: [false, false, true, false]
failed restore: failures=1, retained=0
flattened depth<=2: [(2,1), (3,1), (4,1), (3,2), (4,2), (4,2)]
typed invalid: enabled=true, warnings=[]
```

하네스는 `%TEMP%\kakao-audit-0977dd295b0e43c5a79bb1cf50b90163\probe.rs`에 작성했다. 소스 저장소에는 추가하지 않았다. 임시 산출물은 보존했으며 기존 사용자 임시 폴더를 정리하지 않았다.

### 한계와 반증한 항목

- 실제 카카오톡 UI를 숨기거나 닫지 않았다. RealWin32 worker 테스트는 `apply=false`이며 Win32 smoke는 테스트 소유 창만 조작한다. 실제 카카오톡 버전별 E2E, Explorer 재시작, 로그온/로그오프, DPI/다중 모니터, 권한 상승된 카카오톡은 검증하지 않았다.
- 실제 업데이트 다운로드·배포·EXE 교체, 사용자 Run 키 변경, 디스크 고갈/전원 차단은 실행하지 않았다. 업데이트 이슈는 명시된 조건의 코드 분석이며 경합 피해를 재현했다고 주장하지 않는다.
- Linux/macOS는 제품 지원 대상이 아니다. 비Windows fail-fast 코드는 확인했지만 해당 OS 실행·교차 빌드는 하지 않았다.
- 전체 Python 회귀·Pyright·release packaging·성능 벤치마크는 실행하지 않았다. 기존 문서의 CPU 수치는 이번 측정값이 아니다.
- 2026-09-02 기존 감사의 weak-state 미누적, stale restore 부재, 업데이트 적용 미연결, 파일 로그 부재, 훅 실패 시 무슬립 주장은 **현재 코드에서는 반증되어 제외**했다. 상태 HashMap 유지, stale 처리, `apply_update` 호출, 파일 tracing, 폴백 sleep이 존재한다.
- 일반 UI mutex guard 수명, PID/class 재검증, popup unknown-title guard, settings 토글 저장 실패 롤백, 서명·해시 검증은 실제 보호 장치로 인정했다. `ShowWindow` 반환값을 일반 성공/실패 코드로 잘못 해석하지 않았다.

## 4. High-Risk Issues

이 절에는 근거가 확인된 문제만 수록한다. 섹션명과 별개로 개별 우선순위는 High/Medium을 구분한다. Confirmed는 코드/격리 테스트 확인을 뜻하며, 모두 실제 사용자 환경 재현을 뜻하지 않는다.

### [ISSUE-001] Win32 자손 목록을 직계 자식으로 저장하여 탐색 깊이 계약 위반

- **위치:** `kakao-win32/src/real.rs:58`, `kakao-app/src/graph_build.rs:28`, `kakao-core/src/graph.rs:107`, `kakao-core/src/signals.rs:239` (모두 `rust/crates/` 아래).
- **우선순위:** High
- **신뢰도:** Confirmed
- **문제:** RealWin32는 `EnumChildWindows`의 모든 자손을 그대로 반환한다. `load_tree`는 그 목록을 필터 없이 `set_children`에 넣는다. 실제 직계 관계와 그래프 edge가 달라진다.
- **발생 조건:** 카카오톡 창에 2단계 이상 자손이 존재. 깊이 제한 문제는 허용 깊이보다 깊은 AdFit 자손이 있을 때 두드러진다.
- **영향:** 깊이 3의 AdFitWebView가 깊이 1로 검출되어 닫기 대상에 포함될 수 있다. 직접 자식 시그니처 검사·덤프 계층도 왜곡되며 중복 탐색이 발생한다. 실제 잘못 닫힌 사용자 창은 관찰하지 않았다.
- **근거:** 전체 자손을 반환하는 wrapper로 `1→2→3→4`를 입력했을 때 `enum_descendants(1,2)`에 `(4,1)`이 포함됨. Win32가 손자도 열거한다는 계약은 [Microsoft EnumChildWindows 문서](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-enumchildwindows)로 확인했다.
- **반증 확인:** `visited`는 노드 중복 로드만 막고 잘못된 children edge는 정리하지 않는다. 일부 apply 경로의 parent 검사, PID/class 가드, popup title guard는 존재하나 `find_popup_matches`의 깊이를 교정하지 않는다. FakeWin32는 직계 자식만 반환하므로 기존 parity 통과가 반증이 되지 않는다.
- **호출/영향 범위:** CodeGraph의 `build_graph → load_tree → WindowGraph.enum_children/enum_descendants → find_popup_matches/evaluate` 및 dump. 모든 실제 스캔에 연결된다.
- **권장 수정 방향:** API 경계에서 직계 자식 계약을 명시하고 실제 parent에 따라 edge를 구성한다. owned popup의 owner와 structural parent 구분은 유지한다.
- **필요한 회귀 테스트:** 실제 또는 Win32 동등 열거기로 깊이 1/2/3 AdFit을 넣고 max_depth=2에서 깊이 3 제외, 각 직계 edge와 깊이의 정확성, owned-popup fixture 불변을 검증한다.

### [ISSUE-002] 숨김 fallback 팝업을 광고 신호 소멸로 오인해 반복 재노출

- **위치:** `rust/crates/kakao-core/src/evaluate.rs:633`, `signals.rs:239`; `rust/crates/kakao-app/src/engine.rs:201,235,279`.
- **우선순위:** High
- **신뢰도:** Confirmed
- **문제:** popup 판정은 visible host/descendant만 본다. 차단기가 직접 숨긴 창은 다음 평가의 action 집합에서 빠지고, action 기반 `matched_identities`가 이를 stale로 간주한다.
- **발생 조건:** WM_CLOSE 후에도 창이 남아 hide/zero-size fallback을 적용하고, 광고 클래스와 제목은 그대로 유지되는 경우.
- **영향:** 두 번의 miss 후 광고 창을 복원하고 다음 tick에서 다시 숨긴다. 차단이 켜진 상태에서 반복 노출·추가 close 요청이 발생한다.
- **근거:** 기존 `popup_adfit_webview.json`과 FakeWin32, 실제 `tick`을 연속 4회 호출했을 때 host 200의 visible이 `[false,false,true,false]`. Fake의 WM_CLOSE는 창을 남기도록 의도적으로 구현되어 현실적인 close 거부 상황을 모델링한다.
- **반증 확인:** 정상적으로 파괴된 창은 identity precheck에서 제외되므로 이 문제의 대상이 아니다. 2-miss grace는 재노출을 늦출 뿐이다. 숨김 원인과 무관하게 현재 action만 matched로 사용하며, 숨긴 popup을 별도로 재판정하지 않는다.
- **호출/영향 범위:** CodeGraph `tick → evaluate_graph_with_states → apply_once → dismiss_popup`, 이어 `matched_identities → restore_stale_hidden → restore_snapshot`.
- **권장 수정 방향:** 직접 숨긴 팝업은 visible 여부와 별도로 기존 시그널·host guard를 재평가하고 실제 시그널 소멸 때만 복원한다.
- **필요한 회귀 테스트:** close 거부 팝업을 20 tick 유지하면 계속 숨김, 광고 클래스/허용 제목이 변경되면 grace 후 복원, OFF와 종료는 즉시 복원해야 한다.

### [ISSUE-003] 복원 실패 스냅샷을 제거하여 재시도 정보 소실

- **위치:** `rust/crates/kakao-app/src/engine.rs:152,201,319`.
- **우선순위:** Medium
- **신뢰도:** Confirmed
- **문제:** `restore_all`은 drain 후 실패 항목을 돌려놓지 않는다. stale 복원도 먼저 remove한다. reset 메뉴는 실패 카운터만 초기화한다.
- **발생 조건:** 유효한 창에서 복원 SetWindowPos/표시 확인이 일시 실패하는 경우.
- **영향:** 원래 크기·가시성 정보가 없어져 이후 복원 재시도 불가능. 재활성화 시 이미 숨겨진 상태를 원본으로 저장할 수도 있다.
- **근거:** 정상 hide 후 복원 API 실패를 주입해 `failures=1, retained=0`. API를 정상화한 뒤 다시 restore해도 창은 숨김 상태였다.
- **반증 확인:** 다른 PID/class 또는 사라진 창을 버리는 것은 올바르다. 이번 재현은 identity가 유지된 창이다. 실패 카운터·로그는 남지만 snapshot 복구/별도 retry 큐는 없다. 정상 성공 테스트만으로 실패 재시도를 보장하지 않는다.
- **호출/영향 범위:** CodeGraph `spawn_worker → restore_all`(OFF/프로세스 미검출/종료), `tick → restore_stale_hidden`.
- **권장 수정 방향:** 성공 또는 identity 소멸 때만 제거하고 실패 스냅샷은 재시도 간격·횟수와 함께 유지한다.
- **필요한 회귀 테스트:** 1회 실패 후 성공 시 원본 rect/visibility 복원 및 큐 제거; 반복 실패 시 큐 보존; HWND 재사용 시 조작 없이 제거.

### [ISSUE-004] JSON 필드 타입 오류가 시작 panic 또는 전체 설정 초기화 유발

- **위치:** `rust/crates/kakao-core/src/rules.rs:83`; `rust/crates/kakao-app/src/config.rs:104,108,127`.
- **우선순위:** High
- **신뢰도:** Confirmed
- **문제:** rules overlay 역직렬화에 `expect("merge rules")`가 있다. settings는 역직렬화 실패 시 경고 없이 전체 기본값을 반환한다.
- **발생 조건:** JSON 문법은 맞지만 필드 타입이 틀림. 예: rules `{"popup_search_depth":"2"}`, settings `{"enabled":false,"poll_interval_ms":"bad"}`.
- **영향:** rules는 일반 실행·self-check·dump 모두 로드 단계에서 종료한다. settings는 사용자가 꺼 둔 차단이 켜지며 후속 트레이 저장 때 정상 필드까지 기본값으로 덮어쓸 수 있다.
- **근거:** 격리 APPDATA의 debug EXE에서 rules panic/exit 101. 임시 하네스에서 settings `enabled=true, warnings=[]` 확인.
- **반증 확인:** 문법 파손/최상위 object 검사·백업·heal은 있지만 필드 타입 오류는 통과한다. clamp/min-max 보정은 overlay 이후라 panic을 막지 못한다. 호출자에 catch/recovery가 없다.
- **호출/영향 범위:** CodeGraph `run_with_args/self_check::run → load_rules/load_settings → overlay/load_json`. 워커 시작 이전의 실제 production 경로다.
- **권장 수정 방향:** typed parsing을 Result로 반환하고 손상 필드/파일을 명시한다. 정상 필드 보존 또는 명시적인 안전 복구 정책을 적용하고, 백업 실패 시 원본 보존 정책을 정한다.
- **필요한 회귀 테스트:** 문자열 숫자, null, 음수/u32 overflow, 잘못된 배열 필드에서 panic 금지·경고 필수. `enabled=false` 보존과 정상 파일/필드 기본 보완을 각각 확인한다.

### [ISSUE-005] 트레이 생성 실패 시 제어·정상 종료 수단 없이 워커 대기

- **위치:** `rust/crates/kakao-app/src/lib.rs:234,384`; `rust/crates/kakao-win32/src/tray.rs:88`.
- **우선순위:** High
- **신뢰도:** Confirmed (실패 분기 코드 확인; 실제 Shell 실패 주입 미실행)
- **문제:** 워커를 먼저 시작하고 트레이 run_loop가 Err이면 stopping을 설정하지 않은 채 worker.join을 호출한다.
- **발생 조건:** 로그온 중 Shell 준비 지연, Shell_NotifyIcon 실패, 트레이 윈도우 생성 실패 등.
- **영향:** 워커가 계속 차단하며 아이콘·토글·종료 메뉴는 없다. 프로세스가 mutex를 보유해 재실행으로 제어 UI를 얻기도 어렵다. 강제 종료하면 정상 복원이 생략된다.
- **근거:** 트레이는 NIM_ADD를 100ms 후 한 번 재시도한 뒤 Err. 앱 실패 분기는 join만 하고, 워커 종료 조건은 stopping이다.
- **반증 확인:** 초기 1회 재시도는 존재하나 이후 재생성/대체 UI/자동 종료가 없다. 정상 메뉴 Exit에서만 실행되는 stop/join 경로까지 실패 분기가 도달하지 않는다.
- **호출/영향 범위:** CodeGraph `run_with_args → spawn_worker`, `tray::run_loop → run_loop_inner`; 실패 후 앱 전체 생명주기.
- **권장 수정 방향:** 실패 시 워커 stop→복원→join 후 명확한 오류 종료, 또는 사용자 제어가 가능한 대체 UI/제한된 재시도 구현.
- **필요한 회귀 테스트:** NIM_ADD 지속 실패에서 제한 시간 내 복원·종료 또는 대체 UI 제공, mutex 해제 및 다음 실행 성공.

### [ISSUE-006] README의 단일 EXE 설치로는 자동 업데이트 헬퍼가 없음

- **위치:** `rust/crates/kakao-app/src/updater.rs:304,337`; README 빠른 시작; `.github/workflows/release.yml`, `.github/workflows/windows-ci.yml`.
- **우선순위:** Medium
- **신뢰도:** Confirmed (배포·탐색 코드 기준)
- **문제:** README는 앱 EXE 하나 다운로드를 안내하지만 업데이트는 별도 `kakao-updater.exe`가 필요하다. `find_or_extract_helper`는 이름과 달리 추출하지 않고 파일 검색만 한다.
- **발생 조건:** 사용자가 새 폴더에 README대로 앱 EXE만 다운로드하고 업데이트를 승인함.
- **영향:** 다운로드 후 헬퍼 누락 오류로 자동 교체·재시작 실패. 수동 EXE 다운로드가 workaround다.
- **근거:** 정상 release에는 헬퍼가 별도 asset으로 업로드되지만 앱에 embed/download하는 코드가 없다. Windows CI artifact 업로드는 앱 EXE만 포함한다.
- **반증 확인:** 빌드 스크립트와 release workflow는 헬퍼를 실제 생성하므로 '헬퍼 구현 없음'이 아니다. 나란히 설치한 환경은 작동 가능하지만 문서의 설치 절차로는 그 전제가 충족되지 않는다.
- **호출/영향 범위:** CodeGraph `CheckUpdate → apply_update → find_or_extract_helper → Command::spawn`.
- **권장 수정 방향:** 검증 가능한 헬퍼를 포함한 ZIP/설치 패키지 또는 embedded helper를 제공하고 README/CI 산출물도 맞춘다. 헬퍼 준비 상태를 다운로드 전에 검사한다.
- **필요한 회귀 테스트:** 깨끗한 폴더·빈 TEMP에서 문서 절차로 설치한 산출물을 사용해 helper 준비와 업데이트 완료를 검증한다.

### [ISSUE-007] 업데이트 종료가 워커 복원을 기다리지 않음

- **위치:** `rust/crates/kakao-app/src/lib.rs:329,343`; `rust/crates/kakao-app/src/engine.rs:455` 부근 종료 처리.
- **우선순위:** Medium
- **신뢰도:** Confirmed
- **문제:** helper spawn 성공 직후 백그라운드 callback이 stopping=true를 저장하고 곧바로 `process::exit(0)`한다. 정상 tray 종료의 worker.join/restore를 통과하지 않는다.
- **발생 조건:** 실행 중 숨김 스냅샷이 있는 상태에서 업데이트 승인 후 helper 실행 성공.
- **영향:** 정상 종료와 달리 복원 완료가 보장되지 않는다. 새 앱이 숨겨진 현재 상태를 원본으로 수집하면 이후 OFF에서도 기존 visible 상태를 잃을 수 있다. helper 교체/재실행 실패 시에도 이전 숨김 상태가 남을 수 있다.
- **근거:** 스냅샷은 메모리 전용이고 restore는 워커 loop 종료 뒤 실행한다. stopping 저장과 즉시 process exit 사이에 completion handshake가 없다.
- **반증 확인:** 일반 Exit는 stop 후 join하므로 문제가 제한된다. helper는 파일 교체만 수행하고 HWND 스냅샷을 전달받지 않는다. 재시작은 원래 visibility를 복구하는 장치가 아니다.
- **호출/영향 범위:** CodeGraph `run_with_args` 업데이트 콜백 → `apply_update`, 병렬 `spawn_worker → restore_all`.
- **권장 수정 방향:** 메인 루프에 종료 요청을 전달하고 restore 완료 후 helper가 진행하도록 프로세스 생명주기를 통합한다. 숨겨진 진단 타이머 `--exit-after-startup-ms`의 직접 exit도 함께 정리한다.
- **필요한 회귀 테스트:** 광고 숨김→업데이트 준비→restore 완료→기존 앱 종료 순서 확인. helper 실패 및 새 앱 OFF 후에도 최초 visible/rect 보존.

### [ISSUE-008] 중복 업데이트 작업이 같은 임시 EXE와 교체 대상 공유

- **위치:** `rust/crates/kakao-app/src/lib.rs:329`; `updater.rs:337`; `rust/crates/kakao-updater/src/lib.rs:73`.
- **우선순위:** Medium
- **신뢰도:** Likely
- **문제:** 메뉴 클릭마다 새 업데이트 스레드/확인창을 만들고 busy guard가 없다. 같은 버전은 동일 TEMP 파일을 쓰며 helper에도 transaction lock이 없다.
- **발생 조건:** 다운로드가 진행 중일 때 다시 확인·승인하거나, 두 확인창에서 같은 업데이트를 승인한다.
- **영향:** 중복 다운로드·저장 충돌 및 helper 간 backup/replace 경쟁으로 업데이트 실패 가능. 첫 helper가 파일을 이동하면 다른 helper는 replacement를 찾지 못할 수 있다. 실제 실행 파일 손상은 재현하지 않았다.
- **근거:** Atomic 단일 실행 guard가 있는 것은 앱 시작 경로이며, 업데이트 스레드에는 없음. temp 이름은 버전만 포함하고 backup 이름도 고정이다.
- **반증 확인:** 앱 named mutex는 같은 프로세스 내부 스레드와 별도 helper를 직렬화하지 않는다. 해시 검증은 파일 저장 전 바이트에 적용되어 동시 교체 순서를 보호하지 않는다. 첫 작업의 exit가 모든 상황에서 두 번째 helper 생성을 막는다는 보장은 없다.
- **호출/영향 범위:** CodeGraph `CheckUpdate → apply_update → download_and_verify → helper main → update_executable`.
- **권장 수정 방향:** check/download/apply 전체에 single-flight 상태, 작업별 고유 staging 파일, 교체 대상별 helper lock 및 실패 정리를 적용한다.
- **필요한 회귀 테스트:** 지연 다운로드 중 10회 클릭·2회 승인에도 helper 1개만 실행되고 current/replacement/backup이 한 transaction에만 속해야 한다.

### [ISSUE-009] 진단 보고서 쓰기 실패와 트레이 성공 여부를 사실대로 보고하지 않음

- **위치:** `rust/crates/kakao-app/src/self_check.rs:7`; `rust/crates/kakao-app/src/lib.rs:40,86,237`.
- **우선순위:** Medium
- **신뢰도:** Confirmed
- **문제:** self-check 보고서 쓰기 오류를 버리고 OS/디렉터리 존재만으로 exit 0을 반환한다. strict 플래그는 전달·사용되지 않는다. startup trace는 트레이 생성 전에 성공 값을 상수로 기록한다.
- **발생 조건:** report가 디렉터리/쓰기 불가 경로, APPDATA가 존재하지만 쓰기 불가, 트레이 시작 실패.
- **영향:** 사용자·배포 smoke가 실패를 정상으로 판단한다. 트레이 미가용 문제를 진단하기 어렵다.
- **근거:** report 대상으로 기존 임시 디렉터리를 지정해 저장 불가능 상태에서도 `core=ok`, exit 0 재현. trace의 `tray_available=true` 기록은 `tray::run_loop` 호출보다 앞선다.
- **반증 확인:** 일반 dump의 `write_json` 실패는 exit 1로 처리되므로 모든 진단 쓰기가 문제인 것은 아니다. 빌드 스크립트가 일부 누락 보고서를 별도로 감지할 수 있지만 self-check의 반환 계약과 가짜 trace 값은 고쳐지지 않는다. Python 상세 self-check를 Rust 보장으로 사용하지 않았다.
- **호출/영향 범위:** CodeGraph `main → run_with_args → self_check::run`; startup trace → 빌드 스크립트의 packaged startup smoke.
- **권장 수정 방향:** I/O 실패를 exit/status에 반영하고 strict 검사항목을 정의한다. trace는 실제 트레이 초기화 결과를 받은 뒤 기록한다.
- **필요한 회귀 테스트:** report 디렉터리/권한 오류 시 nonzero, strict에서 필수 probe 실패 시 nonzero, tray 실패 trace=false 및 실제 오류 문자열.

## 5. Potential Functional Gaps

아래는 누락/보완 필요 영역이다. 별도 이슈와 중복 집계하지 않으며 추정은 현재 버그로 단정하지 않는다.

| 분류 | 내용·근거·보완 방향 |
|---|---|
| Confirmed Gap | **네트워크 전체/읽기 timeout·취소 없음.** `updater.rs:275,371`의 기본 ureq 호출에 deadline이 없다. 로컬 설치 소스 `ureq-2.12.1/src/agent.rs:256`에서 connect=30초, read/write=None을 확인했다. '모든 timeout이 없다'는 주장은 제외했다. 연결 후 응답 정지 시 작업이 끝나지 않을 수 있으므로 읽기/전체 deadline 및 제한된 retry를 추가해야 한다. 실제 원격 hang은 재현하지 않았다. |
| Confirmed Gap | **헬퍼 신뢰 검증 및 임시 경로 격리.** `find_or_extract_helper`는 주변 개발 경로와 고정 `%TEMP%\kakao-updater.exe`를 존재 여부만 보고 실행한다. 앱 EXE의 서명·해시 검증이 helper에는 적용되지 않는다. 해당 위치 쓰기 권한이 있는 로컬 주체/이전 파일을 전제로 하며 원격 무인 실행이나 권한 상승을 확인한 것은 아니다. 배포한 helper 검증 및 고유 staging 경로가 필요하다. |
| Confirmed Gap | **업데이트 시작 후 건강 확인 없음.** helper는 `Command::spawn` 성공 후 backup을 지운다. 새 프로세스가 즉시 설정 오류 등으로 죽는 경우는 성공으로 끝난다. relaunch 실패 분기의 rollback rename 결과도 무시한다. '모든 실패 시 자동 롤백' 보장 대신 시작 handshake와 오류별 복구 결과가 필요하다. 실제 교체 실패는 실행하지 않았다. |
| Confirmed Gap | **Explorer 재시작 후 트레이 재등록 없음.** wnd_proc에 TaskbarCreated 처리/재등록이 없다. 첫 생성 이후 Shell 재시작 시험과 아이콘 복구 정책이 필요하다. 실제 Explorer를 재시작하지 않았다. |
| Confirmed Gap | **Run 등록 상태와 JSON 동기화 없음.** startup.current_command/registration_health는 있으나 Rust production caller가 없다. 트레이 체크는 JSON에서 출발하므로 외부 삭제/이전 EXE 이동을 반영하지 못한다. 읽기 기반 상태 동기화와 관리 대상 명령만 복구하는 정책이 필요하다. 사용되지 않는 helper의 문자열 판정을 production 보안 버그로 올리지 않았다. |
| Confirmed Gap | **설정 bootstrap·로그 보존 정책 불완전.** 파일이 없으면 기본값을 메모리에만 반환하여 rules JSON이 자동 생성되지 않는다. 로그는 append만 하고 rotation은 없다. `log_level`, `cache_cleanup_interval_ms`도 현재 실행 제어에 연결되지 않는다. 첫 실행 편집용 파일 생성과 장기 실행 로그 상한을 검토한다. |
| Confirmed Gap | **후보 상태 정리 범위 부족.** 워커 states는 전체 PID가 없어질 때 clear하지만 앱이 장기간 살아 있는 동안 사라진 후보 identity를 주기적으로 제거하지 않는다. 장기 창 churn 시험이 필요하다. 실제 메모리 고갈은 확인하지 않았다. |
| Confirmed Gap | **덤프 series의 weak-state 연속성 없음.** 각 `dump_payload → evaluate_graph` 호출은 새 상태로 시작한다. weak signal이 여러 frame 유지되어도 진단 preview가 계속 pending일 수 있다. 실제 worker는 상태를 유지하므로 차단 기능 미구현으로 분류하지 않는다. preview 의미를 문서화하거나 series 전용 상태를 유지한다. |
| 추정 | 강제 종료/로그오프 후 cross-process 복원 저장이 필요할 수 있다. 현재 메모리 전용이라는 계약은 CLAUDE에 명시되어 있으므로 그 자체를 버그로 분류하지 않는다. 필요 시 HWND/PID 재사용을 고려한 설계가 선행되어야 한다. |

## 6. Documentation Mismatches

불일치가 있다. CLAUDE는 많은 상세 계약을 **Python 참고 구현**이라고 명시하므로 그 전체를 Rust 구현 약속으로 간주하지 않았다.

| 문서 설명 | 실제 구현/판정 |
|---|---|
| README 단일 EXE, Python 등 별도 의존성 없이 원클릭 업데이트 | 일반 차단 실행은 단일 EXE로 가능하지만 자동 업데이트에는 별도 helper 필요. ISSUE-006. |
| README 손상 설정은 안전한 self-heal | JSON 문법 오류는 복구하지만 typed rules 오류는 panic, settings typed 오류는 전체 기본값. ISSUE-004. |
| README OFF 시 즉시·완벽 복원 | 워커 관찰 시점에 복원하며 실패 snapshot 보존이 없다. 일반 view 크기 복원은 문서 다른 부분에서 카카오톡 자체 갱신에 맡긴다고 제한하고 있어 '모든 view 크기를 반드시 복원해야 한다'는 별도 버그는 제외. |
| README 백업 30일 후 정리, `broken-YYYYMMDD-HHMMSS` | Rust는 epoch seconds 이름이며 개수 10개 초과만 정리한다. 30일 보존 기준 없음. |
| README 활성 50ms/유휴 200ms, 포커스 변화 시 burst 횟수 | 현재 정상 hook 경로의 reconciliation은 idle interval 중심이고 burst 카운트는 PID 집합 변경에서 설정한다. 이벤트는 별도 처리하지만 foreground 기반의 50/200 전환 계약과 같지 않다. CPU 수치는 이번 감사에서 재측정하지 않음. |
| README '레지스트리를 건드리지 않음' | 광고차단 경로는 맞지만 선택적 시작프로그램 기능은 HKCU Run을 쓴다. 문구 범위를 구분할 필요. |
| README 아키텍처 3개 크레이트 | workspace는 updater 포함 4개. |
| CLAUDE `--dump-tree`가 owned popup 누락 | 현재 Rust는 `owned_popups` 필드에 별도 출력한다. `windows` 배열만 보는 소비자는 여전히 누락할 수 있다. |
| CLAUDE의 상세 Python 실패복원·tray recovery·설정 동기화·self-check 계약 | Python 참고 범위로 설명되어 있음. Rust에도 동일하다고 읽히지 않도록 지원/미지원 표를 두는 것이 좋다. 현재 Rust 격차는 ISSUE-003/005/009 및 위 Gap에 구분했다. |

## 7. Recommended Fix Plan

### Phase 1 — Immediate

1. **ISSUE-001:** 직계 자식/자손/owner의 API 계약을 바로잡고 실제 Win32 3단계 트리 회귀부터 고정한다. 알고리즘 휴리스틱은 변경하지 않는다.
2. **ISSUE-002:** 자체 숨김 popup 상태를 유지하고 연속 tick 재노출을 차단한다.
3. **ISSUE-004:** rules 타입 오류 panic을 제거하고 정상 settings 필드의 조용한 초기화를 방지한다.
4. **ISSUE-003/005/007:** 실패 snapshot 보존, tray 실패 종료, 업데이트 종료를 공통 stop→restore→join 경로로 연결한다.

### Phase 2 — Stability

1. **ISSUE-006/008:** 실제 배포에 helper 포함·검증, 업데이트 single-flight, 고유 staging 경로, helper 교체 lock을 마련한다.
2. 읽기/전체 timeout과 제한된 retry·취소, replacement 실패/재실행 실패/새 프로세스 즉시 종료의 복구 결과를 명시한다.
3. **ISSUE-009:** 진단 exit/status와 startup trace를 실제 I/O·트레이 결과에 연결한다.
4. Run 상태 동기화, Explorer 재등록, 설정 bootstrap·백업 연령·로그 rotation·후보 캐시 정리를 보완한다.

### Phase 3 — Structural

1. Win32Api의 자식 열거·좌표·visibility·close 결과 계약을 Fake와 Real에 공통 테스트로 고정한다.
2. OS 사용자 데이터와 분리된 실패 주입 seam을 tray/설정 I/O/다운로드/helper에 마련한다.
3. pure decision과 실제 mutation outcome을 구분하고, 진단 preview의 시간적 의미를 명시한다.
4. 테스트의 고정 TEMP 이름을 고유 경로로 바꾸어 동시 CI/local 실행 충돌을 막고, 릴리스 산출물 구성 자체를 테스트한다.

이 계획은 권고이며 코드 수정은 수행하지 않았다.

## 8. Test Recommendations

| 종류 | 입력/조건 | 기대 결과·관련 이슈 |
|---|---|---|
| Unit | rules 필드에 문자열 숫자/null/잘못된 배열; settings에 `enabled=false`와 다른 invalid 필드 | panic 없음, 정상 값 보존, 오류 위치·복구 경고 명시. ISSUE-004. |
| Unit | 정상 snapshot + 복원 API 1회 실패 후 성공 | 실패 시 snapshot 유지, 다음 시도에서 원본 rect/visible 복원, 성공 후 제거. ISSUE-003. |
| Integration | Real과 동등하게 모든 자손을 열거하는 3단계 트리; AdFit 깊이=1/2/3 | 그래프의 직계 edge 정확, max_depth=2에서 깊이 3 미검출, 중복 없음. ISSUE-001. |
| Integration | 기존 popup fixture, WM_CLOSE 성공 반환이나 창은 생존, 20 tick | 모든 후속 tick에서 숨김 유지. 시그널 제거 시에만 grace 후 복원. ISSUE-002. |
| Integration | mutation 순간 창의 PID/class 변경 또는 창 소멸 | 다른 창에는 close/hide/resize/restore 없음. 기존 identity 가드 유지. |
| Integration | tray NIM_ADD 두 번 실패, worker에 숨김 snapshot 존재 | 정해진 시간 내 복원·종료 또는 제어 UI 제공; mutex 해제. ISSUE-005. |
| End-to-End | 깨끗한 폴더에 문서대로 릴리스 설치 → 통제된 업데이트 제공 | helper를 찾고 검증하며 복원→종료→교체→재시작 완료. ISSUE-006/007. |
| Concurrency | 응답을 지연시킨 업데이트 확인 10회, 승인 2회 | active transaction/helper 하나, staging 충돌·backup 중복 변경 없음. ISSUE-008. |
| Concurrency | 빠른 OFF/ON, aggressive OFF, 동시에 종료 요청 | 마지막 요청 상태 일치, 정상 종료 후 재은닉 없음, 복원 실패 정보 보존. |
| Regression | report 경로가 디렉터리/쓰기 불가; tray 생성 실패 | exit nonzero 및 실제 실패 상태; trace가 성공을 허위 보고하지 않음. ISSUE-009. |
| Regression | settings 저장 실패 및 Run 설정 후 JSON 저장 실패 | 기존 토글/레지스트리 값 유지 또는 rollback 실패를 명시; UI가 성공으로 보이지 않음. |
| Integration | 연결은 성공하나 HTTP header/body 정지, 부분 다운로드, 해시 불일치 | deadline 내 실패·취소, 기존 EXE 유지, staging 정리. |
| Integration | 잘못된 서명/URL/태그/크기/만료 manifest, 정상이지만 오래된 버전 | 교체·helper 실행 없음. 만료 fixture는 고정 clock을 사용하여 날짜 의존 테스트 실패를 방지. |
| Integration | helper의 rename/copy/relaunch 실패, 새 프로세스가 즉시 exit | 이전 EXE·backup의 명확한 복구 상태와 오류 보고. spawn 성공만으로 건강 판정하지 않음. |
| Platform-specific | 테스트 소유 Win32 중첩 자식/owned popup, 한글·공백 경로, 100/150/200% DPI·다중 모니터 | 탐색 관계와 복원 좌표 정확, 경로 인자 분리 안전. 실제 사용자 카카오톡 대신 합성 테스트 창 사용. |
| Platform-specific | Explorer 재시작/로그온 지연/로그오프; 일반·상승 권한 조합 | 트레이 재등장 또는 명시적 실패/정상 정리, 무제어 백그라운드 방치 없음. |
| End-to-End | 지원 카카오톡에서 시작→배너 차단→잠금→뷰 변경→OFF→ON→종료 | 비광고 뷰 유지, 광고만 차단, 원래 숨김 snapshot 복원. 승인된 별도 테스트 세션에서 수행. |
| Long-running | 카카오톡 PID 유지하며 수천 개 후보 생성·소멸, 장기간 로그 | 상태/로그 크기 상한, stale identity 정리, CPU·메모리 지속 증가 여부 측정. |

## 9. Final Assessment

| 평가 항목 | 등급 | 근거 |
|---|---|---|
| Functional Correctness | **Needs Work** | 기본 골든은 통과하지만 실제 열거 계약과 연속 popup fallback에서 오류 확인. |
| Runtime Stability | **Needs Work** | typed rules로 시작 panic, tray 실패 시 제어 불능 대기, 업데이트 종료 경로 미통합. |
| Data Integrity | **Needs Work** | 대화 DB 조작은 없지만 정상 설정의 조용한 초기화 및 복원 snapshot 소실 확인. 실제 대화 유실 근거 없음. |
| Error Resilience | **Needs Work** | 서명·해시·일부 롤백·문법 복구는 있으나 복원 재시도·진단 실패 전파·트레이 실패 처리가 부족. |
| Cross-platform Robustness | **Acceptable** | Windows 전용 범위와 비Windows 거부가 명시되어 있음. Windows 환경별 E2E는 미검증이며 Win32 계약 문제는 별도 수정 필요. |
| Test Confidence | **Needs Work** | Rust 36/Python 골든 4/Clippy 통과에도 임시 실패 주입에서 문제 재현. Fake/Real 계약 및 연속 상태·실패 경로 커버리지 보강 필요. |

**실제로 먼저 수정할 문제 3개:**

1. **ISSUE-001:** Win32 직계 자식 그래프 구성과 깊이 제한을 바로잡는다.
2. **ISSUE-002:** 팝업 fallback의 숨김·복원 반복을 막는다.
3. **ISSUE-004:** rules 타입 오류 panic과 settings 전체 기본값 전환을 제거한다.
