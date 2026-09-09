# Changelog

## 11.1.3 - 2026-09-09

시작프로그램에 등록되어 있어도 재부팅 후 실행되지 않던 경로를 수정한 패치입니다.

### 수정
- 로그온 직후 Explorer 트레이가 준비되기 전에 `NIM_ADD`가 실패하면 프로세스가 바로 종료되던 문제를 고쳤습니다. `Shell_TrayWnd`를 기다린 뒤 수 초에 걸쳐 재시도하고, 실패해도 메시지 루프를 유지한 채 `TaskbarCreated`/타이머로 아이콘을 다시 붙입니다.
- EXE를 다른 폴더로 옮기거나 이전 경로가 사라진 경우에도 Run 등록을 현재 실행 파일로 복구합니다. cargo 소스 실행은 기존 패키지 EXE 등록을 덮어쓰지 않습니다.
- Windows 설정/작업 관리자에서 시작 앱을 끈 상태(`StartupApproved`)를 감지하고, 시작프로그램 ON이면 다시 켭니다.
- Run 명령의 `\\?\` 경로 접두사를 제거해 부팅 시 실행이 막히지 않게 했습니다.
- 로컬 릴리스 빌드가 Visual Studio 2026 BuildTools(`vcvarsall.bat`)에서도 MSVC 링커를 찾도록 했습니다.

### 참고
- 이 버전을 한 번 실행하면 기존 시작프로그램 등록이 현재 EXE와 Windows 허용 상태에 맞춰집니다.

## 11.1.2 - 2026-09-06

감사(`PROJECT_AUDIT.md`)에서 확인된 실패 경로와 문서 불일치를 수정한 안정화 릴리스입니다.

### 수정
- Win32 자손 열거를 직계 자식 그래프로 정규화해 `popup_search_depth`가 실제 깊이를 따르게 했습니다.
- WM_CLOSE를 거부한 팝업이 숨김·표시를 반복하지 않도록, 숨긴 뒤에도 광고 시그널이 있으면 복원하지 않습니다.
- 복원 API가 한 번 실패해도 원본 스냅샷을 남겨 같은 프로세스에서 재시도합니다.
- rules/settings 필드 타입 오류가 시작 panic이나 전체 기본값 초기화를 일으키지 않습니다. 올바른 필드는 유지합니다.
- 트레이 생성 실패 시 워커를 복원·종료하고 프로세스가 mutex를 붙잡은 채 대기하지 않습니다.
- 업데이트 적용은 숨긴 창을 복원한 뒤에만 헬퍼를 실행합니다.
- 업데이트 확인은 한 번에 하나만 진행하고, 다운로드/헬퍼 경로는 실행마다 고유합니다.
- `--self-check` 보고서 쓰기 실패와 `--strict-self-check`를 종료 코드에 반영합니다. startup trace는 실제 트레이 결과를 기록합니다.

### 배포
- 권장 설치물은 `KakaoTalkLayoutAdBlocker_v11.zip`입니다. 앱 EXE와 `kakao-updater.exe`를 같은 폴더에 두어야 자동 업데이트가 됩니다.
- Windows CI와 release workflow가 헬퍼와 ZIP을 함께 올립니다.

### 기타
- Explorer `TaskbarCreated` 후 트레이 재등록, 시작프로그램 등록 누락 복구, 첫 실행 settings/rules 생성, 로그 회전, HTTP timeout.

## 11.1.1 - 2026-09-04

- 유휴 CPU 최적화 (liveness polling, zero-alloc 평가)
- 트레이 윈도우 생성 경로 수정
- 업데이트 artifact URL이 rust/legacy 저장소를 모두 허용
