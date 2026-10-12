# Mutation Report — 2026-10-12

## 1. 도구 실행 (cargo-mutants 27.1.0)

- 설치: `cargo install cargo-mutants --locked` → 27.1.0 (레지스트리 도달 가능 확인)
- 범위: `kakao-core/src/layout.rs` + `signals.rs` (판정 핵심), 테스트는
  `-p kakao-core`로 제한, mutant당 timeout 180s
- 제약: scratch-copy 모드는 `rust/` 바깥의 `tests/fixtures`를 복사하지 않아
  baseline 자체가 실패하므로, `--in-place`로 실행
- 1차 결과: 244 mutants — 192 caught, **42 missed**, 10 unviable, 0 timeout (20분)
- 2차 결과: 244 mutants — 233 caught, **2 missed**, 9 unviable, 0 timeout (22분).
  생존 2건은 `planned_view_resize` 높이 비교(`height < 1` → `==`/`<=`)로,
  너비 측만 covered된 탓. 높이 경계(부모 32/31/30 → 1/0/-1) 추가로 직접 주입
  검증을 마쳤고(KILLED 확인 후 원복), 3차 인증 실행에서 전원 검출을 확정한다.
- 3차 인증 결과: 244 mutants — **235 caught, 0 missed**, 9 unviable, 0 timeout (23분).
  당초 동등 변이로 분류했던 `- 1`→`/ 1` 4건도 depth-9 테스트가 검출했다
  (0에서의 거동이 다르므로 동등이 아니었음 — 테스트가 분류 오류까지 바로잡음).
  실질 생존 변이 0건으로 종결.

## 2. 1차 실행 트리아지 (244 mutants: 192 caught, 42 missed, 10 unviable)

생존자 42건은 모두 "테스트 부재"가 아니라 "경계 미표현" 유형이었다. 대응:

| 생존 군집 | 건수 | 대응 테스트 |
|---|---|---|
| `contains_any_ad_token` `&&`→`||` (23, 24행) | 2 | `long_ascii_tokens_still_match_as_substrings` ("MyAdvertisementX" 부분일치 유지) |
| `is_chrome_widget_class` →true | 1 | `chrome_widget_class_gate_is_not_universal` |
| `should_close_empty_eva_child` 전 ARM (`→false`, `&&`→`||`, `==`→`!=`, `!` 삭제) | 8 | `empty_eva_close_needs_every_arm` 전체 조합 행렬 + contract positive close e2e |
| `planned_view_resize` `< 1` 계열 | 5 | `planned_view_resize_rejects_degenerate_widths` (너비 3/2/1) |
| `popup_host_text_matches` `&&`→`||` | 1 | `empty_listed_host_token_does_not_match_everything` |
| `empty_eva_close_decision` `&&`→`||` | 3 | `empty_eva_decision_sets_signal_only_on_match` |
| 탐색 깊이 `< 0`→`==`/`<=` (132, 153, 169, 200행) | 8 | `subtree_walkers_stop_exactly_at_the_depth_limit` (깊이 8 발견 / 9 무시) |
| 탐색 `- 1`→`+ 1` (144, 165, 181, 213행) | 4 | 동상 (9-deep e2e negative `deep_signal_beyond_limits` 포함) |
| 탐색 `- 1`→`/ 1` (144, 165, 181, 213행) | 4 | **동등 변이** — 정수 `x/1 == x`로 행위 동일 (근거 아래) |
| `find_popup_matches` 가시성 `&&`→`||` | 1 | `popup_search_requires_visibility_only_when_asked` |
| `has_main_view_signature` `&&`→`||` | 1 | `main_view_signature_and_candidacy_reject_lookalikes` |
| `structural_main_window_candidate` `==`→`!=` | 1 | 동상 (부모 있는 EVA는 후보 아님) |
| 합계 (동등 4 제외 시 유효 38) | 42 | 2차 실행에서 해소 확인 예정 |`

## 3. 표적 수동 변이 검증 (직접 주입 → FAIL 확인 → 원복, 모두 실측)

| # | 변이 (파일) | 기대 검출 테스트 | 실제 결과 |
|---|---|---|---|
| M1 | `layout.rs` `height < min` → `<=` | `rule_boundaries` 경계 2건 | KILLED (2 failed) |
| M2 | `signals.rs` `!require_empty` → `!` 제거 | `f17` + oracle `negative_photo_viewer_guarded` | KILLED (양쪽 FAIL) |
| M3 | `signals.rs` `streak >= ticks` → `>` | `rule_boundaries` weak + oracle `weak_substring_two_ticks` | KILLED (양쪽 FAIL) |

원복 확인: 변이 제거 후 `git diff`에서 `layout.rs`·`signals.rs` 변경 없음,
전체 테스트 GREEN (최종 보고서 참조).

## 4. 실제 결함 (테스트가 찾아낸 제품 버그, RED → 최소 수정 → GREEN)

### FZ-01 `Rect::width/height` overflow panic (`model.rs`)
  `right - left`가 극단 좌표에서 debug panic. `layout_fuzz_invariants`가 RED로
  재현. 수정은 `saturating_sub` 2줄 (정상 좌표 동작 불변 — 전 기존 테스트 통과로
  확인). 호출처(`restore.rs`의 `> 0` 검사, 배너 비교, fake 좌표 계산)는 모두
  포화 의미를 안전 방향으로 소비한다.

### ST-01 empty-EVA close 경로 사망 (`evaluate/apply.rs`, `inspect.rs`)

`contract_oracle` positive 케이스가 RED로 확정: aggressive ON(기본값)에서
빈 EVA 자식의 Weak close는 절대 확정되지 않았다. 원인 — 한 tick 안에서
close 분기가 기록한 streak를 뒤이은 aggressive 분기가 `none`으로 덮어썼고,
진단 preview와 적용 상태가 서로 달라졌다(동일 입력에 preview weak vs 적용 miss).
**최소 수정**: `store_once` — tick당 한 창은 한 번만 기록하고, 확정(matched)은
항상 통과, miss 추적 재기록만 양보한다. 팝업 분기는 대상 집합이 겹치지 않아
불변. 검증: oracle positive close PASS + `golden_parity` 무변경 PASS(기존 golden
기대값이 이중 기록에 의존하지 않음) + 전체 157 GREEN.

## 5. 계층 계약 명확화 (변이가 아닌 설계 확인)

- 퍼즈가 지적한 "외부 PID 자손 조준"은 결함이 아님:
  코어 판정은 렌더러 PID 자손을 지명할 수 있으나(실제 카카오톡 CEF 구조와 동일),
  실제 Win32 변경은 엔진 `precheck`의 PID 게이트가 차단한다.
  `f20_engine_never_mutates_foreign_pid_descendants`가 엔진 계층을 고정한다.
- `cargo-mutants` scratch 모드 한계(상위 `tests/` 미복사)는 toolchains/CI 문제가
  아니라 도구 동작이므로, Nightly 잡에서는 `--in-place` + fixture 포함 구성을
  사용할 것 (본 실행이 그 구성의 선행 실측).
