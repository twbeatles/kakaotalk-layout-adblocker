# Feature Spec: 2026-09-24 감사 지적 사항 개선

- **Feature ID:** 002-audit-2026-09-24-remediation
- **Branch:** `fix/audit-2026-09-24-remediation`
- **근거 문서:** `PROJECT_AUDIT.md` (2026-09-24, v11.1.4)
- **상태:** Implemented (PR-003/P4는 측정 근거로 보류)

## 배경

2026-09-24 감사에서 기능 이슈 5건(ISSUE-001~005), 기능 공백 6건(§5), 성능 개선 10건(§4A P1~P10), 문서 불일치 7건(§6 D1~D7)이 보고되었다. 이 기능은 그 전부를 해소하거나, 해소하지 않을 경우 근거를 남긴다.

## 불변 조건 (Non-goals / Constraints)

- 광고 판정 알고리즘(CLAUDE.md "광고차단 알고리즘 고정 규칙")은 바꾸지 않는다. 성능 변경은 스케줄링, 수집, 할당 계층에 한정한다. 골든 fixture 기대값은 수정하지 않는다.
- `apply_path_parity`, `golden_parity`, `scan_parity`, `graph_direct_children` 회귀는 변경 전후 동일하게 통과해야 한다.
- 변경 사항은 CHANGELOG에 기록한다. 구현 완료 후 사용자 요청으로 v11.1.5로 릴리스했다(버전 동기화 대상: `rust/Cargo.toml`, `config/paths.rs`, `packaging/windows_version_info.txt`, `legacy/python-v11/.../paths.py`, CHANGELOG, README 배지, CLAUDE.md).

## 사용자 결정 (2026-09-24)

| 항목 | 결정 |
|---|---|
| P2 적응형 유휴 주기 | 이벤트가 없는 상태가 약 10초 지속되면 200ms에서 1s까지 점진 백오프한다. 이벤트가 오면 즉시 복귀한다. 새 설정 `idle_backoff_max_ms`(기본 1000)을 두고, 값이 `idle_poll_interval_ms` 이하이면 백오프를 끈다 |
| 워커 panic | `panic=unwind`를 유지하고 감시한다. 워커 이상을 트레이 `last_error`에 표시하고 엔진을 계속 동작시킨다 |
| 문서 성능 수치 | 개선 후 이 PC에서 재측정한 값으로 교체한다. "ARM64 에뮬레이션 측정, 네이티브 재측정 필요"를 명시하고, 근거 없는 수치는 삭제한다 |

## 요구사항

### 기능 (ISSUE)
- **FR-001** (ISSUE-001): UTF-8 BOM이 있는 settings/rules JSON을 정상 파일로 읽어야 한다. 백업/heal은 실제 파싱 실패일 때만 일어난다.
- **FR-002** (ISSUE-002, P1): 카카오톡이 없을 때 PID 스캔은 부재 시간에 따라 백오프한다(최대 2초 간격). 카카오톡이 살아 있는 동안의 동작은 유지한다.
- **FR-003** (ISSUE-003): 종료 시 워커 join은 제한 시간(3초) 안에 끝나야 한다. 초과하면 경고 후 종료를 진행한다. 응답 없는(hung) 카카오톡 창에는 복원이나 변이 호출을 하지 않고 실패로 기록해 스냅샷을 보존한다.
- **FR-004** (ISSUE-004): 복원 성공 판정은 창 자신의 `WS_VISIBLE` 스타일로 한다. 복원은 top-level 창부터 수행한다.
- **FR-005** (ISSUE-005): 복원 실패 게이지가 0으로 돌아오면 복원에서 비롯된 `last_error`를 지운다.

### 기능 공백 (§5)
- **FR-006**: 시작 시 설정 로드 경고 중 1건을 우선순위(복구 실패 > 자동 복구 > 기타)에 따라 트레이 `last_error`에 노출한다.
- **FR-007**: 시작 시 `run_on_startup=false`인데 Run 등록이 존재하면 설정과 트레이 상태를 레지스트리에 맞춘다.
- **FR-008**: 업데이트 헬퍼가 교체에 실패하면(부모 대기 초과 제외) 이전 EXE를 다시 실행한다. staged 파일을 정리한다.
- **FR-009**: 워커 tick에서 panic이 나도 엔진 루프는 유지되고 스냅샷도 보존된다. 이 사실을 `last_error`에 표시한다. 루프 자체가 죽으면 그것도 표시한다.
- **FR-010**: `atomic_write`는 rename 전에 `sync_all`을 호출한다.
- **FR-011**: 차단 OFF 동안에는 WinEvent 훅을 해제하고, ON이 되면 다시 설치한다.
- (§5 "트레이 토글 저장이 외부 편집을 덮어씀"은 FR-012로 다룬다) **FR-012**: 트레이 토글 저장 시 디스크의 최신 설정을 다시 읽고, 해당 필드만 바꿔 저장한다.

### 성능 (§4A)
- **PR-001** (P2): 적응형 유휴 주기 (위 결정 참고). 훅이 없는 폴링 모드에서는 백오프하지 않는다.
- **PR-002** (P3): `build_graph`는 top-level마다 자손 열거 1회와 `GetParent` 1회/노드로 트리를 구성한다. 결과 그래프는 기존과 동일해야 한다.
- **PR-003** (P4): top-level 캐시는 측정 후 결정한다. 효과 대비 위험이 크면 보류하고 근거를 기록한다.
- **PR-004** (P5): 이미 스냅샷이 있는 창은 매 tick 재캡처하지 않는다.
- **PR-005** (P6): 광고 토큰과 needle의 lowercase 결과를 재귀 호출마다 다시 만들지 않는다.
- **PR-006** (P7): 이벤트 병합 대기 중에도 메시지를 펌프해, 버스트가 실제로 하나의 tick으로 합쳐지게 한다.
- **PR-007** (P8): 카카오톡이 살아 있는 동안의 전체 PID 재동기화 주기를 30초로 늘린다.
- **PR-008** (P9): `[profile.release]`에 `lto`, `codegen-units=1`, `strip`을 적용한다. panic 전략은 unwind를 유지한다.
- **PR-009** (P10): PID liveness 확인은 보관한 프로세스 핸들로 한다.

### 문서 (§6)
- **DR-001**: README/BENCHMARK의 CPU, 메모리, EXE 크기, 지연 수치를 재측정값과 환경 주석으로 교체한다. "5ms", "O(1) 캐싱" 문구는 제거한다.
- **DR-002**: CLAUDE.md "Rust 활성 런타임 동작"에 이번 변경으로 생긴 차이와 해소된 차이를 반영한다.
- **DR-003**: `restore.rs`의 잘못된 주석(D7)을 고친다.

## 수용 기준

- `cargo fmt --check`, `cargo clippy --all-targets --all-features -- -D warnings`, `cargo test --workspace`가 모두 통과한다.
- `pytest` 중 이 환경에서 실행 가능한 테스트(`cryptography` 비의존)가 통과한다.
- FR/PR마다 대응 회귀 테스트가 있다(tasks.md에 명시).
