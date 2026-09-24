# Tasks: 002-audit-2026-09-24-remediation

## Phase 1 — Immediate

- [x] T001 [FR-001] `load_json_value` BOM 제거 + `config_migration.rs` 테스트 3건(BOM settings 보존, BOM rules 보존, BOM + 깨진 JSON은 heal)
- [x] T002 [FR-003] `Win32Api::is_hung_app_window` 추가(Real: `IsHungAppWindow`, Fake: 설정 가능한 집합)
- [x] T003 [FR-003] apply precheck·restore에서 hung 창 건너뛰기(실패로 기록, 스냅샷 보존) + 회귀 테스트
- [x] T004 [FR-003] `lib.rs` bounded join(3초) 헬퍼 + 단위 테스트
- [x] T005 [FR-002, PR-007] `engine/schedule.rs` `pid_scan_due` 순수 함수 + 단위 테스트, worker 연결(부재 시 대기 상한 1초)

## Phase 2 — Stability

- [x] T006 [FR-004] `Win32Api::has_visible_style` 추가, 복원 판정 교체, `restore_all` top-level 우선 정렬, Fake opt-in 조상 가시성 + 회귀 테스트
- [x] T007 [FR-005] `SharedFlags` 복원 오류 출처 구분, 게이지 0 복귀 시 해제 + 회귀 테스트
- [x] T008 [FR-006] 시작 경고 우선순위 선택 → `last_error` + 단위 테스트
- [x] T009 [FR-007] Run 레지스트리 → `run_on_startup` 동기화(순수 판정 함수 + 테스트)
- [x] T010 [FR-008] 업데이트 헬퍼 실패 시 이전 EXE 재실행, staged 정리 + 테스트
- [x] T011 [FR-009] 워커 tick `catch_unwind` + 루프 drop guard + 테스트
- [x] T012 [FR-010] `atomic_write` `sync_all`
- [x] T013 [FR-011] OFF 시 훅 해제 / ON 시 재설치
- [x] T014 [FR-012] 트레이 토글 저장 시 디스크 설정 재로드 후 해당 필드만 변경 + 테스트

## Phase 3 — Structural / Performance

- [x] T015 [PR-001] `idle_backoff_max_ms` 설정 + `reconcile_interval` 순수 함수 + 테스트, worker 연결(hook 없으면 백오프 없음)
- [x] T016 [PR-002] `Win32Api::enum_descendant_windows` + `build_graph` O(N) 재작성, parity 확인
- [x] T017 [PR-004] 기존 스냅샷이 있으면 재캡처 생략 + 호출 수 테스트
- [x] T018 [PR-005] 토큰/needle lowercase 1회 계산, 자식 목록 복제 제거
- [x] T019 [PR-006] `EventHook::pump_for`로 이벤트 병합 대기
- [x] T020 [PR-009] `PidWatch` 핸들 기반 liveness
- [x] T021 [PR-008] `[profile.release]` 추가, 릴리스 빌드 크기 확인 → 4,510,720 → 4,261,376 bytes(-5.5%). 크기 대부분은 의존성(TLS, env-filter)에서 나옴
- [x] T022 [PR-003] `EnumWindows` 비중 측정 후 P4 결정 기록 → **보류**: `perf_probe` 실측 결과 `EnumWindows` 순회는 build_graph 307µs 중 45µs(15%). 절감 효과는 tick당 45µs 이하인데, 캐시 무효화 누락 시 주 광고 호스트(owned top-level popup)를 놓칠 위험이 있음

## Phase 4 — Docs & Verification

- [x] T023 [DR-003] `restore.rs` 주석 수정
- [x] T024 fmt/clippy/test + pytest(실행 가능 범위)
- [x] T025 [DR-001] 실측(사용자 확인 후) → README/BENCHMARK 갱신 → 유휴 60초 CPU 406.2ms(0.677%) → 93.8ms(0.156%), 측정 후 dist EXE 원래 인자로 재실행, 사용자 설정 불변 확인
- [x] T026 [DR-002] CLAUDE.md, README 설정 표(`idle_backoff_max_ms`), CHANGELOG `Unreleased`, `PROJECT_AUDIT.md` 조치 현황
