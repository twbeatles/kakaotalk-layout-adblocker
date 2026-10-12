# Live Compatibility Evidence — 2026-10-12

> 상태: **SUPPORTED (spot)** — 아래 실측 세션에서 광고 관측·차단·복원을 확인.
> 원본 덤프는 PII 정책에 따라 커밋하지 않고 파기했으며, 본 문서에는
> 확정 절차서이며, `NOT OBSERVED`를 `PASS`로 기재하는 것을 금지한다.

## 실기 세션 절차 (허가된 테스트 계정·전용 환경에서만)

1. 환경 기록: Windows 빌드, x64/ARM64, DPI, 모니터 구성, 카카오톡 exe 버전,
   차단기 버전, 테스트 시각.
2. `--shadow`로 먼저 동작 확인 후, 차단기 미실행 baseline에서 광고/정상 UI를
   수동 분류하고 스크린샷 취득(민감정보 제거).
3. `--dump-tree`, 상황별 `--dump-tree-series` 획득 후 title/text·사용자명·대화명
   익명화. 익명화가 시그니처를 깨지 않았는지 점검.
4. 사람이 판정한 기대 행동을 기록하고 시뮬레이션 action과 비교.
5. 동의 하에 실제 blocking ON: 광고 부재·정상 기능·레이아웃 검사, ON/OFF 왕복.
   실패 시 즉시 OFF/프로세스 종료, 필요 시 카카오톡 재시작.
6. 릴리스 비교는 동일 머신·동일 카카오톡 버전에서 이전↔새 버전 교차 진행.

## 체크리스트 (증거 첨부 시 PASS/FAIL/NOT OBSERVED/NOT RUN 기재)

- 친구·채팅·잠금·프로필·사진/동영상·설정·개별 채팅방·이모티콘/웹뷰·로그인/로그아웃 정상 작동
- 하단 배너, 시작 시 광고, 독립 `AdFitWebView` 팝업의 노출/미노출
- 최소화/최대화/리사이즈/다중 모니터/125~150% DPI/트레이 복귀
- 카카오톡 종료·재실행, blocker ON/OFF·종료, 비정상 종료 뒤 복구
- self-check, 설정 경로, 트레이 아이콘, 시작프로그램 등록, 업데이트 롤백
- 1시간 soak (`docs/SOAK_TEST.md` 기반) + 실측 로그

## 실측 세션 2026-10-12 (로컬 실기)

- 환경: Windows 11 Pro build 26300, x64, AMD Radeon 760M, 카카오톡 26.8.2.5324 (PID 16132),
  차단기 v11.1.7-debug (릴리스 동등 로직).
- `--self-check --json`: core ok, process 1건, warnings 0.
- `--shadow` (변경 없음): main 1, candidates 2, `hide=[394604]` (Strong).
- 광고 관측 **YES**: owned 팝업형 — `EVA_Window_Dblclk`(owner=메인, 하단 스트립) +
  `Chrome_WidgetWin_0/1` + 광고 텍스트 + `Chrome Legacy Window` exact.
  기존 `owned_popup_legacy_ad` fixture와 동형 (신규 클래스 없음).
- 차단 ON (`--apply`, 약 9초): 덤프상 광고 `visible True → False`.
  메인·`OnlineMainView`·채팅/친구 뷰 무조작 (가시성 유지).
- 종료 복원: 워커 정상 종료 후 덤프상 광고 `visible False → True` (복원 확인).
- 정상 UI: 사진/미디어 뷰어류 팝업 없음 (미노출 항목은 NOT OBSERVED로 별도 기재).

## 버전별 판정표

| KakaoTalk | Windows/DPI | 광고 관측 | 정상 UI | 복원 | 판정 | 증거 |
|---|---|---|---|---|---|---|
| 26.8.2.5324 | Win11 26300/x64 | YES (하단 owned 팝업) | 유지 (메인·뷰 무조작) | ON→종료 복원 확인 | SUPPORTED (spot) | 본 문서 실측 세션 |

분류 정의: `SUPPORTED`(해당 버전/환경 실측 완료), `FIXTURE_ONLY`(본 문서 상태),
`UNVERIFIED`(구조 차이 감지·미확인), `KNOWN_BROKEN`(차단·오탐 확인됨).
