# Contract fixtures — 독립 광고 판정 Oracle

`tests/fixtures/golden/`이 Python 시절 출력과의 **동등성**(parity)을 검증한다면,
이 디렉터리는 광고·비광고 **정답 자체**를 주장한다. 기대값은 구현 함수의 산출물을
복사한 것이 아니라, 문서화된 판정 계약(아래 근거)에서 사람이 작성했다.

- `manifest.json`: 케이스 목록. 로더(`contract_oracle.rs`)는 unknown 필드를
  거부하고, 기대값이 참조하는 HWND가 fixture에 실재하는지 검증한다.
- `positives/`: 차단되어야 하는 광고 사례.
- `negatives/`: 절대 건드리면 안 되는 정상 UI 사례. `must_not_*`는 **모든 tick**에
  대해 검사하므로, 몇 tick 뒤에 나타나는 오탐도 실패한다.
- `boundaries/`: (예약) 기하·토큰·depth 경계는 `rule_boundaries.rs`에서 실제
  기본값으로부터 직접 계산하므로 별도 덤프가 필요 없다.

## 라벨 근거

| 케이스 | 독립 근거 |
|---|---|
| `positive_legacy_exact_strong` | 정확 일치 legacy 시그니처 = Strong, 즉시 hide |
| `positive_bottom_banner_with_token` | 하단 기하 + 명시적 광고 토큰 = Strong hide |
| `positive_popup_allow_empty_host` | 빈 host 제목은 guard 통과 → dismiss(close+hide+zero-size) |
| `positive_weak_substring_two_ticks` | 부분 일치는 Weak → 1틱 pending, 2틱 hide |
| `positive_empty_eva_close_with_signal` | 빈 EVA 자식 + 메인 내 광고 신호 = Weak close (2틱 확정, e2e 최초 커버) |
| `negative_deep_signal_beyond_limits` | 탐색 깊이(8)를 초과한 신호는 도달 불가 (변이 `+1` 킬러) |
| `negative_photo_viewer_guarded` | 제목 있는 host는 guard 거부 → 손대지 않음 |
| `negative_banner_without_token` | 토큰 없는 하단 패널은 기본 차단 금지 |
| `negative_adobe_word_boundary` | 단문 `Ad`는 단어 경계 필요 → `Adobe` 불일치 |
| `negative_other_pid_window` | 카카오톡 PID 집합 밖 창은 후보 제외 |
| `negative_empty_eva_no_signal` | 광고 신호 없는 빈 EVA 자식은 닫기 금지 |

## 갱신 규칙

- 기대값 변경은 **사람의 재검토 없이 금지**한다. 구현 출력을 그대로 덮어써서
  통과시키는 행위는 이 디렉터리에서 실패로 간주한다(오라클이 golden과 분리된 이유).
- PII(채팅 내용·대화 상대·연락처)를 fixture에 넣지 않는다. 합성 fixture는
  `capture_note`에 원형을 명시한다.
