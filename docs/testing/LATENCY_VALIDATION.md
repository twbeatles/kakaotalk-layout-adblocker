# 광고 노출 지연 검증

2026-10-15. 새 런타임의 실제 KakaoTalk 화면·CPU·플랫폼 측정: **NOT RUN**.
테스트 소유 off-screen Win32 창과 fake runtime 결과를 실화면 결과로 대체하지 않는다.

## 자동 검증

```powershell
cd rust
cargo test --workspace --locked --offline
cargo clippy --workspace --all-targets --all-features --locked --offline -- -D warnings
cargo fmt --all -- --check
```

- f21: snapshot/hide 선행, 자식 원래 가시성 보존.
- native_close_latency: 별도 UI 스레드의 WndProc가 300ms 대기할 때 이미 숨겨진 창을 관측. 자체 off-screen 창만 사용.
- real_worker_off_*: 실제 worker 루프의 OFF 실패 후 자동 retry/reset/wake. fake API/runtime 사용.
- worker_recovers_hook_*: 같은 PID에서 두 번 실패 후 세 번째 설치 성공.
- first_event_scan_*: 첫 tick 뒤에만 대기, 추가 이벤트 없는 late text도 후속 탐지.
- series_records_actual_*: frame capture_us 및 실제 monotonic elapsed_ms 검증. 수집이 요청 기간보다 느리면 한 frame도 정상.
- readiness_failure_*: 테스트 EXE/자식의 즉시 종료·준비 timeout rollback 및 성공 확인.

## 전용 실환경 절차

1. Windows 빌드·아키텍처·DPI·모니터·KakaoTalk/차단기 버전과 설정을 기록한다.
2. 허가된 테스트 계정·전용 환경을 사용한다. 실제 대화 데이터 변경을 피한다.
3. log_level=DEBUG, low_latency_mode=true로 설정하고 앱 재시작. 측정 뒤 INFO로 복귀한다.
4. 120fps 이상 영상으로 첫 광고 프레임부터 사라짐까지 센다. kakao_latency의 scan_us/event age/graph_us/evaluate_us/apply_restore_us/total_us/first hide를 비교한다. 메시지 대기와 화면 합성은 별도 비용이다.
5. blocker 60초 대기 후 카카오톡 시작, 반대 시작 순서, 30초 idle 후 탭·잠금·최소화 복귀, OFF→ON을 각각 30회 이상 측정한다. owned exact·독립 팝업·Weak 신호를 분리한다.
6. true/false를 동일 머신에서 비교하고 p50/p95/max·부재/생존 중 CPU를 기록한다. 이전 BENCHMARK 수치를 새 모드 실적으로 쓰지 않는다.
7. 필요 시 `--dump-tree-series --dump-series-duration-ms 2000 --dump-series-interval-ms 10 --dump-dir <전용폴더>`로 신호 생성 시점을 확인한다. elapsed_ms는 실제 시간, interval_ms는 요청 간격이다. dump 제목은 공유 전 익명화한다.
8. OFF/공격 OFF/종료/강제 종료 뒤 UI를 확인한다. 최종 실패 시 카카오톡을 재시작한다. 이전 HWND를 영속 저장해 무조건 복원하지 않는다.
9. Windows 10/11 x64·가능 시 ARM64·100/125/150%/혼합 DPI·1시간 soak를 수행한다. 채팅·사진/미디어·선물/웹뷰는 PASS/FAIL/NOT OBSERVED/NOT RUN을 구분한다.

| 환경/시나리오 | 횟수 | p50 | p95 | max | CPU | 정상 UI/복원 | 판정 |
|---|---|---|---|---|---|---|---|
| cold/warm, owned/popup, true/false | — | — | — | — | — | — | NOT RUN |

0프레임 노출을 보장하지 않는다. warm Strong p95 50ms 등은 합의할 목표 후보이며 달성치가 아니다.

## 업데이트 준비 확인

새 헬퍼와 앱을 함께 배치한다. 서명/해시 검증은 유지한다. 앱은 트레이 루프·워커 준비 후 helper 전용 파일에 자기 PID로 응답한다. 60초 내 준비 실패/즉시 종료면 소유 child를 종료·회수하고 rollback한다. 응답 뒤 250ms 생존 확인은 장기 정상 동작 보장이 아니다. 구 헬퍼에는 protocol이 없어 새 EXE 두 개가 필요하다.
