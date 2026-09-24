# Benchmark: KakaoTalk Layout AdBlocker (Rust)

> 이 문서의 수치는 모두 아래 환경에서 **실제로 측정한 값**입니다. 이전 판에 있던 Python v11 비교표와 Rust 수치(EXE 1.8MB, 메모리 3~7MB, 유휴 CPU 0.0%, 반응 5ms 미만, LTO 빌드)는 재현되지 않았고 측정 근거도 남아 있지 않아 삭제했습니다(`PROJECT_AUDIT.md` 2026-09-24 §6).

---

## 1. 측정 환경

| 항목 | 값 |
|---|---|
| 측정일 | 2026-09-24 |
| OS | Windows 11 Home 10.0.26200 |
| CPU | Snapdragon X Plus (ARM64, 8코어). **x64 EXE를 에뮬레이션으로 실행** |
| 시스템 부하 | 프로세스 약 396개, 데스크톱 top-level 창 약 340개 |
| 카카오톡 | 26.8.1.5315, 실행 중, 사용자 조작 없음(30초간 WinEvent 0건) |
| 카카오톡 창 트리 | top-level 34개, 전체 노드 67개 |
| 빌드 | `cargo build --release` (x86_64-pc-windows-msvc) |

> ⚠️ ARM64에서의 x64 에뮬레이션은 CPU 시간과 Working Set(번역 캐시)을 부풀릴 수 있습니다. 네이티브 x64 PC에서는 더 낮을 것으로 예상하지만 아직 측정하지 않았습니다.

---

## 2. 유휴 CPU / 메모리 (카카오톡 실행 중)

방법: `--startup-launch --minimized`로 실행하고 35초 워밍업한 뒤, 60초 동안 `TotalProcessorTime` 증가량을 측정했습니다. 두 빌드를 같은 세션에서 연달아 측정했습니다.

| 빌드 | 60초 CPU 시간 | 코어 1개 대비 | 8코어 PC 전체 대비 | Working Set | Private |
|---|---:|---:|---:|---:|---:|
| v11.1.4 (감사 전) | 406.2 ms | 0.677% | 약 0.085% | 24.5 MB | 11.0 MB |
| 감사 반영 빌드 | 93.8 ms | **0.156%** | **약 0.020%** | 21.1 MB | 12.0 MB |

감소 요인:
- 창 이벤트가 없으면 재확인 주기를 200ms에서 최대 1s로 늘립니다(`idle_backoff_max_ms`).
- 창 트리를 top-level마다 한 번만 열거합니다.
- 카카오톡이 살아 있는 동안 전체 프로세스 재스캔 주기를 5s에서 30s로 늘렸습니다.
- 이미 숨긴 창의 스냅샷을 다시 읽지 않습니다.

## 3. 구성 요소별 비용 (마이크로벤치)

`cargo test --release -p kakao-app --test perf_probe -- --ignored --nocapture` (읽기 전용, 300회 평균)

| 단계 | 1회 비용 |
|---|---:|
| `EnumWindows` + PID 필터 (데스크톱 약 340개 창) | 45 µs |
| 카카오톡 창 트리 수집 `build_graph` (위 포함, 67노드) | 307 µs |
| 판정 `evaluate_graph_for_apply` | 44 µs |
| 전체 프로세스 스캔 (Toolhelp, 약 396개) | 6.5 ms |

## 4. 카카오톡 미실행 시 (산출값)

프로세스 스캔 1회 6.5 ms 기준:

| 빌드 | 스캔 간격 | 코어 1개 대비 |
|---|---|---:|
| v11.1.4 | 200 ms 고정 | 약 3.2% |
| 감사 반영 빌드 | 200 ms → (5초 후) 1 s → (30초 후) 2 s | 약 0.3% (30초 이후) |

이 표는 종단 간 측정이 아니라 스캔 단가와 코드상 주기로 계산한 값입니다.

## 5. 기타

| 항목 | 값 |
|---|---|
| `KakaoTalkLayoutAdBlocker_v11.exe` 크기 | 4,261,376 bytes (LTO·`codegen-units=1`·`strip` 적용 전 4,510,720) |
| `kakao-updater.exe` 크기 | 1,543,168 bytes |
| 정상 종료 소요(창 복원 포함) | 113 ms (`taskkill` → 프로세스 종료) |
| 창 이벤트 → 첫 재확인 | `burst_scan_interval_ms`(기본 20ms) 대기 + tick(약 0.4ms) |

## 6. 재측정 방법

1. 차단기를 한 번에 하나만 실행합니다(단일 인스턴스 mutex).
2. 워밍업 35초 뒤 60초 동안 PowerShell `(Get-Process -Id <pid>).TotalProcessorTime` 차이를 잽니다.
3. 구성 요소 비용은 위 `perf_probe` 명령으로 잽니다.
4. 결과에는 CPU 아키텍처(네이티브/에뮬레이션), 프로세스 수, 카카오톡 버전을 함께 적어 주세요.
