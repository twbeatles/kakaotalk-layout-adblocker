# Implementation Plan: 002-audit-2026-09-24-remediation

## 기술 접근

| 요구 | 변경 위치 | 접근 |
|---|---|---|
| FR-001 | `kakao-app/src/config/storage.rs` `load_json_value` | `text.strip_prefix('\u{feff}')` 후 파싱 |
| FR-002, PR-007, PR-009 | `kakao-app/src/engine/schedule.rs`(신규, 순수 함수), `worker.rs`, `kakao-win32/src/process.rs` `PidWatch` | 스캔 간격 결정을 순수 함수로 분리해 단위 테스트한다. 부재 시 200ms → 1s(5초 경과) → 2s(30초 경과). 생존 시 전체 재동기화는 30s. liveness는 `SYNCHRONIZE` 핸들 + `WaitForSingleObject(0)` |
| FR-003 | `Win32Api::is_hung_app_window`(신규), `engine/apply.rs` precheck, `engine/restore.rs`, `lib.rs` bounded join | `IsHungAppWindow` 가드. 메인 스레드는 `JoinHandle::is_finished` 폴링으로 3초까지 기다린다 |
| FR-004 | `Win32Api::has_visible_style`(신규, `GWL_STYLE & WS_VISIBLE`), `restore.rs` | 판정 교체. `restore_all`은 `top_level` 우선으로 정렬. Fake에 opt-in 조상 가시성 모드 추가 |
| FR-005, FR-006, FR-009 | `engine/flags.rs` | `last_error` 출처 구분(`restore_error_active`). `set_restore_error`/`clear_restore_error`, `startup_warning_summary()` 우선순위 선택 |
| FR-007 | `startup_repair.rs` | `sync_run_on_startup_from_registry(&mut settings) -> bool`, 변경 시 저장 + `flags.startup` 반영 |
| FR-008 | `kakao-updater/src/lib.rs`, `main.rs`, `kakao-app/src/updater/staging.rs` | 실패 시 `relaunch_previous`. `WaitTimeout`은 제외(부모가 살아 있음). app 측 `launch_helper` 실패 시 staged 파일 삭제 |
| FR-009 | `engine/worker.rs` | tick을 `catch_unwind`로 감싼다. 연속 panic 시 백오프하고 `last_error`에 표시한다. 루프 밖 panic은 drop guard가 `last_error`에 기록 |
| FR-010 | `storage.rs` `atomic_write` | `file.sync_all()` |
| FR-011 | `worker.rs` | OFF 전환 시 `hook = None; hooked_pids.clear()` |
| FR-012 | `lib.rs` 트레이 커맨드 | 저장 직전 `load_settings`로 재로드하고 한 필드만 변경. 재로드가 경고를 내면(파손 등) 메모리 사본 사용 |
| PR-001 | `schedule.rs` `reconcile_interval`, `settings.rs` `idle_backoff_max_ms` | 조용한 시간 10초 이상이면 10초마다 2배, 상한 `idle_backoff_max_ms`. hook 없으면 백오프 없음 |
| PR-002 | `Win32Api::enum_descendant_windows`(신규, 원시 `EnumChildWindows`), `graph_build.rs` | top-level당 1회 열거 → parent map → 기존과 같은 순서의 직계 자식 트리 |
| PR-003 | 측정 후 결정 | tick 비용에서 `EnumWindows`가 차지하는 비중을 측정 |
| PR-004 | `engine/apply.rs` | `snapshots.contains_key(identity)`면 캡처 생략 |
| PR-005 | `kakao-core/src/layout.rs`, `signals.rs` | 재귀 진입점에서 1회 계산 후 내부 헬퍼로 전달. `ascii_words`는 짧은 토큰이 있을 때만 계산. 공개 함수 시그니처 유지 |
| PR-006 | `event_hook.rs` `pump_for`, `worker.rs` | 병합 대기를 `thread::sleep`에서 `pump_for(burst)`로 교체 |
| PR-008 | `rust/Cargo.toml` | `[profile.release] lto = true, codegen-units = 1, strip = true` |

## 위험과 대응

- **PR-002 순서 동일성:** `EnumChildWindows`는 깊이 우선 z-order로 열거하므로 부모별 직계 자식의 상대 순서가 per-node 열거와 같다. Fake는 BFS지만 부모별 순서는 보존된다. `scan_parity`, `graph_direct_children`로 확인한다.
- **PR-001 복구 지연:** 훅이 이벤트를 놓쳐도 최대 1초 안에 재확인한다. 폴링 모드(훅 설치 실패)에서는 백오프하지 않는다.
- **FR-003 `IsHungAppWindow`:** 5초간 무응답이어야 참이 되므로 막 멈춘 스레드는 못 걸러낸다. 이 경우는 bounded join이 막는다.
- **FR-009 catch_unwind:** panic 중에 캐시가 부분 갱신될 수 있지만 메모리 안전성은 유지된다. 스냅샷을 잃는 재기동 방식보다 복원 보장이 강하다.

## 검증

- 단위: `schedule.rs`, `flags.rs`, storage BOM, updater relaunch-on-failure
- 통합(FakeWin32): 복원 판정, 복원 순서, hung 가드, 스냅샷 재캡처 횟수, panic 내성
- 회귀: 기존 parity 스위트
- 실측: 재빌드 후 60초 유휴 CPU/메모리, EXE 크기(사용자 확인 후 실행 중인 인스턴스를 교체해 측정)
