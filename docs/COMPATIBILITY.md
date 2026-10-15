# Compatibility Matrix

갱신: 2026-10-15. Windows 전용. 자동 검증과 실제 화면 검증을 구분한다.

| 환경/기능 | 근거 | 현재 판정 |
|---|---|---|
| 광고 판정·정상 UI guard | oracle/golden/rule_boundaries/apply_path_parity | AUTOMATED |
| 팝업 실패·snapshot·복원 | close_restore_transitions/restore_regression | AUTOMATED |
| 느린 실제 WM_CLOSE 전 은닉 | native_close_latency의 자체 off-screen 창 | AUTOMATED (테스트 Windows) |
| OFF 회복/reset, 동일 PID hook retry | worker runtime/control tests | AUTOMATED (fake API/runtime, 실제 워커 루프) |
| 설정 BOM·파손 backup·원자 저장 | config_migration/log_rotation | AUTOMATED |
| updater 준비 실패/즉시 종료 rollback | updater_tests의 테스트 파일·자식 프로세스 | AUTOMATED |
| KakaoTalk 26.8.2.5324 / Win11 26300 x64 owned 배너 | [2026-10-12 기록](testing/COMPATIBILITY_EVIDENCE.md) | SUPPORTED (과거 spot, 새 스케줄 미측정) |
| Windows 10/ARM64, 125/150% DPI·혼합 모니터 | 새 변경의 실기 기록 없음 | NOT RUN |
| 새 변경의 노출 지연·CPU·1시간 soak | [절차](testing/LATENCY_VALIDATION.md) | NOT RUN |
| Linux/macOS | 앱은 코드 2로 fail-fast | 미지원, 이번 빌드 미검증 |

선택 low_latency_mode=true는 부재 최소 200ms, 생존 중 추가 PID 최소 2초 탐색이다. false는 이전 1~2초/30초 절전 정책이다. 첫 이벤트 스캔 후 유한 burst를 실행한다. Strong은 첫 유효 tick, Weak는 서로 다른 두 tick 확인이다. 공격 모드 OFF 복원은 두 miss tick이며 전체 OFF는 pending restore를 200ms~5초 backoff로 재시도한다.

메인 뷰 resize는 복원 snapshot 대상이 아니다. snapshot은 메모리에만 있어 강제 종료/최종 실패 뒤에는 복원을 보장하지 않는다. 필요하면 카카오톡을 재시작한다. Rust 파손 백업은 `*.broken-<epoch>`다.
