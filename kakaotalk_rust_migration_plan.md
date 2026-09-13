# KakaoTalk PC AdBlocker — Rust Native Migration Plan

> 대상 저장소: `twbeatles/kakaotalk-layout-adblocker`  
> 목적: 기존 Python 구현의 동작/안전성을 보존하면서 Rust 기반 Windows 네이티브 애플리케이션으로 전환한다.  
> 핵심 전략: **동작 규격 고정 → 순수 Rust 판단 엔진 → Win32 래퍼 → Shadow Mode → 이벤트 기반 감시 → 실제 mutation → UI/업데이터 → Python 제거**  
> 최우선 원칙: **오탐 방지와 원상복구 안전성이 성능보다 우선한다.**

**진행 상태:** Rust가 기본 구현이다 (`kakao-adblock-rs` / `dist/KakaoTalkLayoutAdBlocker_v11.exe`). Python v11은 `legacy/python-v11/`. 실기기 오탐 매트릭스는 수동.

---

## 1. 에이전트 작업 지침

이 문서를 구현 계약으로 취급한다.

작업 시작 시 반드시 다음 순서를 지킨다.

1. 저장소 루트의 `README.md`, `CLAUDE.md` 및 현재 개발 지침 문서를 읽는다.
2. 현재 Python 구현과 테스트를 삭제하거나 우회하지 않는다.
3. 기존 Python 구현을 **reference implementation**으로 취급한다.
4. 새 Rust 구현은 처음부터 실제 카카오톡 창을 변경하지 않는다.
5. 기존 window dump fixture와 테스트를 Rust 회귀 테스트의 기준 데이터로 재사용한다.
6. 기능 동등성이 검증되기 전까지 Python 버전을 유지한다.
7. 포팅 중 기존 Python 코드의 동작이 불명확하면 임의로 단순화하지 말고 테스트를 추가해 현 동작을 명시한다.
8. Win32 raw/unsafe 코드는 반드시 별도 계층으로 격리한다.
9. 모든 창 변경은 복원 가능한 상태를 먼저 기록한 뒤 수행한다.
10. `Close`는 되돌릴 수 없으므로 Hide/Resize보다 훨씬 보수적으로 적용한다.
11. 성능 개선을 위해 안전 검사를 제거하지 않는다.
12. 구현 단계마다 `cargo fmt`, `cargo clippy --all-targets --all-features`, `cargo test`를 통과시킨다.

---

# 2. 현재 구조에서 보존해야 할 핵심

현재 Python 코드는 대략 다음 책임을 이미 분리하고 있다.

```text
kakao_adblocker/
├─ event_engine/
│  ├─ controller.py
│  ├─ scanner.py
│  ├─ signals.py
│  ├─ actions.py
│  ├─ models.py
│  └─ dump.py
├─ layout_engine.py
├─ win32_api.py
├─ config.py
├─ protocols.py
└─ services.py
```

Rust 구현에서도 이 책임 경계를 최대한 유지한다.

현재 기능 중 반드시 보존해야 하는 항목:

- 카카오톡 프로세스 탐지
- 메인 윈도우 탐지
- child/owned/popup window 탐지
- 광고 후보 신호 평가
- conservative / aggressive 판정
- 광고창 Hide
- popup Close 또는 fallback Hide/zero-size
- 메인 레이아웃 Resize
- 변경한 창 원상복구
- ON/OFF 토글
- aggressive mode 토글
- cache 및 candidate state 관리
- 종료 중 새 mutation 금지
- watcher 종료 후 restore
- diagnostics/window dump
- 설정 및 규칙 파일
- tray
- 시작프로그램
- updater
- 로그/오류 상태
- HWND 재사용에 대한 방어

---

# 3. 목표 아키텍처

권장 새 디렉터리:

```text
rust/
├─ Cargo.toml
├─ crates/
│  ├─ kakao-core/
│  │  └─ src/
│  │     ├─ lib.rs
│  │     ├─ model.rs
│  │     ├─ rules.rs
│  │     ├─ classifier.rs
│  │     ├─ signals.rs
│  │     ├─ decisions.rs
│  │     └─ snapshot.rs
│  │
│  ├─ kakao-win32/
│  │  └─ src/
│  │     ├─ lib.rs
│  │     ├─ api.rs
│  │     ├─ process.rs
│  │     ├─ window.rs
│  │     ├─ event_hook.rs
│  │     └─ mutation.rs
│  │
│  └─ kakao-app/
│     └─ src/
│        ├─ main.rs
│        ├─ engine/
│        │  ├─ controller.rs
│        │  ├─ scanner.rs
│        │  ├─ actions.rs
│        │  ├─ restore.rs
│        │  └─ state.rs
│        ├─ config/
│        │  ├─ settings.rs
│        │  └─ rules.rs
│        ├─ diagnostics/
│        │  ├─ dump.rs
│        │  └─ logging.rs
│        ├─ tray/
│        ├─ startup/
│        └─ updater/
└─ tests/
   ├─ fixtures/
   └─ parity/
```

초기에는 기존 Python 루트와 `rust/`를 공존시킨다.

최종 전환 이후 Rust를 저장소 루트로 올리는 것은 별도 cleanup 단계에서 수행한다.

---

# 4. 권장 기술 스택

## 필수

- Rust stable
- `windows` (`windows-rs`)
- `serde`
- `serde_json`
- `thiserror`
- `tracing`
- `tracing-subscriber`
- `crossbeam-channel` 또는 `tokio::sync` 중 하나

### 비동기 런타임

이 앱의 핵심은 Win32 callback + event queue이므로 Tokio를 무조건 도입할 필요는 없다.

우선 권장:

```text
WinEvent callback
    ↓
crossbeam channel
    ↓
single engine worker
```

네트워크 updater 등에서 async가 필요해질 때만 `tokio` 도입 여부를 판단한다.

## 선택

- tray: 유지보수 상태가 양호한 Rust tray crate
- UI: `slint` 우선 검토, 대안 `egui/eframe`
- HTTP updater: `reqwest`
- hash/signature: 기존 updater 방식에 맞춰 SHA-256 / Ed25519 라이브러리 선정
- release packaging: WiX / Inno Setup / cargo-wix 등 검토

## 피할 것

- Tauri/WebView를 단순 설정창 때문에 도입
- C/C++ DLL 추가
- Win32 호출을 여러 crate에 분산
- global mutable state
- callback 내부에서 직접 복잡한 window mutation 수행

---

# 5. 데이터 모델 설계

Rust에서는 문자열 기반 상태를 적극적으로 타입화한다.

예시:

```rust
pub struct WindowIdentity {
    pub hwnd: isize,
    pub pid: u32,
    pub class_name: String,
}

pub struct WindowSnapshot {
    pub identity: WindowIdentity,
    pub parent: Option<isize>,
    pub owner: Option<isize>,
    pub rect: Option<Rect>,
    pub visible: bool,
    pub title: WindowText,
}

pub enum WindowText {
    Known(String),
    Unknown { error_code: u32 },
    Truncated(String),
}

pub enum AdDecision {
    Ignore,
    MainWindow,
    AdCandidate { confidence: Confidence, reason: DecisionReason },
    PopupAd { confidence: Confidence, reason: DecisionReason },
}

pub enum ActionPlan {
    None,
    Hide,
    Resize(Rect),
    RequestClose,
    ZeroSizeFallback,
    Restore,
}
```

`HWND` 하나만으로 장기 identity를 정의하지 않는다.

필요하면 다음을 조합한다.

- HWND
- PID
- class name
- first_seen generation
- parent/owner
- process generation

HWND reuse 때문에 이전 hidden snapshot을 새 창에 적용해서는 안 된다.

---

# 6. Phase 0 — 기준 동작 고정

## 목표

Python 버전을 회귀 테스트 가능한 “정답 구현”으로 만든다.

## 작업

### 6.1 기존 테스트 조사

다음 범주 테스트를 목록화한다.

- scanner
- signal evaluator
- actions
- restore
- config
- window dump
- tray
- updater
- shutdown
- HWND reuse
- popup
- aggressive mode

### 6.2 Golden fixture 생성

기존 window dump를 다음 중 하나로 정규화한다.

```text
tests/fixtures/windows/*.json
```

권장 포맷:

```json
{
  "windows": [],
  "settings": {},
  "rules": {},
  "expected": {
    "main_windows": [],
    "ad_candidates": [],
    "decisions": [],
    "actions": []
  }
}
```

### 6.3 Python 결과 exporter 추가

필요하면 개발용 CLI를 추가한다.

예:

```bash
python -m kakao_adblocker.dev.export_fixture_decisions tests/fixtures/windows
```

출력은 deterministic JSON이어야 한다.

### 완료 조건

- 주요 실사용 dump가 fixture로 보존됨
- Python 테스트 전체 통과
- 동일 fixture에 대한 예상 decision/action이 JSON으로 고정됨
- Rust 구현이 비교할 수 있는 stable schema가 존재함

---

# 7. Phase 1 — `kakao-core` 구현

실제 Windows API를 호출하지 않는 순수 Rust crate부터 만든다.

## 이관 대상

Python 기준:

```text
event_engine/models.py
event_engine/signals.py
판정 관련 scanner 로직
rules/config의 순수 데이터 구조
```

## 구현할 API 예시

```rust
pub fn classify_window(
    window: &WindowSnapshot,
    graph: &WindowGraph,
    rules: &LayoutRules,
    candidate_state: Option<&CandidateState>,
) -> AdDecision;
```

그리고:

```rust
pub fn plan_action(
    decision: &AdDecision,
    settings: &LayoutSettings,
) -> ActionPlan;
```

## 요구 사항

- Win32 의존성 없음
- 테스트에서 가짜 window graph만으로 실행 가능
- 모든 decision reason을 enum으로 표현
- aggressive/conservative 차이를 명확히 표현
- unknown title과 empty title을 구분
- truncated title 처리
- main window와 ad candidate 분류 충돌 방지

### 완료 조건

Golden fixture 전부에 대해:

```text
Python decision == Rust decision
Python action plan == Rust action plan
```

100% 일치.

일치하지 않는 경우 Rust 쪽을 임의로 “개선”하지 말고 이유를 기록하고 별도 변경으로 다룬다.

---

# 8. Phase 2 — `kakao-win32` 구현

현재 `win32_api.py`의 역할을 Rust로 이전한다.

## 최소 API

- EnumWindows
- EnumChildWindows
- GetWindowThreadProcessId
- GetClassNameW
- GetWindowTextLengthW
- GetWindowTextW
- GetParent
- GetWindowRect
- GetClientRect
- IsWindow
- IsWindowVisible
- ShowWindow
- SetWindowPos
- SendMessageW
- SendMessageTimeoutW
- UpdateWindow
- 필요한 owner 조회 API

## 설계 원칙

raw handle과 `unsafe`는 `kakao-win32` 안에 격리한다.

상위 crate는 다음처럼 사용한다.

```rust
let info = windows.snapshot(hwnd)?;
windows.hide(hwnd)?;
windows.set_position(hwnd, rect)?;
```

## 오류 정책

`GetWindowTextLengthW == 0`을 무조건 빈 문자열로 간주하지 않는다.

Python 구현처럼:

- known empty
- API failure
- truncated

을 구분한다.

## 테스트

실제 KakaoTalk 없이 가능한 Win32 smoke test를 작성한다.

테스트용 자체 window를 생성하거나 안전한 OS window 조회만 수행한다.

### 완료 조건

- wrapper API에 raw Win32 호출이 캡슐화됨
- 상위 core crate에 `unsafe` 없음
- Windows x64 release build 통과
- Win32 smoke test 통과

---

# 9. Phase 3 — Rust Shadow Mode

## 목적

실제 창을 변경하지 않고 Rust 엔진의 탐지 정확도를 실사용 환경에서 검증한다.

## 실행 모드

```bash
kakao-adblock-rs.exe --shadow
```

Shadow mode:

- 카카오톡 프로세스 탐지: O
- window enumerate: O
- signal 평가: O
- decision: O
- action plan 생성: O
- 실제 Hide/Resize/Close: X

로그 예:

```text
[shadow]
hwnd=0x...
pid=...
class=...
decision=PopupAd
action=Hide
reason=...
```

## 비교

동일 시점 Python diagnostics dump와 Rust dump를 비교하는 보조 스크립트를 만든다.

비교 대상:

- Kakao PID
- main window
- child/owned window graph
- ad candidate
- decision
- planned action

### 완료 조건

실제 다양한 KakaoTalk 상태에서 parity 확인:

- 친구 목록
- 채팅 목록
- 채팅방
- 설정창
- popup
- 광고 표시 전/후
- 창 최소화/복구
- KakaoTalk 종료/재실행

오탐 0을 목표로 한다.

---

# 10. Phase 4 — Polling 중심 → Event-driven Hybrid

## 목표

Rust 전환의 실제 native 효과를 만든다.

현재 방식의 짧은 polling interval을 주 경로에서 제거한다.

## 핵심

`SetWinEventHook`을 이용한다.

우선 검토 이벤트:

- `EVENT_OBJECT_CREATE`
- `EVENT_OBJECT_SHOW`
- `EVENT_OBJECT_HIDE`
- `EVENT_OBJECT_DESTROY`
- `EVENT_OBJECT_LOCATIONCHANGE`
- `EVENT_OBJECT_NAMECHANGE`
- `EVENT_SYSTEM_FOREGROUND`

## 구조

```text
WinEvent callback
    ↓
minimal event object
    ↓
channel
    ↓
engine worker
    ↓
deduplicate/coalesce
    ↓
targeted rescan
    ↓
decision
    ↓
action
```

callback 안에서는:

- 긴 작업 금지
- enumeration 금지
- mutation 금지
- logging 최소화
- channel send 중심

## Reconciliation scan

WinEvent를 완전히 신뢰하지 않는다.

예:

```text
event-driven targeted scan
+
2~5초 수준의 low-frequency reconciliation
+
Kakao process 시작 직후 burst scan
```

정확한 주기는 benchmark 및 실기기 검증 후 결정한다.

## Event coalescing

동일 HWND에 짧은 시간 여러 이벤트가 오면 하나로 합친다.

예:

```text
CREATE → NAMECHANGE → SHOW → LOCATIONCHANGE
```

를 모두 별도 full scan으로 처리하면 안 된다.

### 완료 조건

- 광고 대응 latency가 Python과 동등 이상
- idle polling wake-up 대폭 감소
- 이벤트 누락 시 reconciliation이 복구
- KakaoTalk 재실행 정상 탐지

---

# 11. Phase 5 — 실제 Window Mutation

단계적으로 활성화한다.

## 적용 순서

1. Hide
2. Hide restore
3. Resize
4. Resize restore
5. popup close request
6. close failure fallback
7. aggressive mode

`Close`는 마지막에 활성화한다.

## Mutation guard

모든 action 전에 다음 조건을 재확인한다.

- engine enabled
- app not stopping
- HWND still valid
- PID still KakaoTalk
- identity still matches
- target still classified as ad
- action is allowed by current mode

## Restore Snapshot

mutation 전 저장:

```text
WindowIdentity
visibility
rect
parent/owner
reason
timestamp
original state
```

종료/disable/aggressive-off 시 복원한다.

## shutdown

현재 Python 구현의 안전 속성을 유지한다.

```text
stop requested
↓
prevent new mutations
↓
wake engine worker
↓
join worker with timeout
↓
restore hidden/resized windows
↓
shutdown tray/app
```

### 완료 조건

- ON → OFF 시 원상복구
- 앱 정상 종료 시 원상복구
- 강제 오류 경로에서도 가능한 범위 복구
- stale HWND에 restore하지 않음
- restore failure 기록 가능

---

# 12. Phase 6 — 설정 호환

가능하면 기존 설정/규칙 파일을 그대로 읽는다.

예:

```text
layout_settings_v11.json
layout_rules_v11.json
```

Rust 구조체는 `serde`로 deserialize한다.

## 목표

Python → Rust 업데이트 시 사용자의 기존:

- enabled state
- aggressive mode
- polling 관련 사용자 설정
- rules
- custom override

를 잃지 않는다.

사용하지 않게 된 legacy polling option도 첫 버전에서는 parser 호환을 위해 받아들이고 deprecation 처리할 수 있다.

---

# 13. Phase 7 — Tray / Settings UI

엔진이 안정화된 후 구현한다.

## 최소 tray

```text
차단 활성화
공격적 차단
설정
진단 정보 저장
로그 열기
버전 정보
종료
```

UI는 기능 중심으로 작게 유지한다.

## 반드시 지킬 것

- tray crash가 engine 상태를 비정상으로 남기지 않음
- 종료 명령은 restore 완료 후 process 종료
- 설정 변경은 thread-safe하게 engine으로 전달
- UI thread에서 Win32 scan 수행하지 않음

---

# 14. Phase 8 — Startup / Updater / Release

## Startup

현재 동작과 호환.

가능하면 일반 사용자 권한으로 실행.

UAC 요구 금지.

## Updater

기존 프로젝트의 release/update 정책을 조사하여 기능 동등성을 우선한다.

보안 요구:

- HTTPS
- release metadata 검증
- SHA-256
- 기존 서명 체계가 있다면 Ed25519 호환 유지
- download 후 hash/signature 검증 전 실행 금지
- temp file + atomic replace 또는 안전한 installer handoff

## CI

권장 GitHub Actions:

```text
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --workspace
fixture parity test
cargo build --release
Windows smoke test
package
sign if configured
release artifact
```

---

# 15. 성능 검증

반드시 Python과 Rust를 동일 PC에서 비교한다.

## 측정

- cold start
- working set / private bytes
- idle CPU
- CPU wake-up
- active CPU
- process thread 수
- 광고 생성 → action latency
- KakaoTalk 미실행 상태 idle footprint
- KakaoTalk 실행 상태 idle footprint
- executable/package 크기

## 성공 판단

단순히 Rust가 더 빠르다는 가정을 금지한다.

실제 성공 조건:

1. 오탐 증가 없음
2. restore 신뢰성 저하 없음
3. idle CPU/wake-up 감소
4. RAM 감소
5. 시작 시간 개선 또는 동등
6. 유지보수 가능한 구조 확보

---

# 16. 테스트 계획

## Unit

- classification
- signal weights/conditions
- aggressive mode
- WindowIdentity
- action planning
- restore eligibility
- config parsing

## Fixture

Python golden fixture 전체.

## Integration

가짜 Win32 adapter를 만들어:

```text
scan → classify → mutate → restore
```

전체 흐름 테스트.

## Windows smoke

테스트용 window에만:

- enumerate
- rect
- hide/show
- set position
- restore

실행.

## 실사용 manual matrix

- Windows 10
- Windows 11
- 100/125/150% DPI
- multi-monitor 가능하면 포함
- KakaoTalk 최신 버전
- 재로그인
- 재시작
- popup
- minimize/restore
- aggressive on/off

---

# 17. 금지사항

에이전트는 다음을 하지 않는다.

- Python 코드를 먼저 삭제
- fixture를 삭제해서 Rust test를 통과시키기
- 오탐을 줄이기 위해 모든 popup을 무시
- 성능을 위해 restore snapshot 제거
- HWND만으로 영구 identity 판단
- callback에서 직접 복잡한 mutation 수행
- busy loop 도입
- 관리자 권한 요구
- hosts/DNS/network blocking 방식으로 기능 변경
- injection/hooking DLL을 KakaoTalk process 안에 삽입
- KakaoTalk binary patch
- anti-cheat/보안 우회성 코드 도입
- 불필요한 C/C++ 혼합
- unsafe를 application 전역에 확산
- 실측 없이 기존 polling fallback 제거

---

# 18. Definition of Done

Rust 전환 완료는 다음 조건을 모두 만족해야 한다.

- [x] Python 주요 golden fixture와 Rust 결과 일치 (`tests/golden_parity.rs` 10/10 PASS)
- [x] 광고 hide/resize 정상 (`scan_parity.rs`, `restore_regression.rs`)
- [x] popup 처리 정상 (AdFitWebView dismiss, depth 2, guard verification)
- [x] ON/OFF restore 정상 (disable 시 SW_SHOW 및 zero-size 복원)
- [x] 종료 restore 정상 (stopping 시 restore_all)
- [x] HWND reuse 안전성 검증 (다른 PID/class 재사용 시 restore 차단)
- [x] Event-driven path 정상 (SetWinEventHook + drain)
- [x] reconciliation 정상 (유휴 200ms / 활성 50ms 폴백)
- [x] tray/settings 정상 (Win32 네이티브 트레이 + 설정 저장 실패 시 롤백)
- [x] startup 정상 (HKCU Run 레지스트리 동기화)
- [x] updater 정상 (`kakao-updater` 전용 헬퍼 + Ed25519/SHA-256 검증 + 재시작/롤백)
- [x] Windows release package 생성 (`dist/KakaoTalkLayoutAdBlocker_v11.exe`)
- [x] cargo fmt/clippy/test 통과 (workspace 전체 테스트 통과)
- [x] Python 대비 benchmark 작성 (`BENCHMARK.md`)
- [x] 기존 사용자 설정 migration 검증 (`config_migration.rs` legacy fixture 호환)
- [x] Rust 버전을 기본 release로 사용할 수 있음 (`v11.1.0`+)

### Manual verification pending (실기기 수동 확인)
- [ ] 실제 최신 카카오톡 환경에서 1시간 이상 장시간 구동 시 오탐/누수 확인 (`docs/SOAK_TEST.md`)

최종 단계에서만 Python 구현을 `legacy/`로 이동하거나 제거한다.

---

# 19. 권장 구현 커밋 단위

큰 원샷 리라이트를 금지한다.

권장 순서:

```text
1. test: freeze python decision fixtures
2. chore: add rust workspace
3. feat(core): add window models and config
4. feat(core): port signal evaluator
5. feat(core): port decision engine
6. test(core): pass golden fixtures
7. feat(win32): add safe windows-rs wrapper
8. feat(app): add process/window scanner
9. feat(app): add shadow mode
10. feat(win32): add SetWinEventHook
11. feat(app): add event coalescing and reconciliation
12. feat(actions): enable hide + restore
13. feat(actions): enable resize + restore
14. feat(actions): enable popup close fallback chain
15. feat(ui): add tray/settings
16. feat: startup
17. feat: updater
18. perf: benchmark and tune
19. release: switch default implementation to Rust
20. chore: archive/remove Python legacy
```

각 커밋은 가능한 범위에서 independently testable해야 한다.

---

# 20. 첫 구현 작업

## 완료됨 (다시 하지 말 것)

브랜치 `feat/rust-native-migration`:

1. ~~현재 저장소와 테스트 전체 조사~~
2. ~~Python 엔진의 decision/action 입출력 목록 작성~~
3. ~~기존 window dump fixture inventory 작성~~
4. ~~부족한 fixture 추가~~ (기존 10개 dump로 golden 고정)
5. ~~deterministic golden JSON exporter 구현~~ → `kakao_adblocker/dev/export_fixture_decisions.py`
6. ~~`rust/Cargo.toml` workspace 생성~~
7. ~~`kakao-core` 생성~~
8. ~~WindowSnapshot/WindowIdentity/AdDecision/ActionPlan 타입 구현~~
9. ~~첫 fixture를 Rust unit test로 통과~~
10. ~~fixture 전체 golden parity~~ → `rust/crates/kakao-core/tests/golden_parity.rs` 10/10

커밋: `a244e13` 계획 문서, `6953f65` Python golden, `ae7eb03` kakao-core.

## 이어서 할 일

**Win32 mutation 구현은 golden parity가 유지되는 동안만 시작한다.**

남은 작업의 실행 계약(파일 경로, API, 검증 명령, 금지사항)은 다음 문서가 정본이다.

- [docs/superpowers/plans/2026-09-02-rust-native-remaining.md](docs/superpowers/plans/2026-09-02-rust-native-remaining.md)

시작점: Phase 2 `kakao-win32`. `--apply` / 실제 Hide는 Phase 3 shadow 이후.
