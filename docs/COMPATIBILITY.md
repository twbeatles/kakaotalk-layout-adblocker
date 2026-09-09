# Compatibility Matrix (호환성 매트릭스)

> 대상: KakaoTalk Layout AdBlocker v11.1.x (Rust Native)  
> 최종 업데이트: 2026-09-09

## 1. 운영체제 및 디스플레이 환경

| 환경 | 세부 스펙 | 지원 여부 | 검증 상태 | 비고 |
|---|---|:---:|:---:|---|
| **Windows 11** | 64-bit (x86_64, ARM64 에뮬레이션/네이티브) | **지원** | **PASS** | 개발 및 CI 메인 타깃 |
| **Windows 10** | 64-bit (1903 이상) | **지원** | **PASS** | Win32 API 및 SetWinEventHook 호환 |
| **Windows 7 / 8** | 32/64-bit | 미지원 | N/A | Windows 10 이상 필수 |
| **DPI 100% (96 DPI)** | 표준 해상도 (FHD) | **지원** | **PASS** | 레이아웃 리사이즈 공식 일치 |
| **DPI 125% / 150%** | 고해상도 (QHD/4K 스케일링) | **지원** | **PASS** | Win32 클라이언트 영역 자동 비례 계산 |
| **다중 모니터** | 주/보조 모니터 간 창 이동 | **지원** | **PASS** | SetWindowPos SWP_NOMOVE로 좌표 간섭 방지 |

---

## 2. 런타임 수명 주기 및 실행 시나리오

| 시나리오 | 동작 규격 및 기대 결과 | 검증 결과 | 관련 테스트 / 근거 |
|---|---|:---:|---|
| **카카오톡 미실행 상태에서 blocker 시작** | 카카오톡 프로세스를 기다리며 유휴 폴백(200ms) 슬립. CPU 0% 유지. | **PASS** | `spawn_worker`, `idle_ms` sleep |
| **blocker 실행 중 카카오톡 시작** | 프로세스 감지 후 burst scan(3회, 20ms)을 거쳐 메인 창 감지 즉시 하단 배너 제거 및 리사이즈. | **PASS** | `burst_scan`, `scan_parity.rs` |
| **카카오톡 실행 중 blocker 시작** | 시작 즉시 첫 tick에서 카카오톡 창을 스캔하여 광고 영역 즉시 은닉. | **PASS** | 워커 첫 tick warm-up |
| **카카오톡 종료 및 재실행** | 이전 PID의 스냅샷은 안전하게 무시되며, 새 PID 윈도우 그래프를 새로 빌드하여 정상 적용. | **PASS** | `kakaotalk_restart_ignores_stale_snapshots` |
| **친구 목록 탭 활성화** | `OnlineMainView` 리사이즈로 하단 배너 영역 확장, 뷰 내용 정상 노출. | **PASS** | `view_resize_keeps_child_top_left` |
| **채팅 목록 탭 활성화** | 메인 뷰 리사이즈 유지, 채팅 목록 스크롤 및 목록 표시 정상. | **PASS** | Golden parity (`normal_main_window.json`) |
| **개별 채팅방 (독립 창)** | 메인 창 가드에 의해 개별 대화방 창은 변경하지 않음 (대화방 UI 정상). | **PASS** | Top-level main window guard |
| **잠금 화면 모드 (`LockModeView`)** | 비밀번호 입력 잠금 뷰 리사이즈 공식(높이 전체) 적용, 하단 공백 제거. | **PASS** | `layout.rs` LockModeView formula |
| **독립 팝업 광고 (`AdFitWebView`)** | 팝업 호스트 가드 검증 후 `WM_CLOSE` 전송. 실패 시 fallback zero-size/hide 적용. | **PASS** | `popup_adfit_webview.json` |
| **공격 모드 (Aggressive Mode) ON/OFF** | 토글 즉시 공격 모드 숨김 창 복원 및 재스캔 적용. | **PASS** | `SharedFlags`, `restore_stale_hidden` |
| **차단기 끄기 (Blocker OFF)** | 숨김 및 zero-size 처리된 광고 창을 원래 상태로 `SW_SHOW` 복원. 메인 뷰는 카카오톡 자체 레이아웃에 위임. | **PASS** | `hidden_ad_restored_on_disable` |
| **차단기 종료 (Exit)** | 모든 변경 창 안전 복원 후 트레이 및 백그라운드 워커 종료. | **PASS** | `restore_all`, `stopping` flag |
| **Windows 로그인 시작프로그램** | HKCU Run에 `--startup-launch --minimized`로 등록. 로그온 시 `Shell_TrayWnd` 대기·NIM_ADD 재시도·`TaskbarCreated` 재등록. 낡은/누락 EXE 경로와 `StartupApproved` 꺼짐은 다음 실행 때 복구. | **PASS** | `kakao_win32::startup`, `tray.rs`, `kakao-app/src/startup.rs` |
| **중복 실행 방지 (Single Instance)** | 커널 Named Mutex로 2번째 실행 시 기존 인스턴스 보존 및 콘솔/대화상자 안내 후 즉시 종료(0). | **PASS** | `single_instance.rs`, `lib.rs` |
| **자동 업데이트 확인 및 적용** | Ed25519 서명 검증 -> 임시 다운로드 -> `kakao-updater` 헬퍼 실행 -> 안전 교체 및 자동 재시작. | **PASS** | `updater_tests.rs`, `updater.rs` |

---

## 3. 예외 및 안전 복원 방어 매트릭스

| 장애 / 예외 상황 | 방어 동작 | 검증 결과 |
|---|---|:---:|
| **HWND 재사용 (다른 PID)** | 프로세스 종료 후 같은 HWND 번호가 타 앱에 할당되어도 WindowIdentity(hwnd+pid+class) 불일치로 복원 스킵. | **PASS** (`hwnd_reuse_different_pid_or_class_skips_restore`) |
| **광고 텍스트 변경 (Stale Hide)** | 광고 창으로 숨겼던 창의 텍스트가 일반 대화 등으로 변경되면 2틱 후 자동으로 `SW_SHOW` 복원. | **PASS** (`stale_hide_restored_after_two_miss_ticks`) |
| **설정 JSON 손상/파손** | 파손된 파일은 `*.broken-YYYYMMDD-HHMMSS` 백업 후 기본값 JSON으로 자동 치유(self-heal). | **PASS** (`malformed_json_creates_broken_backup_and_heals`) |
| **업데이트 파일 위변조 / 해시 불일치** | SHA-256 불일치 또는 Ed25519 서명 위조 시 다운로드 파일 즉시 폐기 및 기존 파일 100% 보존. | **PASS** (`updater.rs`) |
| **업데이트 중 교체 실패** | 헬퍼가 `.exe.old` 백업을 즉시 롤백하여 프로그램 손상 방지. | **PASS** (`updater_tests.rs`) |
