# Rust 정기 감사 (Audit) 절차

Rust 워크스페이스(`rust/`)의 코드 품질·보안 취약점·의존성 건전성을 주기적으로
점검하는 절차다. CI(`.github/workflows/windows-ci.yml`의 `rust-core` 잡)가
`fmt` / `clippy` / `audit` / `machete` / `test`를 매 push·PR마다 강제하므로,
아래 수동 점검은 **릴리스 전 + 월 1회**를 권장 기준으로 둔다.

## 1. 정적 검사 및 빌드 검증

```powershell
cd rust
cargo check --all-targets --all-features
```

## 2. 포맷팅 검증

```powershell
cargo fmt --all -- --check
```

## 3. 린트 검증 (경고=에러)

```powershell
cargo clippy --all-targets --all-features -- -D warnings
```

## 4. 의존성 보안 취약점 검증

```powershell
cargo install cargo-audit --locked   # 최초 1회
cargo audit
```

취약점이 보고되면 `cargo update -p <crate>`으로 호환 범위 안에서 올리고
`cargo audit`을 재실행한다. `ureq 2`의 `tls` 피처처럼 간접 의존성이면
`Cargo.lock`에서 유입 경로를 먼저 확인한다.

## 5. 미사용 의존성 및 `unsafe` 검증

```powershell
cargo install cargo-machete --locked  # 최초 1회
cargo machete
```

`unsafe` 현황은 코드 검색으로 파악한다. `unsafe`는 `kakao-win32`의
Win32 FFI에 집중되어 있는 것이 정상이며, 그 외 크레이트로 번지면
사유를 확인한다. 원시 포인터·핸들 역참조 지점에는 `// SAFETY:` 근거
주석을 유지한다.

## PowerShell 주의사항

cargo가 stderr로 진행 상황을 출력해서 PowerShell이 `NativeCommandError`로
감싸 보여줄 수 있다. 성공/실패 판정은 화면이 아니라 `$LASTEXITCODE`로 한다.

## 점검 이력

- **2026-09-25**: `rustls 0.23.43` 취약점 1건(`RUSTSEC-2026-0285`, medium,
  `ureq 2` 경유) → `0.23.45`로 업데이트. 미사용 선언 3건 제거
  (`kakao-app`의 `crossbeam-channel`, `kakao-core`·`kakao-win32`의
  `thiserror`). `startup.rs`(`from_raw_parts`), `tray/host.rs`(`&*host`)에
  `SAFETY` 주석 추가. CI에 `Audit`·`Machete` 스텝 신설.
