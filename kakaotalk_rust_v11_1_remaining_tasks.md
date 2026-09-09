# KakaoTalk PC AdBlocker Rust — v11.1.x 잔여 안정화 작업 지시서

> 대상 저장소: `twbeatles/kakaotalk-pc-adblock-rust`
> 기준 버전: `v11.1.0`
> 기준 main 확인 커밋: `021084913557d96a54aa0fe705863a656f88f51c`
> 목적: Rust 네이티브 전환은 완료된 것으로 간주하고, 남은 기능 미스매치·회귀 위험·문서 정합성·릴리스 완성도를 정리한다.
> 작업 성격: 신규 아키텍처 개발이 아니라 **v11.1.x 안정화 / hardening / release polish**.

---

## 1. 에이전트 최상위 지침

먼저 다음을 읽는다.

1. `README.md`
2. `CLAUDE.md`가 있으면 해당 파일
3. `kakaotalk_rust_migration_plan.md`
4. `rust/README.md`
5. `.github/workflows/windows-ci.yml`
6. 핵심 구현 파일

```text
rust/crates/kakao-app/src/lib.rs
rust/crates/kakao-app/src/engine.rs
rust/crates/kakao-app/src/updater.rs
rust/crates/kakao-app/src/startup.rs
rust/crates/kakao-win32/src/event_hook.rs
rust/crates/kakao-win32/src/tray.rs
rust/crates/kakao-win32/src/single_instance.rs
rust/crates/kakao-core/tests/golden_parity.rs
```

현재 Rust 전환 자체는 완료된 상태다. 다음은 다시 하지 않는다.

- Rust workspace 재설계
- `core / win32 / app` crate 재분리
- Python 메인 구현 복귀
- WinEvent 구조 전면 재작성
- tray UI를 Tauri/Electron으로 교체
- golden fixture 체계 재설계
- 기존 동작과 무관한 대규모 리팩터링

이번 작업은 남은 리스크를 최소 변경으로 닫는 것이 목적이다.

---

## 2. 완료된 것으로 간주할 항목

- [x] 순수 Rust native binary
- [x] `kakao-core`
- [x] `kakao-win32`
- [x] `kakao-app`
- [x] `windows-rs`
- [x] Python golden parity
- [x] Window dump / diagnostics
- [x] `--shadow`
- [x] `--apply`
- [x] `SetWinEventHook`
- [x] low-frequency reconciliation
- [x] KakaoTalk PID 변경 burst scan
- [x] Hide / Resize / popup Close / zero-size fallback
- [x] HWND/PID/class identity 재확인
- [x] disable 및 종료 시 restore
- [x] Named Mutex single-instance
- [x] Win32 native tray
- [x] 시작프로그램 등록
- [x] Ed25519 manifest verification
- [x] SHA-256 artifact verification 함수
- [x] Windows GitHub Actions
- [x] release EXE packaging
- [x] packaged self-check
- [x] `v11.1.0` GitHub Release
- [x] legacy Python reference 보존

---

## 3. 이번 작업 우선순위

```text
P0  자동 업데이트 실제 적용/재시작 완성
P0  restore 동작 회귀 테스트 강화
P1  README와 실제 구현 정합성 수정
P1  migration plan 상태 최신화
P1  기존 설정 migration 검증
P1  compatibility matrix 정리
P2  Python vs Rust 실측 benchmark 작성
P2  최종 soak/manual test checklist
```

---

# 4. P0 — 자동 업데이트 실제 기능 완성

현재 updater에는 이미 다음이 있다.

```text
update.json 다운로드
→ JSON parse
→ Ed25519 서명 검증
→ version/tag/artifact URL 검증
→ artifact URL pinning
→ size 검증
→ SHA-256 검증 함수
→ download_and_verify()
```

하지만 tray의 `업데이트 확인`은 사실상 `check_for_update()` 후 로그 출력까지다.

즉 실제 동작은 아직:

```text
확인 O
서명 검증 O
다운로드 검증 함수 O
다운로드 적용 X
현재 EXE 교체 X
재실행 X
```

수준이다.

## 목표

tray에서 `업데이트 확인` 실행 시:

```text
1. 새 버전 확인
2. 서명 검증
3. 새 EXE 임시 다운로드
4. SHA-256 / size 검증
5. 안전한 updater helper 실행
6. 현재 앱 정상 종료
7. 기존 EXE 교체
8. 새 버전 재실행
9. 임시 파일 정리
```

까지 연결한다.

---

## 5. Updater 구현 원칙

금지:

- 서명 검증 전 EXE 실행
- SHA 검증 전 기존 EXE 교체
- arbitrary artifact URL 허용
- GitHub 외 임의 URL 허용
- 관리자 권한 요구
- PowerShell ExecutionPolicy 변경
- 실행 중인 자기 EXE 직접 overwrite
- 실패 시 기존 정상 EXE 손상
- updater에 Python/Node runtime 추가

기존 보안 정책을 유지한다.

- Ed25519
- SHA-256
- pinned GitHub release URL
- downgrade 방지
- size 제한
- HTTPS

---

## 6. 권장 updater 구조

별도 작은 Rust helper binary를 권장한다.

```text
rust/crates/kakao-updater/
```

예시:

```text
kakao-updater.exe
    --pid <current_pid>
    --current <current_exe>
    --replacement <verified_new_exe>
```

helper 동작:

```text
현재 app PID 종료 대기
→ current EXE 존재 확인
→ replacement 존재 확인
→ current.exe → current.exe.old
→ replacement → current.exe
→ 새 current.exe 실행
→ 성공 시 .old 정리
```

교체 실패 시:

```text
current.exe.old → current.exe
```

rollback을 시도한다.

가능하면 최종 EXE를 계속 단일 파일로 배포하기 위해 helper를 메인 바이너리에 embed 후 `%TEMP%`로 추출하는 방식을 우선 검토한다.

복잡성이 과하면 helper 동봉 방식도 허용한다.

---

## 7. updater 사용자 경험

최신 버전:

```text
현재 최신 버전입니다.
```

새 버전:

```text
새 버전 vX.Y.Z가 있습니다.
업데이트 후 프로그램이 재시작됩니다.
```

가능하면 사용자 확인 후 적용한다.

현재 tray 구조와 잘 맞는 native `MessageBox` 또는 기존 알림 방식을 사용한다.

오류는 silent하게 삼키지 않는다.

최소한:

- 로그
- MessageBox 또는 알림

중 하나로 사용자가 원인을 알 수 있어야 한다.

---

## 8. updater 테스트

반드시 테스트를 추가한다.

### Unit

- valid signed manifest
- invalid signature
- wrong hash
- wrong size
- invalid tag
- URL mismatch
- downgrade
- same version
- malformed version

### Helper integration

실제 앱 대신 temporary dummy files로:

```text
old.exe
new.exe
→ helper
→ old path에 new content
```

를 검증한다.

추가:

- replacement 없음
- current 없음
- rename 실패
- rollback
- relaunch 실패

가능한 범위까지 Windows CI에 포함한다.

---

# 9. P0 — Restore 회귀 테스트 강화

최근 다음 버그가 수정된 상태다.

```text
GetWindowRect = screen coordinates
child SetWindowPos = parent-relative coordinates

→ OnlineMainView를 그대로 복원
→ child가 화면 밖으로 이동
→ KakaoTalk black screen
```

현재 해결 방향은 정상적인 view resize는 restore snapshot을 저장하지 않고:

```text
Hide
Zero-size
```

등 실제 복원이 필요한 경우만 snapshot을 저장하는 것이다.

이 동작을 테스트로 고정한다.

---

## 10. Restore regression test 필수 항목

### Normal view resize

`OnlineMainView` 크기 변경은 restore snapshot에 저장하지 않아야 한다.

종료 시 screen-coordinate rect를 child `SetWindowPos`에 replay하지 않아야 한다.

### Hidden ad

```text
visible ad
→ SW_HIDE
→ snapshot 존재
→ disable
→ SW_SHOW
```

### Zero-size fallback popup

```text
popup
→ 0x0
→ snapshot 존재
→ disable/stop
→ original rect restore
```

### HWND reuse

동일 HWND 숫자가 다른 PID 또는 class로 재사용되면 restore하지 않는다.

### KakaoTalk restart

```text
hide window
→ KakaoTalk 종료
→ 새 PID로 재실행
→ stale snapshot 무시
```

### Disable during event processing

disable 이후 새 Hide/Resize/Close mutation이 발생하지 않아야 한다.

### Stop during event processing

`stopping` 이후 새 mutation을 시작하지 않아야 한다.

---

## 11. Restore 정책 유지

현재 의도를 유지한다.

```text
normal main view resize
    → KakaoTalk 자체 layout에 맡김

hidden / zero-sized ad
    → blocker가 직접 restore
```

이 정책을 바꾸려면 `ScreenToClient`, `MapWindowPoints` 등의 좌표 변환을 먼저 올바르게 설계해야 한다.

이번 안정화 작업에서는 필요성이 명확하지 않으면 변경하지 않는다.

---

# 12. P1 — README 정합성 수정

## Restore 설명

“숨겨지거나 리사이즈된 모든 창을 100% 원복” 같은 절대 표현은 피한다.

권장 의미:

> 프로그램이 직접 숨기거나 zero-size 처리한 광고 창은 저장된 상태를 기준으로 안전하게 복원하며, 일반 메인 뷰 크기 조정은 카카오톡 자체 레이아웃 갱신 동작을 따릅니다.

## Updater 설명

실제 apply/relaunch 완료 전에는 “원클릭 다운로드·교체·재시작”이라고 단정하지 않는다.

P0 구현 후 실제 동작과 README를 맞춘다.

---

# 13. P1 — Migration Plan 상태 최신화

`kakaotalk_rust_migration_plan.md`의 오래된 `[ ]` 체크 상태를 실제 코드에 맞춰 갱신한다.

예:

```text
[x] Python golden parity
[x] 광고 hide/resize
[x] popup
[x] ON/OFF restore
[x] 종료 restore
[x] HWND reuse guard
[x] Event-driven path
[x] reconciliation
[x] tray
[x] startup
[ ] updater apply/relaunch
[x] Windows release package
[x] cargo fmt/clippy/test
[ ] benchmark
[ ] 기존 settings 실사용 migration 검증
[x] Rust 기본 release
```

manual 확인이 필요한 항목은 별도 “Manual verification pending” 섹션으로 둔다.

---

# 14. P1 — 기존 설정 migration 검증

legacy Python v11 설정 파일 형태를 fixture로 추가한다.

```text
tests/fixtures/config/
```

검증:

```text
Rust load
→ field 유지
→ save
→ reload
```

필수 확인:

- enabled
- aggressive mode
- startup
- legacy scan interval
- rules overrides
- unknown field
- malformed file fallback
- partial configuration

설정을 읽지 못했다고 기존 파일을 무조건 덮어쓰지 않는다.

---

# 15. P1 — Compatibility Matrix

새 문서:

```text
docs/COMPATIBILITY.md
```

필수 시나리오:

- KakaoTalk 미실행 상태에서 blocker 시작
- blocker 실행 후 KakaoTalk 시작
- KakaoTalk 실행 중 blocker 시작
- KakaoTalk 종료/재실행
- 친구 목록
- 채팅 목록
- 채팅방
- 잠금 화면
- 광고 popup
- aggressive ON/OFF
- blocker ON/OFF
- blocker 종료/재실행
- Windows login startup
- 2중 실행
- update check

가능하면:

- Windows 10
- Windows 11
- DPI 100/125/150%
- multi-monitor

를 기록한다.

테스트하지 않은 환경을 PASS로 쓰지 않는다.

---

# 16. P2 — BENCHMARK.md 작성

Rust 전환 성능 효과를 실측한다.

```text
legacy Python v11
vs
Rust v11.1.x
```

가능하면 동일 PC, 동일 Windows, 동일 KakaoTalk 상태.

측정:

- EXE size
- cold start
- tray ready까지 시간
- first scan까지 시간
- Working Set
- Private Bytes
- KakaoTalk 미실행 idle memory
- KakaoTalk 실행 idle memory
- 5~10분 idle CPU
- 가능하면 WinEvent → mutation latency

문서 예:

```markdown
# Benchmark

## Environment
- CPU:
- RAM:
- Windows:
- KakaoTalk:
- Python version:
- Rust version:

## Results

| Metric | Python v11 | Rust v11.1.x | Difference |
|---|---:|---:|---:|
| EXE size | | | |
| Cold start | | | |
| Idle working set | | | |
| Kakao running working set | | | |
| Idle CPU 10 min | | | |
| Median detection latency | | | |
```

README의 수치는 이 실측 결과와 맞춘다.

실측 근거가 없다면 `3~8MB`, `0%`, `90% 감소` 같은 절대 수치는 완화한다.

---

# 17. P2 — Soak Test

최종 release 전 장시간 실사용 테스트.

```text
blocker 실행
→ KakaoTalk 계속 사용
→ 여러 채팅방
→ 최소화/복원
→ 친구 탭 이동
→ popup
→ KakaoTalk 종료/재실행
→ blocker ON/OFF 반복
```

관찰:

- 오탐
- black screen
- 창 위치 이상
- 정상 UI hide
- CPU runaway
- event loop 폭주
- log spam
- restore failure 증가
- crash/panic

---

# 18. Event Hook 검증

현재 hybrid 구조는 유지한다.

```text
SetWinEventHook
+
event coalescing
+
low-frequency reconciliation
+
PID change burst
```

확인:

- KakaoTalk 미실행 때 busy loop 없음
- event 없을 때 CPU 지속 상승 없음
- 이벤트 폭주 시 과도한 repeated scan 없음
- reconciliation이 event 누락 복구
- 새 KakaoTalk PID 정상 인식

전면 재설계하지 않는다.

---

# 19. CI 강화

기존 Windows CI는 유지한다.

```text
Python legacy:
  pyright
  pytest

Rust:
  cargo fmt
  cargo clippy
  cargo test --workspace
  cargo build --release
  packaged self-check
```

추가:

- updater helper build
- updater helper tests
- config migration fixture tests
- restore regression tests

---

# 20. Legacy Python 처리

`legacy/python-v11/`은 삭제하지 않는다.

목적:

- golden fixture 생성
- 행동 비교
- 회귀 조사
- 향후 KakaoTalk UI 변경 대응

README에 이것이 runtime dependency가 아니라 reference implementation임을 명확히 한다.

---

# 21. 코드 품질 기준

각 단계 완료 후:

```bash
cd rust
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --workspace
cargo build --release -p kakao-app
```

Updater crate가 추가되면:

```bash
cargo build --release -p kakao-updater
```

도 포함한다.

legacy Python의 pyright/pytest도 계속 통과해야 한다.

---

# 22. 권장 커밋 순서

```text
1. test: add restore regression cases
2. test: add legacy settings migration fixtures
3. feat(updater): add replacement helper
4. test(updater): cover replace and rollback
5. feat(updater): connect download verification to tray flow
6. feat(updater): launch helper and relaunch app
7. test(updater): add end-to-end temporary update scenario
8. docs: align restore and updater behavior in README
9. docs: update migration completion checklist
10. docs: add compatibility matrix
11. docs: add measured benchmark
12. ci: validate updater helper and regressions
13. release: prepare v11.1.x stabilization release
```

한 커밋에 updater와 unrelated engine refactor를 섞지 않는다.

---

# 23. Release Gate

## Automated

- [ ] `cargo fmt` PASS
- [ ] `cargo clippy -D warnings` PASS
- [ ] `cargo test --workspace` PASS
- [ ] Python legacy pytest PASS
- [ ] Pyright PASS
- [ ] release build PASS
- [ ] packaged self-check PASS
- [ ] golden parity PASS
- [ ] restore regression PASS
- [ ] updater manifest verification PASS
- [ ] updater replacement test PASS
- [ ] config migration fixture PASS

## Manual

- [ ] KakaoTalk 최신 버전
- [ ] 광고 hide 정상
- [ ] main view resize 정상
- [ ] popup 처리 정상
- [ ] aggressive ON/OFF
- [ ] blocker ON/OFF
- [ ] 종료 후 KakaoTalk 정상
- [ ] KakaoTalk 재실행 정상
- [ ] black screen 재발 없음
- [ ] single instance 정상
- [x] startup 정상 (v11.1.3: 로그온 트레이 대기/재시도, stale Run 복구, StartupApproved)
- [ ] update check 정상
- [ ] 이전 release → 새 release 실제 update 성공
- [ ] update 후 자동 재실행
- [ ] update 실패 시 기존 EXE 보존

---

# 24. Definition of Done

- [ ] updater가 실제 다운로드까지 수행
- [ ] artifact의 signature/URL/size/SHA 검증
- [ ] 안전한 self-update helper 존재
- [ ] 교체 실패 시 rollback
- [ ] update 후 자동 relaunch
- [ ] restore black-screen regression test
- [ ] hidden/zero-size restore test
- [ ] stale HWND restore 방어 test
- [ ] legacy Python v11 설정 fixture 호환
- [ ] README restore 정책 정합성
- [ ] README updater 기능 정합성
- [ ] migration plan 상태 최신화
- [ ] `docs/COMPATIBILITY.md`
- [ ] `BENCHMARK.md`
- [ ] Windows CI 전체 PASS
- [ ] 실제 KakaoTalk soak test PASS
- [ ] 안정화 release 생성 가능

---

# 25. 에이전트 첫 실행 순서

1. 현재 `main` 최신 상태 확인
2. `git status` clean 여부 확인
3. README / migration plan / updater / engine / CI 읽기
4. `cargo test --workspace` baseline 실행
5. restore 테스트 inventory
6. 최근 `OnlineMainView` restore bug regression test부터 추가
7. legacy Python settings fixture 추가
8. updater call graph 확인:
   - `check_for_update`
   - `download_and_verify`
   - tray `CheckUpdate`
9. `download_and_verify()` 미사용 여부 재검증
10. 최소 self-update helper 설계
11. helper test-first 구현
12. app → helper handoff 연결
13. rollback 검증
14. README 수정
15. compatibility/benchmark 문서 작성
16. CI equivalent 명령 전체 실행
17. 변경을 기능별 작은 커밋으로 정리

---

# 26. 최종 목표

이번 작업의 성공 기준은 새 기능 수가 아니다.

```text
현재 잘 동작하는 Rust architecture
+
업데이트 실제 완성
+
restore 회귀 방지
+
문서와 구현 일치
+
실측 근거
+
릴리스 검증 체계
```

이미 완료된 Rust 전환을 다시 흔들지 않는다.

**최소한의 변경으로 v11.1.x를 신뢰할 수 있는 안정화 버전으로 만드는 것이 이번 작업의 최우선 목표다.**
