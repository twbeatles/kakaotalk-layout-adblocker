# KakaoTalk Layout AdBlocker — 광고 차단 기능 검증·테스트 고도화 에이전트 구현 지시서

> 작성: 2026-10-12  
> 대상 저장소: **`twbeatles/kakaotalk-layout-adblocker`** (`kakaotalk-pc-adblock-py`의 변경 후 이름)  
> 검토 기준: `main` 최신 확인 커밋 **`5a2c25d`** (2026-10-04, v11.1.6 버전 상향). **실제 구현 전 반드시 HEAD 재확인.**  
> 작업 성격: **실행 가능한 코드·테스트·CI 고도화** / 기존 광고 차단 행위 보존 우선 / 증거 중심  
> 예상 저장 위치: `docs/TEST_HARDENING_AGENT_PLAN.md` (이 파일 자체는 지시서이며 저장소에 아직 반영되지 않았음)

---

## 0. 에이전트에게 내리는 최상위 지시

**이 문서를 받으면 구현까지 진행하라. 계획만 재작성하거나 제안서로 끝내지 말 것.** 단, 테스트와 독립 판정 기준을 먼저 구축하고, 실제로 재현된 결함이 없는 한 광고 판별 규칙·타이밍·복원 로직을 임의로 변경하지 말 것. 차단 강화를 이유로 정상 카카오톡 기능을 희생해서는 안 된다.

1. 현재 체크아웃의 `git status`, `git rev-parse HEAD`, 최근 커밋, README, `CLAUDE.md`, `PROJECT_AUDIT.md`, Rust 워크스페이스, 테스트, CI 및 기존 fixture를 읽는다. 저장소가 갱신되었다면 이 계획과 현재 코드의 차이를 기록하고 **현재 코드에 맞추어 구현**한다.
2. **기준선 확보**: 변경 전 `cargo fmt`, `cargo clippy`, `cargo test --workspace`, 릴리스 빌드(가능한 환경)를 직접 실행하고 결과·실패·ignored·환경을 보존한다. 실행하지 못했으면 `NOT RUN`으로 기록한다. 기존 문서의 “PASS”를 이번 실행의 PASS로 전용하지 말 것.
3. 테스트 설계의 최상위 품질 규칙: **정상 창을 숨기거나 닫는 오탐을 0으로 하는 명시적 안전 계약**, 알려진 광고 사례의 차단, OFF/종료 시 복원, 카카오톡 업데이트 시 미검증 상태 탐지. 테스트 기대값은 구현 함수의 산출물을 복사하지 않고 별도 근거에서 작성한다.
4. 단위 테스트만 추가하지 말고 **FakeWin32 적용 테스트, 상태 전이, 실패 주입, mutation, Windows 통합, 실사용 실기 확인 절차와 증거 보고**까지 구축한다. 실기·상용 카카오톡 설치가 없는 환경에서는 자동화가 가능한 부분까지만 PASS로 표시한다.
5. 실패를 발견하면 먼저 **최소 재현 테스트를 RED로 확정**, 원인을 추적한 뒤 위험을 최소화하는 코드 수정, 재검증, mutation 또는 반례 추가를 실시한다. 근거 없는 ‘안전해 보이는’ 대규모 리팩터링 금지.
6. 실행 환경이 허용하는 범위에서 바로 테스트와 코드 수정까지 진행하되, 사용자 PC의 실제 카카오톡 프로세스·설정·레지스트리를 무단 변경하지 말 것. **실사용 광고를 실제로 숨기는 테스트는 사전 합의된 테스트 환경에서만 실행**한다.
7. 작업 완료 시 PR에 들어갈 수 있는 변경물, 명령어별 실제 로그, 테스트 성공·실패, mutation 생존자, 환경 제약, 잔여 위험, 카카오톡 버전별 판정 결과를 한눈에 보이는 보고서로 제출한다.

### 절대 금지 사항

- 프로젝트와 무관한 드라이브/디렉터리 삭제, 광범위한 `rm -rf`, `rmdir /s`, `git clean -fdx`, 강제 리셋·강제 푸시 금지. 작업 디렉터리·빌드 산출물만 최소 범위로 다룬다.
- Git에 없는 로컬 `legacy/` 및 Python 파일을 ‘불필요’하다고 삭제하지 말 것. 최근 커밋은 **Git 추적에서만 제외**한 것이다.
- 광고 차단 기능 향상을 명목으로 hosts/DNS 변경, DLL 주입, 프로세스 메모리 패치, 자격 증명 수집, 타 프로그램 창 제어 도입 금지.
- 라이브 광고 없이 통과하는 mock/fixture 테스트를 ‘실제 광고 100% 차단’의 근거로 주장 금지.
- 테스트 실패 시 baseline이나 golden expected를 기계적으로 새 실제값에 덮어써서 통과시키지 말 것.
- 개인정보가 담긴 `--dump-tree` 원본, 실제 채팅 내용·대화 상대·연락처, 레지스트리 개인정보, 서명 비밀키를 커밋·CI artifact·이슈에 업로드 금지.

---

## 1. 저장소의 현재 구조와 증거 수준

**확인된 구현**

- Rust workspace: `rust/crates/kakao-core`, `kakao-win32`, `kakao-app`, `kakao-updater`.
- 판별: `kakao-core/src/{signals.rs,layout.rs,rules.rs,evaluate/{inspect,apply,orchestrate}.rs}`.
- 적용·원복: `kakao-app/src/engine/{tick,apply,restore,worker,schedule,caches,flags}.rs`.
- Windows 인터페이스 및 mock: `kakao-win32/src/{api,real,fake,event_hook}.rs`.
- 기존 테스트: `kakao-core/tests/{golden_parity,evaluate_weak_parity,apply_path_parity}.rs`; `kakao-app/tests/{restore_regression,scan_parity,graph_build_parity,engine_idle_cpu_test,perf_probe,config_migration,...}.rs`; `kakao-win32/tests/smoke.rs` 등.
- fixture: `tests/fixtures/window_dumps/*.json`와 `tests/fixtures/golden/*.json`, 현재 각 10개 쌍 확인.
- CI: `.github/workflows/windows-ci.yml`은 Windows에서 fmt/clippy/audit/machete/cargo test/릴리스 빌드/일반 `--self-check --json`를 실행. 실제 카카오톡 프로그램 설치를 통한 광고 차단 E2E는 수행하지 않음.
- 수동 검증 문서: `docs/COMPATIBILITY.md`, `docs/SOAK_TEST.md`; 진단 옵션 `--shadow`, `--dump-tree`, `--dump-tree-series`, `--self-check --strict-self-check --json` 존재.

**중요한 기초 진단 — 확인한 사실 / 아직 미검증인 사실 구분**

| 항목 | 확인 사실 | 다음 검증의 핵심 |
|---|---|---|
| 과거 Golden | `golden_parity.rs`는 Python 시절 Golden과 Rust 출력이 동일한지 검증 | 출력 동등성 ≠ 광고 판정이 옳다는 독립 근거. 독립 라벨 oracle 구축 |
| `FakeWin32`의 `WM_CLOSE` | `send_message_timeout`이 `WM_CLOSE` 요청에 성공을 반환해도 fake 창을 제거하지 않는 경로로 고정되어 있음 | 팝업 실제 제거, 전송 실패, 거부, timeout, 재등장 등 다중 결과 주입 |
| 기존 오탐 방어 | `popup_non_adfit_viewer`, `guarded_popup_adfit_viewer`, `bottom_web_panel_no_token` 등 음성 사례가 있음 | 음성 사례와 경계·변형 사례가 부족한지 조사. 실사용 정상 UI 시나리오 추가 |
| `restore_regression.rs` | 숨김 복원, stale 2-tick 복원, HWND/PID/class 가드, 응답 없음, 종료 healthy-first 등 테스트가 있음 | concurrent OFF/EXIT, timeout, 업데이트 경유 종료, 후속 복원 결과·관측 증거 강화 |
| 엔진 워커 | tick panic 격리 및 일정 실패 후 backoff, worker exit 감지가 구현됨 | 상태 전이 실패 주입 및 감시 메시지의 실제 유효성 검증 |
| CI | `cargo test --workspace` 및 빌드 수행 | 실제 카카오톡 실행·광고 차단·화면 정상성에 대한 자동 증거 없음 |
| 기존 문서 PASS | `COMPATIBILITY.md`에 PASS 표기 다수, `SOAK_TEST.md`에 1시간 수동 지침 | 실행 시각·카카오톡 버전·실측 로그가 없는 PASS는 이번 릴리스 실기 확인으로 간주하지 말 것 |

**과거 감사 기록은 베이스라인 참고자료일 뿐.** `PROJECT_AUDIT.md`는 2026-10-04에 `cargo test --workspace` **114 passed / 0 failed**를 보고하지만, 이 지시서를 작성하면서 해당 테스트 명령을 직접 실행한 것은 아니다. HEAD가 바뀌면 개수와 상태를 다시 확인한다.

### 광고 차단과 관계된 현재 계약(기존 코드 분석)

- 메인 윈도우 식별: `EVA_Window_Dblclk` / `EVA_Window`, `카카오톡`/`KakaoTalk`, `OnlineMainView`/`LockModeView`의 조합.
- 레거시 광고: `Chrome Legacy Window` 정확/부분 시그니처. 부분 일치는 weak signal로 기본 2틱 확인.
- 하단 배너: `Chrome_WidgetWin_*` + 지정 기하 조건 + 광고 토큰 조합. `hide_bottom_banner_without_token` 기본 false.
- 팝업: `AdFitWebView` 자손 + host 창 텍스트 가드. 비어 있지 않은 정상 뷰어 제목은 기본적으로 차단 금지.
- `aggressive_mode` 기본 ON. `Ad`, `AdFit`, `Advertisement`, `광고` 텍스트와 기하 조건이 관여한다. 단문 `Ad`는 word boundary 처리가 있음.
- 행동: `SW_HIDE`, `SetWindowPos`, `WM_CLOSE`(500ms 전달 제한), 거부 시 숨김·zero-size 폴백. `--shadow`는 적용 없는 판별 확인에 활용 가능.
- 복원: 스냅샷 보존, `hwnd+pid+class` identity 검증, stale 2틱 복원, 실패 backoff, 종료 시 healthy-first. 단 **동일 PID·class 내 재사용/고장 상황도 추가 확인 대상**.
- 메인 뷰 리사이즈는 기존 설계상 숨김 스냅샷에 저장하지 않으며, 카카오톡 본래 레이아웃에 재위임한다. **모든 크기를 무조건 저장·복원하는 방식으로 바꾸지 말 것**. 과거 검은 화면 회귀 원인을 유념하라.

---

## 2. 목표 및 품질 우선순위

1. **안전성 최우선**: 정상 채팅/이미지/프로필/보이스톡/미디어/설정/로그인/잠금/웹뷰가 의도치 않게 숨겨지거나 닫히지 않는다.
2. **광고 제거 성공**: 수집·라벨링된 실제 광고 사례의 목표 요소가 사라지며, 정상 레이아웃은 유지된다. 새로운 광고 형식을 무조건 가정해 차단하지 않는다.
3. **복원 가능성**: 토글 OFF, aggressive OFF, 정상 종료, 카카오톡 종료·재실행, 장애 회복에서 적용한 가역적 변경은 정확히 처리된다. 실제 `WM_CLOSE`로 종료된 창 자체는 일반적으로 복원 불가하므로 별도 정책·상태로 구분한다.
4. **업데이트 대비**: 카카오톡 UI 클래스/트리/레이아웃 변화가 탐지되면 기존 규칙을 근거 없이 공격적으로 적용하지 않고, 진단 정보를 남기고 안전한 상태로 실패한다.
5. **테스트의 결함 탐지력**: 독립 기대값, negative assertions, fault injection, mutation으로 테스트가 핵심 결함을 잡는지 증명한다.
6. **재현 가능성**: 샘플 입력, 실제 실행 명령, 테스트 환경, 로그, 결과, 잔여 위험이 PR과 CI에서 추적된다.

### 위험을 보는 방법

- **FP(False Positive)** = 정상 UI에 대한 금지된 `hide/close/zero-size`. 심각도 P0.
- **FN(False Negative)** = 라벨된 광고가 그대로 노출됨. 심각도 P1(반복·광범위한 회귀는 릴리스 차단).
- **Restoration defect** = 복원이 필요한 창이 복원되지 않음, 잘못된 타 프로그램 HWND 조작, 종료 후 무한 잔류. 심각도 P0/P1.
- **Unknown UI / Unverified version** = 새 구조를 판별할 충분한 증거가 없음. *광고가 없어 보인다*는 이유로 PASS 처리 금지.
- P0는 증거가 있는 1건만으로 해당 변경/릴리스를 차단한다. 수치상 커버리지와 전체 PASS 건수로 상쇄 불가.

---

## 3. Phase 0 — 테스트 기준선 동결 및 리스크 맵 (먼저 수행)

### 구현 절차

1. `git status --short`, `git rev-parse HEAD`, `rust/Cargo.toml`, 4개 crate의 기능 구성을 보고 `docs/testing/BASELINE.md` 작성.
2. Windows 개발기에서 가능하면 아래 명령을 실행하고 로그 저장:

   ```powershell
   Set-Location rust
   cargo fmt --all -- --check
   cargo clippy --all-targets --all-features -- -D warnings
   cargo test --workspace
   cargo build --release -p kakao-app -p kakao-updater
   Set-Location ..
   # 바이너리가 있는 경우와 환경이 안전한 경우에만:
   .\dist\KakaoTalkLayoutAdBlocker_v11.exe --self-check --strict-self-check --json
   ```

3. 위 명령의 성공/실패·ignored 목록·실행 머신/OS·Rust 버전·테스트 수·실행 시간을 기록한다. 빌드 전 `dist` 산출물이 없다면 경로를 확인하고 스크립트로 정식 생성한 뒤 실행한다.
4. 표 작성: `requirement -> 위험 -> 판별 코드 -> 적용 코드 -> 기존 테스트 -> 새 테스트 -> 실행 환경 -> 릴리스 Gate`.
5. **기존 테스트 이름과 실제 assertion을 매핑**한다. 파일명·함수명 존재만으로 커버되었다고 표시하지 말 것. 수동 체크리스트와 자동화의 중복도 구분.

**완료 조건**: 모든 P0 동작에 기존 테스트 또는 `GAP` 표시가 존재하고, 재현 가능한 baseline 로그가 남는다.

---

## 4. Phase 1 (P0) — 독립 Oracle + Positive/Negative 계약 테스트

### 4.1 Golden parity와 독립 Contract fixture 분리

기존 `tests/fixtures/golden`의 Python-Rust parity는 **회귀 참고용으로 유지**한다. 별도 `tests/fixtures/contract/`를 추가해 테스트가 **광고·비광고 정답 자체**를 주장하도록 한다. 기계가 방금 생성한 `Evaluation`을 그대로 expected로 저장하지 말 것.

권장 파일:

```text
tests/fixtures/contract/
  README.md
  manifest.json
  positives/      # 사람의 라벨에 근거한 광고 사례, 익명화된 창 트리
  negatives/      # 정상 UI 등 반드시 건드리면 안 되는 사례
  boundaries/     # 지오메트리, 토큰, title_known, depth 경계
  transitions/    # 여러 tick의 창 변화와 결과 기대값
rust/crates/kakao-core/tests/contract_oracle.rs
rust/crates/kakao-core/tests/rule_boundaries.rs
```

Contract 메타데이터 권장 스키마(기존 창 덤프 자체는 기존 형식 재사용):

```json
{
  "id": "negative_photo_viewer_adfit_child",
  "kind": "negative",
  "source": "sanitized-human-reviewed-dump",
  "capture_kakaotalk_version": "record-at-capture-time",
  "captured_os": "record-at-capture-time",
  "fixture": "negatives/photo_viewer.json",
  "settings": {"enabled": true, "aggressive_mode": true},
  "rules_overrides": {},
  "expect": {
    "must_not_hide": [200, 201],
    "must_not_close": [200, 201],
    "must_not_zero_size": [200, 201],
    "required_hide": [],
    "required_close": []
  },
  "label_basis": "title+manual visual review",
  "reviewed_by": "human-or-independent-reviewer",
  "reviewed_at": "YYYY-MM-DD"
}
```

> 위 JSON은 **스키마 예시**이며 저장소에 존재하는 실제 fixture가 아님. 테스트 로더는 기존 dump 형식과 contract 파일을 명확히 연결하고, 알 수 없는/누락된 필드를 조용히 허용하지 않도록 설계하라. 실제 `HWND`가 어떤 것인지 fixture와 맞춰 넣어야 한다.

### 4.2 필수 광고·정상 UI 매트릭스

**Positive(정상 차단되어야 함):**

- `Chrome Legacy Window` 정확 일치 광고, 부분 일치 weak signal(1틱 미적용/2틱 적용).
- 하단 `Chrome_WidgetWin_*` + 유효 기하조건 + 명시적 광고 토큰.
- 친구·채팅 탭의 광고 배너, 잠금 뷰 전후 광고, 앱 시작 시 짧게 등장하는 배너.
- 독립 `AdFitWebView` 팝업 + 허용되는 host 구조. owner/structural parent/자손 depth 1·2 구분.
- WM_CLOSE 성공 제거, WM_CLOSE 전달 실패, 처리되지만 창 존속, 응답 없음의 각각의 기대 결과.
- 광고가 잠시 사라졌다 재등장하는 경우 재탐지·중복 처리가 정상인 시퀀스.

**Negative(절대 수정되면 안 됨):**

- 사진·동영상 뷰어, 프로필, 채팅방 팝업, 설정, 보이스톡 UI 등 제목이 있는 정상 창. 특히 `AdFitWebView` 유사 자손이 있어도 host guard 거부 시 닫지 않음.
- 하단에 위치한 정상 웹 패널이지만 토큰이 없는 창(기본 `hide_bottom_banner_without_token=false`).
- 정상 창의 텍스트에 `Adobe`, `Address`, `add`, URL·상품명·대화 메시지 등이 섞인 사례. `Ad` 단독 토큰과 부분 매칭을 구별.
- KakaoTalk과 다른 PID, 같은 HWND 번호를 재사용한 타 프로세스, 유사한 클래스명을 가진 타 앱.
- `WindowText::Unknown`, `Truncated`, 빈 제목, UTF-8/한글 문자열, 과도하게 긴 제목.
- 상위 창이 숨겨진 자손, 제목이 늦게 채워지는 정상 창, 중간에 클래스가 바뀌는 재사용 HWND.
- 범위를 벗어나는 배너 높이·너비 비율·하단 margin, 잘못된/음수/0 크기, 부모가 없는 자식.
- 서로 다른 카카오톡 버전에서 확인한 메인 UI 구조 및 정상적인 독립 창.

### 4.3 경계값과 불변식

- `banner_min_height_px=40`, `banner_max_height_px=260`, `banner_min_width_ratio=.75`, `banner_bottom_margin_px=40`의 **바로 아래/정확히 경계/바로 위** 사례. 실제 규칙값을 fixture 또는 테스트 설정에서 가져와 경계를 매핑한다.
- `weak_signal_confirm_ticks` = 1, 2, 3 및 동일 후보 `match/miss` 교차. 확인 전에는 hide 금지.
- `popup_search_depth` = 0/1/2/3, owner/structural parent 차이, 숨긴 host를 다시 평가하는 경우.
- `enabled=false` 또는 `apply=false`, `stopping=true`의 **호출된 Win32 변경 API 0건**.
- `--shadow`는 실제 `ShowWindow`, `SetWindowPos`, `WM_CLOSE` 호출 0건이면서 동일 입력을 근거로 실제 모드와 **판별 결과**가 일치해야 한다(명시적으로 허용된 진단 차이만 예외).
- idempotence는 **관측 가능한 최종 상태** 기준(연속 틱의 내부 조정 시도 수 자체가 0일 필요는 없음); 반복 적용에도 위치·크기·가시성·재시도 상태가 의도치 않게 누적 변경되지 않는다.

**완료 조건**: 핵심 판정 경로에는 Positive와 Negative 쌍이 있고, independent expected는 검토자 승인 없이 생성/변경되지 않는다. 기존 golden은 테스트 통과를 위해 수정하지 않는다.

---

## 5. Phase 2 (P0) — FakeWin32를 행동 검증 도구로 강화

### 5.1 API Fake의 한계 수정(실제 API에 영향 없는 테스트 지원)

현재 `kakao-win32/src/fake.rs`는 `WM_CLOSE`를 처리해도 창을 제거하지 않는 방향으로 동작한다. 이는 숨김 폴백 재현에는 유용하지만 **정상적으로 닫힌 경우**를 제대로 검증하지 못한다. 기존 fake 기본 동작을 즉시 변경하여 구 테스트를 깨지 말고 **창별 제어 가능한 시나리오**를 추가하라.

제안하는 테스트 전용 기능(실제 Win32와 분리):

```text
set_close_behavior(hwnd, Destroy | DeliveredButAlive | Timeout | DeliveryFailure)
set_window_text_result(hwnd, Known | Unknown | Truncated)
remove_window(hwnd) / recreate_window(hwnd, pid, class, ...)
set_fail_show_window(hwnd, bool)
set_fail_set_window_pos(hwnd, bool)
set_hung(hwnd, bool)
take_api_calls() -> ordered calls (target, method, args, result)
snapshot_window_state() -> visible/style/rect/owner/parent
```

API가 이미 지원하는 옵션은 재사용하고 불필요한 중복 구현은 피하라. `Timeout`은 **실제 테스트를 500ms × N 대기시키지 않는 결정론적 가상 결과**로 재현하되, 별도 Win32 테스트에서 실제 timeout 예산을 확인한다. `WM_CLOSE` 후 창 파괴되면 스냅샷/후속 처리·카운터와 fake의 window 존재 여부를 일치시킨다.

### 5.2 반드시 추가할 상태 전이 테스트

| ID | 전이·장애 | 독립 검증 기대값 |
|---|---|---|
| F01 | 시작→광고 탐지→hide→동일 광고 지속 | 지속 은닉, 최초 상태 보존, 무한 스냅샷 증식 없음 |
| F02 | 팝업 `WM_CLOSE` 성공→창 소멸 | 존재하지 않는 창에 후속 hide/resize 금지, 실제 closed 카운터만 증가 |
| F03 | 전달 성공이지만 창 생존 | hide/zero-size 폴백 정상, OFF 시 가능한 변경 원복 |
| F04 | Timeout/DeliveryFailure | 워커는 중단되지 않고 대기 예산 관리, 오류 관측, 안전한 폴백 |
| F05 | aggressive ON→OFF→ON | 공격 모드에만 해당하던 숨김 대상 복원, 재활성화 때 조건부 재차단 |
| F06 | blocker ON→OFF→ON 반복 | OFF에서 실제 API 변경 호출 없음, 잔여 숨김 없음, ON 정상 복귀 |
| F07 | 광고 텍스트가 정상으로 변화 | 규정된 grace/miss 이후 숨김 해제, 원래 가시성 보존 |
| F08 | 앱 종료 또는 업데이트 전 shutdown | healthy-first 복원, time-bound 종료, 실패 잔여 건수 보고 |
| F09 | 카카오톡 PID 교체/HWND 재사용 | 구 스냅샷으로 타 프로세스/새 창을 복원하지 않음 |
| F10 | 스냅샷 확보 후 ShowWindow 실패 | 실패를 성공으로 기록하지 않으며 재시도 정책·관측 일치 |
| F11 | SetWindowPos 실패/부모 변경 | 기존 정상 창 위치를 오염시키지 않음, 복구 시 좌표 혼동 방지 |
| F12 | 렌더링 중 트리·제목 변경 | 탐지 결과의 stale handle에 무조건 Win32 변경 호출하지 않음 |
| F13 | WinEvent hook 실패→polling | 주기적 재탐지 계속, 과도한 CPU 스핀 없음 |
| F14 | tick panic→다음 tick 정상 | 스냅샷 유지, 복원 가능, 오류는 분리·관측 가능 |
| F15 | worker 비정상 종료 | 건강 상태를 거짓 PASS로 표시하지 않음, 사용자에게 상태 알림 |
| F16 | 종료 중 카카오톡 응답 없음 | 무한 대기/보이지 않는 고아 프로세스 없음. 복원 미완료가 있으면 명시적 기록 |
| F17 | 자손 창에서 `AdFitWebView` 발견, 정상 host | child/host 닫거나 숨기지 않음; 정상 미디어 뷰어 보존 |
| F18 | 빈 EVA child에 광고 신호 없음 | 닫지 않음; 후속 tick에 스크롤 자손 등장해도 오탐 금지 |

**중요**: `WM_CLOSE`로 파괴된 광고 팝업을 ‘종료 시 원복’ 대상으로 판단하지 않는다. close의 비가역성 때문에 정책상 위험한 정상 창은 **닫지 않아야 한다**. 정상 닫기/거부/숨김 상태를 별도 출력으로 검증한다.

### 5.3 강한 assertion

- 테스트마다 최종 `WindowState`와 **Win32 호출 로그** 모두 검증한다. 최종 상태가 정상처럼 보여도 중간에 정상 창에 `WM_CLOSE`가 갔다면 FAIL.
- `target_pid ∈ KakaoTalk PID set`, valid identity, nonhung predicate를 실제 API 호출 직전에 검사했는지 관찰.
- `restore_failures`는 누적 횟수가 아닌 **현재 복원 실패 창 수 gauge**임을 유지.
- `GetWindowRect` 스냅샷은 최초 적용 시 1회만; 메인 뷰 리사이즈는 복원 스냅샷에 넣지 않는 불변식 유지.
- 기존 `restore_regression.rs`, `scan_parity.rs`, `apply_path_parity.rs`의 시나리오를 삭제하지 않고 확장.

**완료 조건**: 테스트 Fake만으로 광고/정상 창 선택·Win32 행동·복원·오류 처리가 끝까지 관측된다. 최소 F01~F18 모두 테스트 또는 근거 있는 통합/실기 검증으로 매핑되며, 자동 테스트에서 불가능한 항목은 `MANUAL_REQUIRED`로 기록.

---

## 6. Phase 3 (P1) — 카카오톡 실제 UI 변화 탐지 및 라이브 호환성 실험

### 6.1 실제 앱은 CI와 분리

GitHub 호스티드 `windows-latest` 러너에서 카카오톡 실제 광고를 항상 노출·검증할 수 있다고 가정하지 말 것. **자동 CI = 코드/fixture 품질**, **격리된 Windows 실기 = 실제 광고 차단 호환성**으로 구분한다. 카카오톡 계정·대화 내용·UI를 포함한 실기 캡처는 반드시 허가된 테스트 계정·가상/전용 환경에서 수행.

### 6.2 재현 가능한 캡처 프로토콜

1. Windows 버전/빌드, x64/ARM64 실행 방식, DPI, 모니터 구성, 카카오톡 `exe` 파일 버전, 차단기 버전, 테스트 시각을 기록.
2. 먼저 `--shadow`로 동작 확인. **차단기를 실행하지 않은 baseline**에서 광고/정상 UI를 수동 분류하고 스크린샷 증거 취득(민감정보 제거).
3. `--dump-tree`와 상황별 `--dump-tree-series`를 획득. **원본 파일을 업로드하기 전 window title/text·사용자명·대화명 익명화**하고, 익명화가 시그니처를 깨지 않았는지 점검.
4. 사람이 판정한 행동 기대값을 기록하고, 시뮬레이션 모드의 action과 비교.
5. 사용자 동의 하에 실제 blocking ON으로 바꿔 UI의 광고 부재·정상 기능·레이아웃을 검사. ON/OFF 왕복을 확인하고 기능 실패가 있으면 즉시 OFF/프로세스 종료, 필요 시 카카오톡 재시작.
6. 릴리스 비교는 동일한 테스트 머신·가능하면 동일한 카카오톡 버전에서 **이전 버전 대 새 버전** 순서 교차로 진행. 광고 배포 A/B나 네트워크 조건의 차이는 별도 변동성으로 기록.

### 6.3 UI 변동 탐지기(권장 추가 구현)

별도 테스트용 `scripts/compare_layout_fixtures.py` 또는 Rust CLI로 **PII를 제거한 창 구조 특징** 비교 리포트를 생성한다. 기존 `--dump-tree`를 재사용하며, 구조 신호는 PID 절대값·HWND 절대값·타임스탬프에 의존하지 않도록 정규화:

- 메인 창 후보 수와 클래스/제목 가드, `OnlineMainView`/`LockModeView` 유무.
- 광고 후보 클래스 분포·자손 깊이, known/unknown title 비율.
- 하단 배너 기하 조건, 사용된 룰·결정 강도·가드 거부 이유.
- 매 tick 신규 후보/사라진 후보/실제 hide·close·fallback·restore 수.

**변경 징후가 검출된 새 버전은 `UNVERIFIED`**로 표시하고, 임의로 규칙을 완화하지 말 것. 분류는 `SUPPORTED`(해당 버전/환경 실제 검증), `FIXTURE_ONLY`, `UNVERIFIED`, `KNOWN_BROKEN` 등을 사용. 구조 diff의 존재 자체가 광고 미차단/오탐을 뜻하지는 않으므로 사람이 확인한다.

### 6.4 실기 체크리스트 (증거 첨부)

- 친구·채팅·잠금·프로필·사진/동영상·설정·개별 채팅방·이모티콘/웹뷰·로그인/로그아웃 UI의 정상 작동.
- 하단 배너, 시작 시 광고, 독립 `AdFitWebView` 팝업의 실제 노출/미노출. **광고가 등장하지 않은 세션은 차단 성공 PASS가 아니라 `NOT OBSERVED`**.
- 창 최소화/최대화/리사이즈/다중 모니터 이동/125~150% DPI/트레이 숨김 후 복귀.
- 카카오톡 종료·재실행, blocker 먼저 실행, blocker OFF/ON·종료, 비정상 종료 뒤 복구.
- self-check, 설정 경로, 트레이 아이콘, 시작프로그램 등록, 업데이트 성공/실패 롤백.
- 1시간 이상 soak는 기존 `docs/SOAK_TEST.md`를 기반으로 하되 **실측 로그와 재현된 행동**을 기록. 대표 출시 대상 장비는 확대 soak(예: 4~8시간) 실시.

**완료 조건**: 카카오톡 버전별로 `checked-at`, 실제 광고 관측 여부, 정상 UI 검증, 익명화된 증거, PASS/FAIL/NOT OBSERVED/NOT RUN이 담긴 `docs/testing/COMPATIBILITY_EVIDENCE.md`가 존재.

---

## 7. Phase 4 (P1) — 속성 기반·Fuzz·Mutation 검증

### 7.1 Property testing / Fuzzing

`proptest` 등 도구 사용은 적합성·빌드비용을 검토한 뒤 **테스트 전용 의존성**으로 추가. 다음 불변식을 랜덤 데이터 + seed 고정 회귀 fixture로 검증한다:

- 정상 PID 또는 가드 거부 창에 대한 `hide/close/zero-size` 없음.
- 토큰 제거가 의도치 않은 새 광고 일치를 증가시키지 않음(명시적 tokenless 옵션은 예외).
- `weak_signal_confirm_ticks` 전에는 weak 후보 동작이 발생하지 않음.
- 깊이 제한 초과, 빈/잘린/unknown 문자열, 많은 sibling, 순환/잘못된 부모 데이터에서도 panic/스택 고갈/무한 루프가 없다.
- 판별 입력 순서가 바뀌어도 관측 결과가 같아야 하는 범위 정의(윈도우 열거순서/중복 제거). 순서 의미가 있는 실제 API 적용은 별도 계약으로 구분.
- 값 범위 밖의 설정(JSON 잘못된 타입·극단값·null·BOM·깨진 UTF-8)은 오류 처리/기본값/경고 계약대로 반응하며 막연히 crash하지 않음.

실기 캡처가 필요한 fuzz는 금지. 임의 데이터를 만들더라도 테스트에서 **실제 Win32를 호출하지 말 것**.

### 7.2 Mutation testing

- `cargo-mutants`(사용 시 설치·버전 기록)로 **선정된 핵심 함수** 위주 시작: `signals.rs`(판별·host guard·weak state), `layout.rs`(기하/토큰), `evaluate/apply.rs`(행동 선택), `engine/apply.rs`(PID·identity·stopping 가드), `engine/restore.rs`(재시도·복원·identity), 필요 시 updater 검사.
- 첫 실행 전 baseline 테스트가 통과하는지 검증. 장시간 소요/Windows 환경을 고려해 **야간·수동 job**으로 분리.
- 살펴볼 의도적 변이: `>=`↔`>`, `&&`↔`||`, guard 무력화, `weak_signal_confirm_ticks` 무시, `is_hung_app_window` 검사 제거, `identity_matches` 제거, `popup_host_require_empty_text` 뒤집기, snapshot 저장 생략, grace 변경.
- `cargo mutants --list`로 후보 확인하고 **현재 버전 CLI `--help`를 확인한 뒤** 해당 함수/파일로 범위 제한하여 실행. `--timeout` 지정으로 hang 방지. 출력 `mutants.out` 등을 artifact로 저장.
- `caught`, `missed`, `timeout`, `unviable`, `equivalent` 구분. `missed`는 테스트 강화 또는 의미 동등성 근거를 제출; `timeout`은 test hang 가능성을 검토. **중요 가드 변이는 생존 0건**을 목표로 하고, 기타 유효 변이는 80% 이상 검출을 개선 목표로 설정하되 전체 비율 하나로 P0 결함을 덮지 말 것.

공식 사용 설명: https://mutants.rs/ , https://mutants.rs/timeouts.html

### 7.3 Coverage 지표는 보조적

- 가능하면 `cargo-llvm-cov`로 `kakao-core` 및 위험 앱 코드의 라인/브랜치 커버리지 추적. 1차는 baseline 측정, 2차는 **핵심 동작의 branch 미검증 영역 감소**를 목표로 삼는다.
- 단순 전역 90% 달성 등을 릴리스 유일 조건으로 쓰지 말 것. 자동 생성 모델·Win32 FFI 비가역 부분의 커버리지보다는 **negative oracle, 상태 전이, P0 mutant 결과**가 우선.
- 테스트용 coverage 스크립트는 CI와 같은 옵션을 사용하고 보고서가 누락돼도 성공 표시하지 말 것.

**완료 조건**: 재현 가능한 seed, 핵심 mutation 생존자 목록·해소 여부, 타겟 coverage 보고서, 새 회귀 테스트가 남음.

---

## 8. Phase 5 (P1) — Windows CI / Nightly / Release Gate 3단계

### A. PR 필수 게이트(빠르고 결정론적)

현재 `.github/workflows/windows-ci.yml`을 유지·개선하여 다음 실패는 PR에서 즉시 차단:

```text
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --workspace             # 기존 + 신규 independent contract/transition
cargo build --release -p kakao-app -p kakao-updater
packaged --self-check --strict-self-check --json  # 러너 환경 영향 확인 후 적용
fixture schema validation / label metadata validation
no-PII/secret-on-fixtures check
```

- `--strict-self-check`는 **APPDATA, HKCU, 프로세스 스캔 등 환경 검사를 강화**하므로 Windows CI에서 실제 예상 동작을 확인한 다음 적용한다. 러너 환경이 지원하지 않으면 `ENVIRONMENT_BLOCKED`와 이유를 기록, 무조건 정상 통과 처리하지 말 것.
- 실행 가능한 코드 변경과 contract fixture 변경을 `paths-ignore`가 건너뛰지 않는지 확인. 현재 workflow는 Markdown-only 변경을 무시하므로 설명서만 바꾼 커밋의 CI 결과가 없을 수 있다.
- `ignored` 테스트는 **테스트 실기 대상**과 **일반 CI 제외 이유**를 표로 기록한다. ignored가 늘어난 이유 없이 증가하면 검토 대상으로 삼는다.
- 체크 실행이 실패했는데 `continue-on-error` 등으로 최종 녹색 처리하는 방식 금지.

### B. Nightly / 수동 심화 테스트

- 선택된 mutation 테스트, fuzz deterministic+random seed 반복, 오래 걸리는 Windows real API smoke, 장시간 fake soak, 업데이트 실패 주입.
- 실제 카카오톡 로그인이 필요한 unattended 작업은 GitHub 제공 runner에서 임의 수행 금지. 전용 테스트 머신이 있을 때만 권한·비밀 관리·PII 정책을 먼저 정의.
- 리소스 과다/플레이키 테스트를 빠른 PR 잡에 넣지 않고 별도 수집. failure rerun은 원인을 밝히는 증거이며 성공 판정 대체 불가.

### C. Release 필수 게이트

- `release.yml`에서 **태그 빌드 전에** 최신 기준선 테스트 상태·fixture 계약 결과·버전 확인·서명/업데이트 매니페스트 테스트가 정상인지 보장하도록 연동. `windows-ci`가 통과하지 않은 커밋의 태그 릴리스는 차단 또는 명시적 승인 절차 마련.
- 실사용 호환성 증거는 자동적으로 얻을 수 없으므로 별도 수동 승인 단계와 체크리스트/결과 파일을 두고, 실제 광고 미관측은 **미검증**으로 남겨둔다.
- 릴리스 아티팩트의 `--self-check`와 기본 실행/종료, 차단 끄기/복원, 롤백이 기존 실행파일 그대로 작동하는지 smoke 기록.

### 릴리스 판정

| 상태 | 기준 | 릴리스 처리 |
|---|---|---|
| BLOCK | FP 1건 이상, 타 앱 HWND 변경, 심각한 복원 실패, 독립 oracle 불일치, 핵심 guard mutant 생존 | 차단·원인 수정 |
| BLOCK | 기존에 동작하던 광고 사례의 재현 가능한 FN 회귀 | 차단·최소 수정 후 재검증 |
| REVIEW | 새 KakaoTalk 버전 구조 차이, 실제 광고 미관측, 실기 환경 미제공 | `UNVERIFIED` 표시·수동 검증/보류 판단 |
| PASS (CI) | 빠른 테스트·빌드·fixture 계약 모두 통과 | 코드 품질 게이트만 PASS |
| PASS (Release) | CI PASS + 적절한 실기 광고/정상 UI 증거 + 원복·릴리스 체크 | 해당 **확인된 버전/환경**에 한해 배포 승인 |

---

## 9. 관측 및 실패 재현 지원

실사용에서 광고 미차단이 발생하면 사용자가 원인을 재현하기 쉽도록, 기존 `--shadow`, `--dump-tree`, `--dump-tree-series`를 결합한 **진단 절차를 문서화**한다. 현행 CLI가 지원하는지 확인하지 않고 인자를 새로 약속하지 말 것.

권장 진단 출력 항목:

- 차단기 버전, KakaoTalk exe version, Windows/DPI, enabled/aggressive, 현재 룰 해시(비밀정보 제외).
- 후보 `hwnd`의 임시 식별자, PID/클래스, 결정 근거, strength, match/miss streak, host guard 결과.
- 광고 후보별 `detected`, `pending`, `hide`, `WM_CLOSE-delivered`, `destroyed`, `hide fallback`, `zero-size fallback`, `restore-attempt/result` 구별.
- 엔진 health: last scan time, hook 설치 여부/폴링 폴백, worker alive, restore failure gauge, 최근 익명화된 실패 원인.

**데이터 최소화:** GUI 텍스트를 원시 로그에 무제한 기록하지 말 것. `--dump-tree`는 개인정보를 담을 수 있으므로 익명화/동의 방법을 문서에 명시. 디버그 로그 생성·전송은 기본 OFF이며 사용자가 검토 후 수동 제출한다.

추천 사용자 버그리포트 템플릿:

```text
Adblocker version / KakaoTalk version / Windows build / DPI:
Scenario (친구 탭·채팅 탭·팝업 등):
What should happen (독립 판정):
What actually happened:
Blocker enabled / aggressive / shadow:
Ad actually observed? YES / NO / UNKNOWN:
ON→OFF restoration result:
Sanitized dump/series attached? YES / NO:
Reproduction steps & redacted screenshots:
```

---

## 10. 단계별 우선순위 및 실제 파일 작업 명세

| 단계 | 우선순위 | 예상 변경 파일/산출물 | 우선 검증 |
|---|---|---|---|
| 0 기준선 | P0 | `docs/testing/BASELINE.md`, 리스크 맵 | baseline 명령어·실행 로그 |
| 1 독립 oracle | **P0** | `tests/fixtures/contract/**`, `rust/crates/kakao-core/tests/contract_oracle.rs`, `rule_boundaries.rs` | FP/FN Positive·Negative·경계 |
| 2 FakeWin32 | **P0** | `kakao-win32/src/fake.rs`(테스트 목적), `kakao-app/tests/restore_regression.rs`, 신규 transition test | 성공/거부/timeout/복원/타 PID |
| 3 실기 호환성 | P1 | `docs/testing/COMPATIBILITY_EVIDENCE.md`, 익명화 검사 도구, 기존 SOAK 문서 개정 | 실제 광고 출현시 차단·정상 UI 유지 |
| 4 Mutation/Fuzz | P1 | tests, dev-dependency, test scripts, `docs/testing/MUTATION_REPORT.md` | 중요 guard 변이 생존 0 |
| 5 CI/릴리스 | P1 | `windows-ci.yml`, 별도 nightly workflow(필요 시), `release.yml`, release checklist | 명시적 gate와 artifact |

### 구현 순서

- **첫 PR**: Phase 0 + Phase 1. 기존 프로덕션 판정 동작 변경 없이 테스트 증거 구축.
- **둘째 PR**: Phase 2. FakeWin32 확대 + 판별/적용/복원 전이 tests. 확정된 결함만 별도 소규모 product patch.
- **셋째 PR**: Phase 3~5. 실기 도구·문서, mutation/fuzz, CI. 외부 환경 미제공 시 미실행을 정확히 표시하며 자동화 가능한 부분은 완료.

*위 순서는 권장 검토 단위일 뿐이며 에이전트는 단일 작업 환경에서 연속으로 구현·검증해도 된다. 임의 merge·release/tag push는 하지 말 것.*

---

## 11. 결과 보고서 템플릿 — 반드시 실제 수치·근거만 제출

최종 `docs/testing/TEST_HARDENING_REPORT.md`에 다음을 포함:

```markdown
# KakaoTalk Adblock Test Hardening — Execution Report

## 1. Scope
- Base HEAD / Final HEAD:
- KakaoTalk version(s) actually tested:
- Windows build / architecture / DPI:
- Tests executed on (CI / local Windows / dedicated lab):

## 2. Changes
- Files changed:
- New tests and scenario IDs:
- Product behavior changed? If yes, issue/repro/fix/diff:

## 3. Actual verification (never infer PASS)
| Command/Scenario | PASS/FAIL/NOT RUN/NOT OBSERVED | Counts | Evidence |
|---|---|---|---|
| cargo test --workspace | ... | ... | ... |
| Independent Oracle | ... | ... | ... |
| Negative / No-FP | ... | ... | ... |
| FakeWM_CLOSE paths | ... | ... | ... |
| Restoration & shutdown | ... | ... | ... |
| Mutation & survivors | ... | ... | ... |
| CI/release build | ... | ... | ... |
| Actual KakaoTalk ad observation/block | ... | ... | ... |

## 4. Mutation
- Mutants generated / viable / caught / missed / timeout / equivalent:
- Important surviving mutants and mitigation:

## 5. Failures & incidents
- RED reproduction, patch, GREEN result:
- Flaky tests and reproduction seed:

## 6. Known limits & residual risks
- Real KakaoTalk not installed? State NOT RUN.
- Ad never appeared? State NOT OBSERVED, NOT PASS.
- Environment-only warning? State details.

## 7. Release decision
- CI gate: PASS / BLOCK
- Live compatibility: SUPPORTED / FIXTURE_ONLY / UNVERIFIED / KNOWN_BROKEN
- Release recommendation with rationale:
```

**반드시 보고:** 기존 테스트 대비 신규 테스트 수, 실패 시나리오와 원인, 주요 FP/FN 검증, 복원 실패 카운터 관측 결과, 테스트가 잡아낸 mutation 수/생존자, 실제 카카오톡 라이브 검증 여부. 결과 수치를 추정하거나 임의 기입 금지.

---

## 12. Definition of Done (에이전트가 종료를 주장할 수 있는 기준)

- [ ] 현재 HEAD, 기존 테스트 결과, CI 제약, 잠재 위험이 문서화되었다.
- [ ] **사람/독립 자료가 검토한** positive+negative 기대값과 검증 가능한 fixture가 추가되었다.
- [ ] 핵심 규칙 경계, title known/unknown, main-window/owner 가드, 광고 토큰의 오탐 음성 사례가 테스트된다.
- [ ] `FakeWin32`는 WM_CLOSE 성공 파괴/미파괴/전송실패/timeout을 개별 시나리오로 재현한다.
- [ ] OFF/ON, aggressive toggle, 종료, PID 교체, 응답 없음, stale 복원의 최종 상태 및 API 호출 로그를 검증한다.
- [ ] 실제로 재현된 결함만 최소 수정하였고 기존 golden·normal-window 회귀가 유지된다.
- [ ] mutation 실행 및 **핵심 guard 생존자에 대한 조치**가 문서화되었다(미실행 시 해당 Gate 미달).
- [ ] Windows CI에서 테스트 실패가 실제 workflow 실패로 연결되고 증거 artifact를 확인한다.
- [ ] 실기 광고 테스트가 수행됐는지, 광고를 실제로 봤는지 명시한다. 불가능한 것은 미검증으로 남긴다.
- [ ] CI와 실기 결과를 구별한 최종 보고서가 있으며, 사용자가 릴리스 가능 여부를 객관적으로 판단할 수 있다.

**핵심 원칙: 테스트가 많다는 사실이 아니라, 광고가 남는 결함과 정상 UI가 사라지는 결함을 각각 재현하고 반드시 실패시키는 증거를 확보하는 것이 목표다.**

---

## 참고 경로 (2026-10-12 기준 점검)

- 저장소: https://github.com/twbeatles/kakaotalk-layout-adblocker
- 판별: https://github.com/twbeatles/kakaotalk-layout-adblocker/blob/main/rust/crates/kakao-core/src/signals.rs
- FakeWin32: https://github.com/twbeatles/kakaotalk-layout-adblocker/blob/main/rust/crates/kakao-win32/src/fake.rs
- 복원 회귀: https://github.com/twbeatles/kakaotalk-layout-adblocker/blob/main/rust/crates/kakao-app/tests/restore_regression.rs
- Golden parity: https://github.com/twbeatles/kakaotalk-layout-adblocker/blob/main/rust/crates/kakao-core/tests/golden_parity.rs
- CI: https://github.com/twbeatles/kakaotalk-layout-adblocker/blob/main/.github/workflows/windows-ci.yml
- 감사: https://github.com/twbeatles/kakaotalk-layout-adblocker/blob/main/PROJECT_AUDIT.md
- 수동 soak: https://github.com/twbeatles/kakaotalk-layout-adblocker/blob/main/docs/SOAK_TEST.md
- Rust mutation testing: https://mutants.rs/
