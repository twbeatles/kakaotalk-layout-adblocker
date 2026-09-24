# AI Context: KakaoTalk Layout AdBlocker v11

## 개요

- 목적: 카카오톡 Windows 클라이언트의 광고 영역을 레이아웃 조정으로 제거
- 버전: `11.1.5`
- 특징: `hosts/DNS/AdFit` 제거, 트레이 중심 UX, Rust 네이티브 엔진(WinEvent + reconciliation)
- 실행 정책: Windows 전용(비Windows에서는 fail-fast 종료 코드 `2`)
- 기본 구현: Rust `rust/crates/kakao-app` (`kakao-adblock-rs` / `dist/KakaoTalkLayoutAdBlocker_v11.exe`)
- Python v11 참고 구현: `legacy/python-v11/` (골든 fixture/회귀 테스트용). 루트 `kakaotalk_layout_adblock_v11.py`는 사용중단 안내만 출력한다.

## 광고차단 알고리즘 고정 규칙

- v11 광고차단 알고리즘은 고정 계약으로 취급한다. 임의 리팩터링, 휴리스틱 완화/강화, 범용화 시도를 하지 않는다.
- 기본 전략은 항상 `layout-only`다. `hosts/DNS/네트워크 차단` 계열 기능을 다시 추가하지 않는다. 레지스트리는 선택적 시작프로그램(`HKCU Run`)에만 사용한다.
- 기본 알고리즘 의미는 `blurfx/KakaoTalkAdBlock` 계열과 일치시킨다:
  - 메인 윈도우 식별
  - legacy signature 기반 top-level candidate hide
  - subtree token 기반 aggressive hide
  - guarded popup dismiss
  - confirmed ad signal 기반 empty `EVA_ChildWindow` close
- empty `EVA_ChildWindow` close의 custom scroll guard는 메인 윈도우 전체가 아니라 해당 candidate child subtree 기준으로 유지한다.
- token 없는 하단 `Chrome_WidgetWin_*` geometry-only hide는 기본값에서 금지하고, rules opt-in(`hide_bottom_banner_without_token=true`)일 때만 허용한다.
- non-empty popup host title은 기본값에서 allowlist(`popup_host_text_contains`)에 맞지 않으면 dismiss하지 않는다.
- popup class 탐색은 direct child만 보지 않고 `popup_search_depth` 범위의 descendant까지 허용하되, 기본값은 `2`를 넘기지 않는다.
- 알고리즘 자체를 바꾸려면 반드시 실제 `--dump-tree`/`--dump-tree-series` 근거, fixture 또는 회귀 테스트, 관련 문서 갱신을 함께 남긴다.
- 가능하면 rules/fixture/test를 조정하고, 엔진 로직 변경은 실제 회귀가 확인된 경우로 제한한다.
- 실측 기준(2026-06-17, KakaoTalk `26.5.0.5163`): 메인 배너 광고는 **owner=메인창인 owned `WS_POPUP`**(`EVA_Window_Dblclk`, 빈 텍스트) 안에 `Chrome_WidgetWin_1`/`Chrome Legacy Window`(CEF)를 갖는다. 엔진은 `GetParent`가 owned 윈도우에 owner(=메인 핸들)를 반환하는 특성 덕에 "메인의 빈 텍스트 자식 후보" 분기로 등록 후 legacy signature로 hide한다(`parent==0` 분기 아님). 잘 동작하지만 Win32 동작에 기댄 부하지지 경로이므로 추측 변경 금지, `tests/fixtures/window_dumps/owned_popup_legacy_ad.json` 골든 회귀로 고정한다.
- `--dump-tree`의 `windows` 트리는 owned popup을 넣지 않고 `owned_popups` 배열에 따로 둔다. `windows`만 보면 광고 호스트가 빠져 보이므로, 구조 판단은 `owned_popups`·series `candidates[]` 또는 실제 엔진 실행으로 한다. 그래프 자식 에지는 Win32 직계 자식(`GetParent`) 기준이다.

## Rust 활성 런타임 동작 (아래 "핵심 모듈"의 Python 계약과 다른 지점)

> 아래 "핵심 모듈" 절은 명시된 대로 **Python 참고 구현(`legacy/python-v11`)의 알고리즘 계약**이다.
> Rust 기본 구현이 의도적으로 다르게 동작하는 지점은 다음과 같다. 혼동하지 말 것.

- 설정/규칙 파손 백업 파일명은 Rust에서 `*.broken-<unix-epoch>`다. Python 계약의 `*.broken-YYYYMMDD-HHMMSS`가 아니다.
- 트레이 상태는 v11.1.4부터 메뉴 헤더 + `NIF_TIP` 툴팁으로 노출한다(`kakao-win32/src/tray/status_text.rs`의 `status_menu_lines` / `status_tooltip`). 상태 값은 `SharedFlags`의 `main_windows`(확정 게이지), `hidden_windows`/`closed_windows`/`resized_windows`(누적), `restore_failures`(현재 실패 창 수 게이지), `last_error`에서 읽는다.
- `EngineStatePayload.closed_windows`는 **empty `EVA_ChildWindow` close 요청 수**다. popup dismiss는 `popup_close_requests`가 따로 센다. 실제 창 소멸 확인은 순수 평가 계층이 알 수 없으므로 `SharedFlags.closed_windows`(엔진 계층)가 담당한다.
- popup dismiss는 `WM_CLOSE` 결과로 분기하지 않고 hide/zero-size fallback을 항상 적용한다. 다만 소멸/거부/미전달 여부를 `DEBUG` 로그로 남긴다(`engine/apply.rs` `apply_evaluation`). 매 tick 반복되는 경로라 `WARN`이 아니다.
- 복원 실패는 지수 백오프로 재시도하고(`RESTORE_MAX_ATTEMPTS`, `RESTORE_GIVEUP_COOLDOWN_TICKS`) 창당 1회만 경고한다. `restore_failures`는 누적 시도 횟수가 아니라 현재 실패 중인 창 수다.
- 복원 성공 판정은 `IsWindowVisible`이 아니라 창 자신의 `WS_VISIBLE` 스타일(`Win32Api::has_visible_style`)이다. 부모가 숨겨진 자식 창의 정상 복원을 실패로 세던 문제 때문이다. 복원은 top-level 창부터 수행한다.
- 응답 없는(`IsHungAppWindow`) 카카오톡 창에는 apply와 복원 모두 동기 Win32 호출을 하지 않고 스냅샷을 보존한다. 종료 시 워커 join은 3초(`WORKER_STOP_TIMEOUT`)로 제한하며, 초과하면 경고 후 프로세스를 종료한다(Python `stop()` join timeout 2.0s 계약에 대응).
- `last_error`는 출처를 구분한다. 복원 실패로 설정된 오류는 게이지가 0으로 돌아오면 자동으로 지워진다(`SharedFlags::report_restore_failures`). 시작 시 설정 로드 경고 1건을 `복구 실패 > 자동 복구 > 기타` 우선순위로 노출한다(`observability::startup_warning_summary`).
- 워커 tick panic은 `catch_unwind`로 격리한다(`engine/worker.rs` `guarded_tick`). 스냅샷은 유지하고 `last_error`에 표시하며, 3회 연속이면 5초 쉰다. 루프 자체가 죽으면 drop guard가 `last_error`에 기록한다. 릴리스 프로파일의 `panic`은 이 때문에 `unwind`를 유지한다.
- 로그 회전은 시작 시 1회가 아니라 `config::RotatingLog`가 기록 중에도 수행한다.
- 설정/규칙 JSON의 UTF-8 BOM은 허용한다(`load_json_value`). 저장은 BOM 없는 UTF-8 + `sync_all` 후 rename이다.
- 트레이 토글은 디스크의 최신 설정을 다시 읽고 한 필드만 바꿔 저장한다(`config::update_settings`). 시작 시 `run_on_startup=false`인데 Run 값이 있으면 설정을 `true`로 맞춘다(`startup_repair::adopt_registry_startup_state`, Python 계약 "레지스트리 상태로 1회 동기화").
- WinEvent 훅은 카카오톡 PID로 범위를 한정하며 PID 집합이 바뀌면 재설치한다(`EventHook::install_for_pids`). 차단 OFF 동안에는 훅을 해제한다. 이벤트 병합 대기는 `thread::sleep`이 아니라 `EventHook::pump_for`다(펌프해야 훅 콜백이 실행된다).
- 워커 스케줄은 `engine/schedule.rs`의 순수 함수가 정한다. PID 스캔은 카카오톡 생존 시 30초마다 전체 재동기화한다(liveness는 `process::PidWatch`의 보관 핸들). 부재 시에는 `pid_scan_interval_ms`에서 시작해 5초 후 1s, 30초 후 2s로 백오프한다.
- `build_graph`는 top-level마다 `enum_descendant_windows` 1회 + 자손당 `GetParent` 1회로 트리를 만든다. 이전 per-node 알고리즘과의 동일성은 `kakao-app/tests/graph_build_parity.rs`가 고정한다(실데스크톱 비교는 `--ignored`).
- 워커 tick은 `evaluate_graph_for_apply`(진단 `candidates` 미생성)를 쓰고, `--dump-tree`/`--dump-tree-series`/`--shadow`만 `evaluate_graph_with_states`를 쓴다. 두 경로의 `actions`/`state`는 동일해야 하며 `kakao-core/tests/apply_path_parity.rs`가 이를 고정한다.
- `poll_interval_ms`는 활성(최근 2초 내 카카오톡 이벤트) 재확인 주기, `idle_poll_interval_ms`는 유휴 재확인 주기, `cache_cleanup_interval_ms`는 `states`/`stale` 캐시 정리 주기로 실제 사용된다. `idle_backoff_max_ms`(기본 1000)가 `idle_poll_interval_ms`보다 크면, 이벤트 없는 상태가 10초 지속된 뒤 10초마다 유휴 주기를 2배로 늘려 이 값까지 키운다. 이벤트가 오면 즉시 복귀하고, 훅이 없는 폴링 모드에서는 백오프하지 않는다. Python 계약의 "idle→active 약 200ms"는 이벤트 수신 경로 기준으로 유지되고, 훅 누락 시 복구 지연은 최대 `idle_backoff_max_ms`다. `start_minimized`는 트레이 전용 런타임에서 미사용이며 호환용으로만 파싱한다.
- 업데이트 헬퍼는 교체에 실패하면(부모 대기 초과 제외) staged 파일을 지우고 이전 EXE를 원래 인자로 다시 실행한다(`kakao_updater::should_relaunch_previous`).
- 성능 측정용 읽기 전용 프로브: `cargo test --release -p kakao-app --test perf_probe -- --ignored --nocapture`.
- `--self-check`는 APPDATA 쓰기, `HKCU Run` 읽기/쓰기 접근, Run 등록 명령 health, 프로세스 열거를 점검한다. 경고는 `core_warnings`(strict에서 실패)와 `info_warnings`(설정 자동 복구 등, strict에서도 통과)로 분리한다.

## 엔트리포인트

- 실행(기본): `dist/KakaoTalkLayoutAdBlocker_v11.exe` 또는 `cargo run -p kakao-app --release`
- 소스: `rust/crates/kakao-app` (`kakao-adblock-rs`)
- 시작프로그램(Rust): HKCU Run + `StartupApproved`. 트레이 시작 전 `Shell_TrayWnd` 대기, NIM_ADD 재시도, 실패해도 메시지 루프 유지 후 `TaskbarCreated`/타이머 재등록. `run_on_startup`이면 missing/stale/missing_target Run 명령을 현재 EXE로 복구하고, Windows가 꺼 둔 시작 앱 상태를 다시 켠다. cargo 소스 실행은 존재하는 패키지 EXE 등록을 덮어쓰지 않는다.
- 루트 `kakaotalk_layout_adblock_v11.py`는 Rust EXE 안내만 출력하고 종료 코드 `0`
- Python 참고 구현: `legacy/python-v11/kakao_adblocker`, 엔트리 `legacy/python-v11/kakaotalk_layout_adblock_v11.py`
- 정적 분석: `pyrightconfig.json` extraPaths=`legacy/python-v11`, include=`legacy/python-v11/kakao_adblocker`, `tests`
- 권장 검증: `.\scripts\dev_check.ps1` (Python 골든) + `cd rust; cargo test --workspace` (fmt/clippy 포함)
- 일반 UI: named mutex `Local\KakaoTalkLayoutAdBlocker_v11`. 중복 실행은 stderr 후 종료 코드 `0`
- `--self-check`, `--dump-tree`, `--dump-tree-series`, `--shadow`는 mutex 밖 진단 경로
- 트레이: 차단 On/Off, 공격 모드, 시작프로그램, 복원 실패 초기화, 로그/릴리스/업데이트, 종료(restore 후)

## 핵심 모듈

- 활성 런타임: `rust/crates/kakao-core`, `kakao-win32`, `kakao-app`, `kakao-updater`
- 엔진 캐시(`snapshots`/`states`/`stale`)와 정리 시계는 `kakao-app/src/engine/caches.rs`의 `EngineCaches`에 모여 있고 `engine/tick.rs`의 `tick(api, pids, settings, rules, caches, flags)`가 이를 받는다
- Rust 장문 파일은 단일 책임 하위 모듈로 분할되어 있다(순수 이동, 알고리즘·공개 경로 불변). 기존 `.rs` 파일은 `pub use` 퍼사드로 남는다:
  - `kakao-core/src/evaluate/` — `payloads`(진단 DTO) / `mutation_log` / `inspect`(읽기전용 검사) / `apply`(변이 계획) / `orchestrate`(`evaluate_graph*` 진입점)
  - `kakao-app/src/engine/` — `model`(스냅샷/상수) / `caches`(`EngineCaches`) / `flags`(`SharedFlags`) / `apply`(Win32 적용) / `restore`(복원·백오프) / `schedule`(PID 스캔·재확인 주기 순수 함수) / `tick`(단일 조정 단계) / `worker`(백그라운드 루프, tick panic 격리, `join_with_timeout`)
  - `kakao-win32/src/tray/` — `command` / `state` / `status_text`(Win32-free) / `shell_ready` / `host`(메시지 루프) / `menu`
  - `kakao-app/src/updater/` — `error` / `model` / `version` / `canonical` / `manifest`(서명 검증) / `http` / `staging`
  - `kakao-app/src/config/` — `paths` / `settings` / `storage`(self-heal I/O) / `log`(회전 라이터)
  - `kakao-app/src/lib.rs`는 컴포지션 루트(`run_with_args`)로 남고 `args` / `dialogs` / `observability` / `dump_cmd` / `startup_repair`를 추출했다. `kakao_app::{Args, should_attach_parent_console}` 공개 경로는 유지된다
  - 소스 grep 테스트(`tests/test_release_pipeline_v11.py`의 `read_rust_module`)는 퍼사드+분할 디렉터리 전체를 읽으므로 이후 분할에도 깨지지 않는다. 구조 이동 후에는 `cargo test`뿐 아니라 `pytest`까지 돌려야 한다(CI `validate` 잡이 Rust 소스를 직접 검사함)
- Python 참고 구현은 `legacy/python-v11/kakao_adblocker/` 아래에 있다. 아래 모듈 설명은 그 참고 구현의 알고리즘 계약이다.

- `kakao_adblocker/app/`
  - `main`, CLI parser, self-check, startup trace helper
  - package facade는 기존 `kakao_adblocker.app` import surface와 test monkeypatch 지점을 유지
  - 내부 구현은 `cli.py`, `self_check.py`, `startup.py`로 분리
- `kakao_adblocker/config/`
  - `LayoutSettingsV11`, `LayoutRulesV11`
  - `%APPDATA%\KakaoTalkAdBlockerLayout` 경로 관리
  - 성능 설정: `idle_poll_interval_ms`, `pid_scan_interval_ms`, `cache_cleanup_interval_ms`
  - 신규 성능 설정: `burst_scan_iterations`, `burst_scan_interval_ms`
  - 신규 필드 누락 시 기본값 자동 보완(무중단 호환)
  - 신규 rules 플래그: `hide_bottom_banner_without_token=false`, `close_empty_eva_child_requires_ad_signal=true`
  - 신규 rules 튜닝값: `weak_signal_confirm_ticks=2`, `hidden_restore_grace_ms=250`
  - 신규 rules 키: `popup_ad_classes=["AdFitWebView"]`, `popup_search_depth=2`, `popup_host_text_contains=[]`, `popup_host_require_empty_text=true`
  - rules 로드 시 `ad_candidate_classes`가 누락/비정상이면 `main_window_classes`로 폴백
  - JSON 파손(파싱 실패/최상위 타입 불일치) 시 `*.broken-YYYYMMDD-HHMMSS` 백업 생성 후 기본값 JSON으로 self-heal
  - rules 로드 시 `banner_min_height_px > banner_max_height_px` 역전값을 자동 교정(swap)하고 경고 기록
  - `*.broken-*` 백업 자동 정리(30일 초과 삭제 + 최신 10개 유지)를 로드 시마다 적용
  - settings/rules 저장은 원자적 교체(`os.replace`)로 파일 파손 리스크 완화
  - 첫 실행 runtime bootstrap(settings/rules/log)은 create-if-missing 방식으로 처리해 기존 파일 덮어쓰기를 방지
  - rules 문자열 무결성 self-check(mojibake 시그니처/`�`) 경고
  - 앱 계층 전달용 `consume_load_warnings()` 제공
- `kakao_adblocker/event_engine/`
  - `LayoutOnlyEngine`, `EngineState`
  - 내부 구현은 `controller.py`, `scanner.py`, `signals.py`, `actions.py`, `dump.py`, `models.py`로 분리
  - 단일 watch+apply 루프(적응형 폴링), `main_window_classes` 기반 메인 윈도우 식별
  - 차단 OFF 상태에서는 watch/apply를 모두 일시중단하고 1.0초 저비용 대기
  - 광고 후보는 `ad_candidate_classes`(기본: `EVA_Window_Dblclk`, `EVA_Window`)와 레거시 시그니처(exact + substring)를 함께 사용해 필터링
  - 비메인 top-level KakaoTalk window의 descendant(depth<=`popup_search_depth`)가 `popup_ad_classes`와 매치되더라도, 기본값에서는 empty host title 또는 allowlist(`popup_host_text_contains`) 매치일 때만 host와 matched popup descendant만 정리
  - popup dismiss는 `SendMessageTimeoutW` 기반 `WM_CLOSE` timeout(기본 500ms)으로 보호하고, 실제 close/hide/zero-size 성공 여부를 검증하며 실패 시 상태(`last_error`)와 로그에 반영
  - popup dismiss 후 창이 살아 있어 hide/zero-size fallback이 적용되면 `popup` hide snapshot으로 추적해 OFF/stop 또는 popup 신호 소멸 시 복원
  - empty `EVA_ChildWindow` close의 custom-scroll guard는 메인 윈도우 전체가 아니라 candidate child subtree 기준으로 tick마다 재평가
  - 메인 윈도우 상태는 `candidate_main_window_count`(후보)와 `main_window_count`(확정)로 분리되며, 실제 apply는 확정 기준만 사용
  - 엔진 시작 시 enabled인 경우에만 동기 warm-up(scan+apply 1회)으로 초기 광고 깜빡임 완화
  - 빈 문자열 텍스트 캐시는 짧은 TTL로 재조회해 초기 UI 구성 구간 탐지 지연 완화
  - 메인 윈도우는 `top-level + main class + child signature`를 강한 가드로 유지하고, title이 비어있지 않아도 `main_window_titles` 불일치 시 자식 시그니처(`OnlineMainView`/`LockModeView`) fallback 탐지를 허용
  - 차단 OFF/공격 모드 OFF/엔진 종료 시 scan/apply/window mutation single-flight lock으로 숨김·이동 창 원복과 재은닉 경합을 방지
  - `stop()` 시작 이후에는 새 hide/close/apply 작업이 봉쇄되어, join timeout 이후에도 복원 직후 재은닉이 발생하지 않도록 정리
  - aggressive hide 창은 공격 모드 OFF 시 즉시 원복 후 재스캔/재적용
  - 숨김 창은 aggressive/legacy 시그니처에서 벗어나면 stale 상태로 남지 않고 자동 원복
  - `stop()` join timeout(2.0s) 시 상태/로그 경고 후 종료 절차 계속
  - 원복 실패 항목 스냅샷 보존으로 재시도 가능
  - restore failure retry snapshot은 현재 프로세스 메모리 한정이며, 프로세스 재시작 이후 cross-process snapshot persistence는 구현하지 않는다
  - `EngineState.restore_failures`, `EngineState.last_restore_error` 상태 노출
  - `reset_restore_failures()`로 복원 실패 상태 수동 초기화 지원
  - `WindowIdentity(hwnd,pid,class)` 기반 text/hidden-window 캐시로 HWND 재사용 오동작 방지
  - 숨김/후보 aggressive subtree는 stale non-empty 텍스트 캐시를 우회해 광고 토큰 소멸 후 복원 지연을 줄임
  - empty `EVA_ChildWindow` close는 `SendMessageTimeoutW` 성공만으로 집계하지 않고 대상 창 소멸을 확인한 경우에만 `closed_windows`를 증가시킴
  - 스캔 경로는 경량 수집(`rect/visible` 미조회)으로 호출 부담 감소, `--dump-tree`만 상세 수집 사용
  - `--dump-tree-series`는 frame별 candidate decision preview를 함께 저장하며 popup dismiss host와 matched descendant를 모두 기록
  - Win32 text-result 상태(`known/truncated/error`)를 반영하며, popup host text unknown은 empty-title allow가 아니라 guard blocked로 처리
  - PID 스캔/캐시 정리 주기 스로틀 적용
  - PID 스캔 경고(psutil 실패, tasklist fallback/실패)를 상태(`last_error`)와 로그에 반영
  - 기본 설정 기준 idle->active 복귀 목표 지연 약 200ms
  - `report_warning()`로 시작 시점 경고를 상태(`last_error`)에 반영하며, 엔진 시작 이후에도 우선순위 경고 1건 유지
- `kakao_adblocker/layout_engine.py`
  - `OnlineMainView` / `LockModeView` 리사이즈 규칙
  - 공격적 배너 휴리스틱은 token 판정과 geometry 판정을 분리하고, 짧은 ad 토큰은 단어 경계 기준으로 매칭
  - 기본값에서는 token 없는 하단 `Chrome_WidgetWin_*` 패널을 geometry만으로 숨기지 않으며, subtree token도 aggressive signal로 사용
  - main child resize는 `SWP_NOMOVE | SWP_NOZORDER | SWP_NOACTIVATE`로 적용해 z-order/activation 부작용을 피함
- `kakao_adblocker/protocols.py`
  - Win32 API/Joinable Thread/UI Root/Engine 상태에 대한 구조적 타입 프로토콜 정의
  - 테스트 더블(`tests/*`)과 런타임 모듈의 타입 경계를 분리
- `kakao_adblocker/ui.py`
  - `TrayController`
  - 트레이 메뉴: 상태/OnOff/공격 모드/시작프로그램/복원실패초기화/창 열기/로그/릴리스/업데이트 확인/종료
  - 최소화 시작 시(`--minimized`/`start_minimized`) 시작 안내 팝업 생략
  - 트레이 비가용 시 최소화 시작 요청을 무시하고 창을 강제 표시
  - 트레이 시작은 준비 신호 기반으로 판정하며 시작 타임아웃(1.5초) 시 비활성화
  - 트레이 런타임 비정상 종료 시 트레이를 비활성화하고 메인 창 복구
  - 트레이 시작 실패/런타임 중단 후 3초 간격 최대 3회 재시도, startup fallback 중 성공 시 자동 재은닉
  - 트레이 비가용 시 창 닫기(X)는 숨김이 아니라 종료로 처리
  - 시작 시 `run_on_startup` 값을 레지스트리 상태로 1회 동기화
  - 시작 시 Run 등록이 enabled면 등록 명령 stale/missing 여부를 함께 검사하고 자동 복구를 시도
  - 소스 모드에서는 기존 Run 등록이 존재하는 `KakaoTalkLayoutAdBlocker_v11.exe --startup-launch --minimized` 패키지 EXE를 가리키면 healthy로 인정해 설치된 EXE 등록을 소스 스크립트 명령으로 자동 덮어쓰지 않음
  - custom/unknown Run command는 자동 sync로 덮어쓰지 않고 `custom command left unchanged` 상태로 보존한다. 직접 UI 토글은 현재 실행 모드의 표준 command를 사용한다.
  - 상태 문자열에 마지막 오류/갱신시각 표시
  - 엔진 오류가 없을 때는 tray unavailable/startup rollback 같은 UI 계층 경고를 상태 문자열에 짧게 노출
  - 상태 문자열의 `메인윈도우`는 확정 count이며, 후보가 더 많을 때만 `후보 N`을 추가 표기
  - 상태 문자열의 `누적 숨김`/`누적 닫힘`/`누적 리사이즈` 라벨로 누적 카운터 의미를 명시
  - pystray/Pillow 지연 로딩 + 실패 TTL(30초) 자동 재시도
  - 트레이 콜백은 queue 디스패치(`_safe_after` -> main-thread drain)로 처리
  - `_safe_after()`는 tray/worker thread에서 Tk/root 메서드를 호출하지 않고 queue put만 수행한다. `winfo_exists()`와 callback 실행 여부는 Tk main-thread drain에서만 판단한다.
  - 설정 저장 실패 시 토글 값 롤백(`enabled`/`run_on_startup`/`aggressive_mode`)
  - startup 토글에서 저장 실패 시 레지스트리 역롤백
  - aggressive mode 토글은 저장 성공 후 엔진에 즉시 반영
  - `_tick_status` 스케줄링(`root.after`)도 종료 경합 예외 비전파
  - `로그 폴더 열기` / `GitHub 릴리스 열기` 실패도 상태 문자열 경고로 노출
- `kakao_adblocker/services.py`
  - `ProcessInspector`, `StartupManager`, `ReleaseService`
  - `ProcessInspector.get_process_ids()`는 psutil 경로에서 per-process 예외 격리 처리
  - psutil 초기화/루프 실패 시 `tasklist` 폴백
  - `ProcessInspector.consume_last_warning()`로 PID 탐지 경고를 엔진 계층에서 소비 가능
  - `StartupManager.probe_access()`는 Run 레지스트리 읽기/쓰기 접근을 함께 점검
  - Run command parsing은 Windows `CommandLineToArgvW`와 환경변수 확장을 우선 사용하고 실패 시 기존 fallback 파서를 사용한다
  - `StartupManager.registration_health()`는 exact expected command, source-mode compatible packaged EXE를 healthy로 보고 custom command는 자동 복구 대상에서 제외한다
  - dump/report/startup trace 파일 쓰기 실패는 traceback 대신 stderr와 종료 코드 `1`로 처리한다. `--dump-series-duration-ms` 상한은 `10000`, interval 하한은 `10`이다.
  - hidden `--bootstrap-argv-report`는 인자 누락 시 종료 코드 `2`, 쓰기 실패 시 종료 코드 `1`을 반환한다.
  - 진단용 `ProcessInspector.probe_tasklist()`, `StartupManager.probe_access()` 제공

## 빌드 메모

- `kakaotalk_adblock.spec`는 런타임 핵심 모듈(`kakao_adblocker.app`, `kakao_adblocker.config`, `kakao_adblocker.event_engine`, `kakao_adblocker.layout_engine`, `kakao_adblocker.logging_setup`, `kakao_adblocker.services`, `kakao_adblocker.ui`, `kakao_adblocker.win32_api`, `pystray`, `PIL`, `tkinter`)을 `hiddenimports`로 명시하고 `collect_submodules("pystray"|"PIL")` 및 packageized `app/config/event_engine` 하위 모듈 수집을 함께 사용해 onefile 누락을 방지
- 타입 경계 모듈 `kakao_adblocker.protocols`도 `hiddenimports`에 포함되어 onefile 모듈 누락 가능성을 줄임
- 패키지 루트 `kakao_adblocker`도 `hiddenimports`에 포함되어 lazy export 패키지 접근 경로를 고정
- `pywinauto`, `comtypes`는 active v11 런타임 바깥의 legacy/UIA 의존성이므로 `.spec`의 `excludes`로 유지
- single-instance mutex, 동적 Win32 text-result, Run command parsing은 stdlib `ctypes` 기반 kernel32/user32/shell32 호출이므로 `.spec` hidden import 추가 대상이 아님
- popup parity(`popup_ad_classes` / `AdFitWebView`), `SendMessageTimeoutW` close timeout, popup fallback 복원 추적은 기존 `config/event_engine/win32_api` 내부 구현이라 추가 PyInstaller hook 없이 현재 spec으로 포장 가능
- empty `EVA_ChildWindow` subtree custom-scroll guard는 tick-local `event_engine` 내부 구현이라 추가 hidden import 없이 현재 spec으로 유지
- `scripts/build_release.ps1`는 빌드 시작 시 `VERSION`과 `packaging/windows_version_info.txt` 동기화를 검증하고, 기본값으로 built EXE에 `--self-check --strict-self-check --json` packaged smoke를 1회 수행하며, core failure만 빌드 실패로 취급한다. 필요 시 `-SkipSmokeCheck`로 비활성화 가능
- interactive shell이 감지되면 built EXE에 `--startup-launch --minimized --startup-trace ... --exit-after-startup-ms ...` startup smoke를 추가 수행하고, 60초 timeout으로 멈춤을 차단하며, 비interactive 환경에서는 skip 기록만 남기고 계속 진행한다
- `-StrictStartupSmoke`는 interactive startup smoke가 실제 수행된 경우에만 tray unavailable / tray start warning을 빌드 실패로 승격한다
- GitHub Actions workflow `.github/workflows/windows-ci.yml`는 hosted Windows에서 pyright, pytest, self-check JSON, no-sign packaging build와 packaged strict self-check를 검증하고, interactive tray/startup smoke는 강제하지 않는다

## 동작 규칙

1. `kakaotalk.exe` PID 집합을 수집
2. 메인 윈도우(`EVA_Window_Dblclk`/`EVA_Window`) 식별
3. `OnlineMainView`: `width=parent-2`, `height=parent-31`
4. `LockModeView`: `width=parent-2`, `height=parent`
5. `Chrome Legacy Window` 하위 광고 서브윈도우 숨김
6. 공격 모드에서 `Chrome_WidgetWin_* + ad token` 또는 subtree token이 확인된 하단 배너 후보만 숨기고, geometry-only hide는 rules opt-in일 때만 허용
7. 비메인 top-level 창의 descendant(depth<=`popup_search_depth`)가 `AdFitWebView` 등 `popup_ad_classes`에 매치되더라도 기본값에서는 empty host title 또는 allowlist match일 때만 host와 matched popup descendant만 close/hide/zero-size 처리
8. 시작프로그램 토글은 레지스트리 갱신 성공 시에만 설정 파일에 반영하며, 소스 모드 자동 동기화는 유효한 패키지 EXE Run 등록을 덮어쓰지 않는다
9. `--dump-tree`는 UI 모듈을 로딩하지 않는 경량 경로로 동작
10. `--self-check`는 UI/엔진을 기동하지 않고 APPDATA/logging bootstrap/tasklist/레지스트리/Run 등록 명령/`tkinter/Tk`/트레이 import 환경 진단만 수행하며, 기본 모드의 트레이 import 실패는 optional이고 `--strict-self-check`에서는 core로 취급
11. 일반 UI 실행은 named mutex로 단일 인스턴스만 허용하고, self-check/dump 계열 진단 명령은 mutex 밖에서 실행
12. Win32 text read failure/unknown popup host는 empty title로 간주하지 않고 popup dismiss guard blocked로 처리
13. 시작 경고 상태 반영은 `복구 실패 > 자동 복구 > 기타` 우선순위로 1건 노출

## 설정 파일

- `%APPDATA%\KakaoTalkAdBlockerLayout\layout_settings_v11.json`
- `%APPDATA%\KakaoTalkAdBlockerLayout\layout_rules_v11.json`
- `%APPDATA%\KakaoTalkAdBlockerLayout\layout_adblock.log`

## 레거시 보관

- Python v11 참고 구현: `legacy/python-v11/`
- 원본 모놀리식: `legacy/kakao_adblocker/legacy.py`
- deprecated 엔트리포인트: `legacy/카카오톡 광고제거 v10.0.py`
- 활성 런타임 판단은 `rust/`, `tests/fixtures/` 범위로 좁힌다. Python 알고리즘 회귀는 `legacy/python-v11` + `tests/`
- CodeGraph는 `legacy/` symbol을 노출할 수 있으므로 기본 구현은 Rust로 해석한다

<!-- SPECKIT-AGENT-GUIDE:START -->

## Spec Kit / Spec-Driven Development (AI 에이전트 필독)

> 이 블록은 GitHub Spec Kit 활성화 및 기능 명세 작업 결과를 AI 에이전트가 바로 쓰도록 정리한 안내입니다.
> 수정 시 마커 주석을 유지하세요. 스크립트/후속 세션이 이 구간을 갱신합니다.

### 이 저장소 상태

- **프로젝트**: `kakaotalk-layout-adblocker`
- **Spec Kit 초기화**: `.specify/ 있음`
- **에이전트 스킬**: Grok=True, Claude=True, Codex/Agy(.agents)=True
- **활성 기능**: 기본 구현은 Rust `kakao-app`. Python v11은 `legacy/python-v11`. 계약 문서는 `kakaotalk_rust_migration_plan.md`와 `docs/superpowers/plans/2026-09-02-rust-native-remaining.md`

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
