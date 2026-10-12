# Test Baseline — 2026-10-12

> Phase 0 산출물. 변경 전 기준선을 동결한다. 아래 수치는 본 문서 작성 과정에서
> 직접 실행한 결과이며, 기존 문서의 PASS를 전용한 것이 아니다.

## 1. 리비전

- `git status --short`: `KAKAO_ADBLOCK_TEST_HARDENING_AGENT_PLAN.md`,
  `kakaotalk-pc-adblock-py.spec` 2개 untracked 외 clean
- `HEAD`: `5a2c25d` (v11.1.6, 2026-10-04) — 계획서 기준 커밋과 일치
- Toolchain: `cargo 1.99.0`, `rustc 1.99.0`
- 실행 환경: Windows 로컬 체크아웃, `D:\twbeatles-repos\kakaotalk-pc-adblock-py\rust`

## 2. 기준선 실행 결과

| Command | Result | Evidence |
|---|---|---|
| `cargo fmt --all -- --check` | PASS (exit 0) | 출력 없음 |
| `cargo clippy --all-targets --all-features -- -D warnings` | PASS (exit 0) | 경고 0 |
| `cargo test --workspace` | PASS — **114 passed / 0 failed / 2 ignored** (25 binaries) | exit 0 |
| `cargo build --release` | NOT RUN (시간 절약; CI 게이트에서 커버) | — |
| packaged `--self-check` | NOT RUN (`dist` 산출물 없음, 실기 환경 아님) | — |
| 실 카카오톡 광고 차단 E2E | NOT RUN (상용 카카오톡 미설치) | — |

### Ignored 2건 (CI 제외 사유)

| Test | 사유 |
|---|---|
| `graph_build_parity::single_pass_graph_matches_the_per_node_algorithm_on_the_live_desktop` | 실제 Windows 데스크톱 열거 필요 (실기 전용) |
| `perf_probe::reconciliation_cost_breakdown` | 성능 측정용 수동 프로브 |

## 3. 리스크 맵 (requirement → 코드 → 기존 테스트 → 신규 테스트)

| Requirement (P0) | 판별 코드 | 적용 코드 | 기존 테스트 | 신규 테스트 | Gate |
|---|---|---|---|---|---|
| 정상 창 무조작 (FP=0) | `signals.rs` host guard, `layout.rs` 토큰 경계 | `engine/apply.rs` precheck | golden 10쌍, guarded/non-adfit 음성 | `contract_oracle.rs` negatives, `take_api_calls` 음성 단언 | PR 필수 |
| 광고 차단 (FN) | `signals.rs`, `evaluate/*` | `engine/apply.rs` | golden parity, scan_parity | `contract_oracle.rs` positives | PR 필수 |
| weak 2틱 확인 | `update_candidate_state` | — | `evaluate_weak_parity.rs` | `rule_boundaries.rs` ticks 1/2/3 | PR 필수 |
| WM_CLOSE 4경로 | `apply.rs` close 분기 | `apply_evaluation` | 없음 (GAP — fake이 항상 DeliveredButAlive) | `fake.rs` CloseBehavior + `close_behavior_transitions.rs` | PR 필수 |
| OFF/종료 복원 | `engine/restore.rs` | `drain_restore_all(_final)` | `restore_regression.rs` 16 tests | 전이 테스트 F05/F06/F08, healthy-first 순서 | PR 필수 |
| PID 교체/HWND 재사용 | `identity_matches` | precheck + restore guard | `restore_regression.rs` 2 tests | F09 재생성 변형 | PR 필수 |
| panic/응답없음 격리 | worker panic guard, hung skip | `restore_snapshot` hung skip | hung 1 test | F14 panic 후 tick 복구, F16 기록 | PR 필수 |
| 실기 광고 호환성 | — | — | 없음 (GAP) | `COMPATIBILITY_EVIDENCE.md`에 NOT RUN 기록 | 수동 승인 |
| 변이 탐지력 | — | — | 없음 (GAP) | `MUTATION_REPORT.md` (표적 3건 실측 + 도구 미실행 명시) | Nightly/수동 |

## 4. 기존 테스트 assertion 매핑 (파일명 존재가 아닌 실제 단언 기준)

- `golden_parity.rs`: 10개 fixture × Rust 평가 == Python golden (회귀 동등성, 독립 정답 아님)
- `apply_path_parity.rs`: apply 전용 경로 actions/state가 전체 평가와 바이트 동일
- `evaluate_weak_parity.rs`: substring weak 신호 1틱 미적용 → 2틱 적용 → 텍스트 변경 시 해제
- `restore_regression.rs`: 스냅샷 1회 캡처, OFF 복원, 2-miss 복원, PID/class 가드, 20틱 유지,
  실패 유지·재시도·backoff 상한·gauge 의미, hung 스킵, 조상숨김 오판 방지, last_error 정리
- `scan_parity.rs`: fake 그래프 == dump 평가, owned 팝업 hide·복원, view resize NOMOVE
- `kakao-win32 smoke.rs`: `own_hidden_window_roundtrip` 1건
- GAP: WM_CLOSE 파괴/실패/timeout 분기, OFF 중 API 0건 호출의 직접 관측, 경계값ponsored,
  tokenless 옵션, 순서 결정성, 설정 오류 계약 — 본 hardening에서 보강
