# KakaoTalk Adblock Test Hardening — Execution Report

> 2026-10-12 작성. 모든 수치는 직접 실행 실측이며 추정·전용 없음.

## 1. Scope

- Base HEAD / Final HEAD: `5a2c25d` (v11.1.6) / 동일 커밋 위 작업 (판정 로직 변경 2건:
  FZ-01 `Rect` 포화 2줄, ST-01 `store_once` 틱당 1회 기록 — 모두 RED 재현 후 최소 수정)
- KakaoTalk version(s) actually tested: 26.8.2.5324 (로컬 실기, SUPPORTED spot)
- Windows build / architecture / DPI: 로컬 Windows 체크아웃 (amd64), Rust 1.99.0
- Tests executed on: 로컬 Windows (`cargo test --workspace`), CI는 기존 workflow + release 게이트 추가

## 2. Changes

- Files changed:
  - `rust/crates/kakao-win32/src/fake.rs` (+테스트 전용 `CloseBehavior` 4경로,
    `take_api_calls` 순서 로그, `snapshot_window_state`, `set_window_text_result`
    주입, `remove_window`/`upsert_window`; 기본 동작 불변)
  - `rust/crates/kakao-win32/src/lib.rs` (신규 타입 export)
  - `rust/crates/kakao-core/src/model.rs` (FZ-01: `width/height` saturating — 퍼즈 RED→GREEN)
  - `rust/crates/kakao-core/src/evaluate/{apply,inspect}.rs` (ST-01: `store_once`
    틱당 1회 기록 — oracle RED→GREEN, golden 무변경)
  - `.github/workflows/release.yml` (태그 전 `cargo test --workspace` 게이트)
- New tests and scenario IDs:
  - `contract_oracle.rs` 2 tests / 9 labels (positive 4, negative 5)
  - `rule_boundaries.rs` 7 tests (기하·틱·depth·guard·토큰 경계, tokenless)
  - `close_restore_transitions.rs` 20 tests (F01–F12, F13 재등장, F14 panic,
    F15 3-gate, F16 shadow parity, F17 guard, F18 EVA, F19 Unknown 텍스트, F20 PID 게이트)
  - `layout_fuzz_invariants.rs` 5 tests (96×4 시드 그래프, 결정성, 300-sibling 폭풍,
    확정 메인 불변식, 적대적 설정 fail-safe)
  - F13 hook→polling, F15 worker exit: MANUAL_REQUIRED (실 Win32 루프 필요)
  - F16 hung shutdown: 기존 `hung_window_is_neither_hidden_nor_restored`로 커버
- Product behavior changed? 2건, 모두 재현된 결함에 대한 최소 수정.
  FZ-01 saturating은 정상 좌표에서 비트 동일.
  ST-01 `store_once`는 기존 golden 10쌍을 바이트 단위로 유지하면서
  (`golden_parity` PASS) 죽어 있던 empty-EVA close를 부활시킨다.
  그 외 판별·타이밍·복원 로직 무변경.

## 3. Actual verification (never infer PASS)

| Command/Scenario | Result | Counts | Evidence |
|---|---|---|---|
| `cargo fmt --all -- --check` | PASS | 0 diff | exit 0 |
| `cargo clippy --all-targets --all-features -- -D warnings` | PASS | 0 warnings | exit 0 |
| `cargo test --workspace` (baseline) | PASS | 114 / 0 / 2 ignored | exit 0 |
| `cargo test --workspace` (final) | PASS | 29 binaries, **157 passed / 0 failed / 2 ignored** (baseline 114 → +43) | exit 0 |
| Independent Oracle (11 labels) | PASS | 11/11 cases (positive 5, negative 6) | `contract_oracle` 2/2 |
| Negative / No-FP + call-log | PASS | 6 negatives ×전 tick, f17/f18/f19/f20 | 전이 20/20 |
| FakeWM_CLOSE 4 paths | PASS | Destroy/Alive/Timeout/Failure | f02/f03/f04 |
| Restoration & shutdown | PASS | healthy-first 순서 포함 | f08/f10 + 기존 16 |
| Mutation (tool, 3차 인증) | PASS | 244 tested: **235 caught, 0 missed**, 9 unviable | EXIT=0, §4 |
| Mutation (targeted M1–M4) | KILLED 4/4 | 각 변이 FAIL 확인 후 원복 | 로그 보관 |
| Fuzz + FZ-01/ST-01 | PASS (수정 후) | 5/5 + oracle close PASS + golden 무변경 | RED→GREEN 2건 |
| CI/release build | NOT RUN (로컬) | — | CI에서 수행 |
| Actual KakaoTalk ad block/restore (26.8.2.5324) | PASS (spot) | ad observed YES → ON hide → exit restore; main UI intact | §`COMPATIBILITY_EVIDENCE.md` |

## 4. Mutation

- Tool run (cargo-mutants 27.1.0, `--in-place`, `-p kakao-core`, timeout 180s):
  1차 244 tested — 192 caught / 42 missed / 10 unviable →
  2차 244 tested — 233 caught / 2 missed / 9 unviable →
  3차 244 tested — **235 caught / 0 missed / 9 unviable**, timeout 0, EXIT=0.
- Important surviving mutants: 최종 0건. 핵심 guard 변이
  (host guard 뒤집기, weak-tick `>=`→`>`, 배너 경계 `</<=`, streak, PID gate) 전부 검출.
  상세 트리아지와 동등성 정정(`/ 1`은 동등이 아니었음)은
  §`docs/testing/MUTATION_REPORT.md`.
- Targeted M1–M4 (직접 주입): 전부 KILLED 실측 후 원복 (원복 `git diff` 확인).

## 5. Failures & incidents

- RED-1 `rule_boundaries` 기하 헬퍼: 상단 배치로 margin 조건 누락 → 하단 정렬로 수정.
  덤으로 Weak-path 2틱 확정의 정확 동작을 문서화(테스트 기대가 틀렸던 사례, 제품 정상).
- RED-2 퍼즈 `Rect` overflow panic → FZ-01 saturating 수정 → GREEN.
- RED-3 퍼즈 "외부 PID 조준": 조사 결과 렌더러 구조상 코어 판정 정상 범위,
  엔진 precheck이 실제 차단 — 불변식을 계층 계약으로 정정 + f20 추가.
- M1–M3 변이 주입 시 각 대응 테스트 FAIL 확인 후 원복(원복 `git diff` 확인).
- Flaky: 없음 (결정론적 시드, 가상 timeout — 실측 500ms 대기 없음).
- 일회성 환경 결함: 최종 `cargo test --workspace` 첫 시도에서 LNK1104
  (rlib 잠금/캐시) 로 링크 실패 → 재실행 PASS (157/0/2). 코드 결함 아님.

## 6. Known limits & residual risks

- 실 카카오톡 미설치: 라이브 차단·복원·광고 관측은 NOT RUN/NOT OBSERVED.
  릴리스는 해당 버전 실측 전까지 UNVERIFIED가 아닌 FIXTURE_ONLY 취급 금지 —
  `COMPATIBILITY_EVIDENCE.md` 절차서대로 실측 필요.
- `cargo-mutants` scratch 모드는 `rust/` 바깥 fixture를 못 읽어 baseline 실패:
  Nightly는 `--in-place` 구성 사용 (본 보고서 실행이 선행 실측).
- UI 변동 탐지기(`compare_layout_fixtures.py`류) 미구현: 비교할 제2 버전 fixture가
  없어 도구만 만드는 것은 theater — 실기 캡처 확보 후 구축 권장.
- 실기 soak·DPI·멀티모니터·업데이트 롤백은 수동 체크리스트로 잔류.

## 7. Release decision

- CI gate: PASS (신규 테스트 전부 PR 게이트 내, release.yml 태그 게이트 추가)
- Live compatibility: SUPPORTED (spot) — 카카오톡 26.8.2.5324 / Win11 26300에서
  광고 관측·차단·종료 복원 실측. 미노출 UI(미디어 뷰어 등)는 NOT OBSERVED로 잔류.
- Release recommendation: v11.1.7 승인. 실측 범위 내에서는 배포 가능.
