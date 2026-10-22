# Audit follow-up validation — 2026-10-23

최종 자동 검증: **2026-10-23 (Asia/Seoul)**.

대상은 `9e33a55` 이후 main에 반영한 후속 안정성 수정과 보안 패치다. 현재 릴리스 버전은 `11.1.14`이며, 앱·워크스페이스·Windows 파일 버전 동기화를 수행했다. 전용 VM 실기 항목은 아직 미실행이다.

## 구현 및 감사 항목 대응

| 감사 항목 | 구현/결정 | 검증 |
|---|---|---|
| ISSUE-001 | PreparedInstall이 설치 폴더에 새 EXE와 백업 복사를 완료하고 sync/해시 확인 후 rename. 설치 경로 직접 copy/선삭제 제거. 복원도 같은 경로 사용. | preparation/commit 경계의 child 즉시 종료, hash 실패, Windows 공유 잠금 실패, readiness 실패 rollback, explicit recovery CLI |
| ISSUE-002 | 초기 등록·TaskbarCreated·retry timer의 등록 결과를 공통 처리. 실패는 항상 재시도 예약. | 테스트 전용 Win32 창에 TaskbarCreated 전달 후 add 실패 주입. 실제 WM_TIMER만으로 성공 재시도 확인 |
| ISSUE-003 | child zero-size는 SWP_NOMOVE로 기존 상대 offset 유지. 판정 action은 그대로. | non-zero fixture OFF/종료 복원, 실제 중첩 Win32 child 좌표 roundtrip |
| 트레이 대체 제어 | `app.exe --exit`가 기존 호스트에 정상 종료 요청. CLI는 설정/워커를 새로 기동하지 않음. | 아래 격리된 수동 시나리오로 실기 확인 필요. 요청 전달 성공과 최종 복원 성공은 구분 |
| 강제 종료 후 UI snapshot | HWND를 프로세스 간 저장하지 않음. 재사용 위험 때문에 이 설계 유지. | 카카오톡 재시작 복구 제한을 README에 유지. 구현하지 않은 자동 영속 복구를 완료로 표시하지 않음 |
| helper 동기화 | 2초 protocol probe로 구 helper를 종료 전에 거절. 수동 ZIP 두 EXE 동시 설치 안내. | 실제 helper protocol CLI 테스트, MZ-only helper 거절. 서명되지 않은 별도 helper 다운로드는 추가하지 않음 |
| 설정·레지스트리 오류 | enabled/aggressive 저장 실패에 다이얼로그/상태. startup 실패 시 원래 command로 역롤백하고 결과/현재 등록 여부를 표시. StartupApproved 실패도 실패로 전파. | 저장 실패/부분 변경/역롤백 실패 주입. 사용자 HKCU를 바꾸는 테스트는 실행하지 않음 |
| 파일 복구·경합 | Windows named ConfigMutex, unique backup create_new/sync, backup 실패 시 heal/토글 저장 거절, bootstrap create_new. | thread 및 별도 두 프로세스 read-modify-write, backup 실패 경계, 기존 bootstrap 파일 보존 |
| HTTP | 최대 3회 요청, 200/500ms backoff, 요청당 기존 90초 timeout. 일시 transport/408/429/일부5xx만 재시도. signature/hash 검증은 retry 밖. | injected 503→503→성공, 404 즉시 실패, 429 3회 제한. 라이브 GitHub 장애 실험은 하지 않음 |
| 구조 | 파일 교체를 transaction 모듈로 분리. shell add와 timer 간 경계, retry 및 startup 저장 경계를 테스트 가능하게 구성. | 기존 classifier/golden 회귀 유지. 일반 리팩터링이나 광고 휴리스틱 변경 없음 |
| 문서·CI | strict/core/info·복원 한도·업데이트 교체·트레이 설명 정정. windows-ci가 build_release.ps1 -NoSign 호출. | `v11.1.13` release workflow 성공. Windows CI는 새 `RUSTSEC-2026-0382` 권고로 실패했고, v11.1.14에서 `rustls-webpki`를 수정 버전으로 갱신 |

## 자동 검증

현재 테스트/빌드의 최종 수치와 smoke 결과는 이 문서의 마지막 검증 결과 절에 기록한다.

환경: Windows, `rustc 1.99.1`, `x86_64-pc-windows-msvc`. `v11.1.13` 검증 당시 앱 의존성은 바꾸지 않았고, 후속 `v11.1.14`에서 권고된 `rustls-webpki 0.103.16`만 갱신했다. Cargo.lock의 workspace package metadata도 해당 앱 버전에 맞췄다. MSRV 1.88 실행 검증은 별도로 하지 않았다.

실행 명령:

```powershell
cd rust
cargo test --workspace --locked
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo build --release --locked -p kakao-app -p kakao-updater
```

초기 baseline에서는 `readiness_failure_rolls_back_and_success_releases_backup`가 WinError 5로 실패했다. 종료된 child의 이미지 mapping이 잠시 남아도 설치 파일을 먼저 삭제하지 않고 2초 한도에서 rename을 재시도하도록 고쳐 통과했다. 기존 기대 assertion은 실패 시 backup 보존 정책에 맞춰 수정했다.

`transaction_recovery` child 중단 시험은 부모 테스트가 만든 임시 파일에서 `process::exit`로 Drop을 건너뛴다. 실제 설치본·작업 중인 앱·카카오톡을 종료시키지 않는다. native timer/좌표 시험도 자체 생성한 숨겨진/화면 밖 테스트 창만 대상으로 한다.

## 11.1.14 릴리스 빌드 확인

`scripts/build_release.ps1 -NoSign -DistDir build/release_11_1_14`가 앱·helper·ZIP을 빌드하고 packaged strict self-check를 통과했다. CI와 같은 `CI=true` 환경이라 대화형 startup smoke는 skip됐다. 기존 `dist/` 앱 파일은 다른 프로세스가 열고 있어 덮어쓰지 않았다. 로컬 빌드는 코드 서명을 생략했다. `rustls-webpki`는 `0.103.16`으로, 나머지 의존성 버전은 그대로 유지했다.

GitHub `release` workflow run `39872680811`은 workspace 테스트, v11.1.13 빌드, 서명 매니페스트 생성, 업로드 및 공개 매니페스트 검증까지 모두 성공했다. 이후 Windows CI run `39872671703`의 Audit 단계가 `RUSTSEC-2026-0382`로 실패했고, 수정 버전은 `0.103.16`이다. v11.1.14의 release/Windows CI 실행 결과는 새 푸시 후 확인한다.

## 전용 VM 수동 절차 — 모두 NOT RUN

아래는 실제 사용 중인 PC나 대화 데이터에서 실행하지 않는다. 준비된 스냅샷 VM/전용 계정을 사용한다. 각 항목은 PASS/FAIL/NOT OBSERVED/NOT RUN과 Windows 빌드·아키텍처·카카오톡/차단기 버전·DPI를 함께 기록한다.

### 1. 다른 볼륨·중단 업데이트

1. VM C: TEMP와 D: 전용 설치 폴더를 준비하고 기존/신규 테스트 EXE의 해시를 기록한다.
2. 테스트 helper를 준비 복사 도중, 준비 완료 직후, 최종 rename 직후, readiness 직전에 각각 중단한다.
3. 설치 경로가 이전 또는 새 **완전한 EXE**를 가리키는지 해시로 확인한다. partial install이 없어야 한다.
4. rollback 대상 `.exe.old`가 있으면 companion helper `--recover --current <test app>`로 복원하고 이전 해시/실행을 확인한다. 경로 없음/쓰기 거절/공유 잠금도 검증한다.
5. 예전 helper가 만든 partial current+정상 backup과 missing current+backup을 따로 만들고 복원한다. backup 부재 시에는 파일을 바꾸지 않고 명확히 실패해야 한다.
6. 전원 차단은 VM snapshot에서만 별도 실험한다. 성공을 관측해도 모든 파일시스템의 무손실 보장으로 일반화하지 않는다.

### 2. Explorer·아이콘 없는 제어

1. 전용 VM에서 앱 하나를 실행하고 트레이 메뉴를 확인한다.
2. VM Explorer를 재시작하고 첫 NIM_ADD 실패 조건을 주입한다. 추가 Explorer 재시작 없이 2초 retry로 아이콘/메뉴가 복귀해야 한다.
3. 아이콘을 계속 사용할 수 없는 조건에서 `app.exe --exit`를 실행한다. 새 워커 없이 기존 앱 종료·복원 시도가 진행되어야 한다.
4. 요청 성공/worker 종료/실제 복원 결과를 각각 확인한다. 종료 중 target hang이면 기존 3초 대기 한도와 카카오톡 재시작 안내를 확인한다.

### 3. 핵심 기능·플랫폼·soak

1. Windows 10/11, x64/ARM64, DPI 100/125/150%, 혼합 모니터/음수 좌표를 준비한다. 한글·공백 경로, C:/다른 볼륨, 일반 계정으로 실행한다.
2. baseline 광고를 실제 관측·분류한 뒤 shadow→ON→OFF→ON→종료로 왕복한다. 하단 owned 배너와 독립 AdFit popup을 구분한다.
3. 친구/채팅/잠금/로그인·로그아웃/미디어/이모티콘/파일전송 UI를 확인한다. 미노출 팝업은 NOT OBSERVED로 기록한다.
4. 카카오톡 종료·재실행 및 시작프로그램 등록·재로그온은 전용 HKCU에서 확인한다. 등록 실패/설정 쓰기 실패 시 명령·설정·메뉴를 대조한다.
5. 최소 1시간 반복 후 CPU/메모리/핸들/스냅샷/로그 회전·지연 p50/p95/max를 기록한다. 첫 프레임 광고 무노출를 보장했다고 표시하지 않는다.
6. 실덤프는 공유 전에 익명화하고 원본 개인정보 자료를 저장소에 커밋하지 않는다.

## 최종 검증 결과 — 2026-10-23

- `cargo test --workspace --locked`: **196 passed / 0 failed / 2 ignored**. 별도 child 프로세스의 중복 요약은 이 수치에서 제외했다. 최종 전체 suite에는 OFF/final/stale 좌표 보강과 실제 트레이 타이머 회귀가 포함된다.
- ignored: `single_pass_graph_matches_the_per_node_algorithm_on_the_live_desktop`, `reconciliation_cost_breakdown`. 실제 사용자 데스크톱/성능 probe를 이번 수정 검증으로 대신 실행하지 않았다.
- `cargo fmt --all -- --check`: 통과.
- `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings`: 통과.
- `cargo build --release --locked -p kakao-app -p kakao-updater`: 통과.
- 릴리스 앱을 테스트 임시 폴더의 `KakaoTalkLayoutAdBlocker_v11.exe`로 복사한 뒤, 별도 APPDATA로 strict self-check/report 실행: **process exit 0, summary exit 0, core ok, warnings []**. worker/UI 실행 없이 HKCU는 접근 probe만 수행했다.
- 릴리스 helper `--protocol-version`: **kakao-updater-protocol-2**.
- 릴리스 helper `--recover --current <임시 partial app> --no-relaunch`: 이전 backup 내용으로 복구 및 backup 보존 확인. 실제 설치 앱은 실행하지 않았다.
- smoke assertion은 모두 통과했으나 첫 임시 디렉터리 정리에서 종료된 EXE mapping의 일시 잠금(WinError 5)이 있었다. 잠금 해제 후 테스트가 생성한 파일만 명시적으로 정리했다. 이를 기능 검증 실패나 무시된 production 오류로 처리하지 않았다.
- Cargo.toml/Cargo.lock, core classifier 및 golden fixture 변경 없음. 신규 외부 의존성 없음. GitHub CI 실행·배포·실사용자 데이터 변경은 수행하지 않았다.
- `git diff --check`: 통과. core classifier/golden 및 의존성 파일의 diff가 없는 것도 확인했다.
