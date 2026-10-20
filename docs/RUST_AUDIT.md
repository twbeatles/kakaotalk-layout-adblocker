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
`cargo audit`을 재실행한다. `ureq`의 TLS(rustls 기본 피처)처럼 간접 의존성이면
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

- **2026-10-20**: Edition 2024 마이그레이션 완료. 워크스페이스 `edition = "2024"` + `rust-version = "1.88"` 선언(4개 크레이트 상속). `unsafe_op_in_unsafe_fn` 대응(Win32 호출 19곳 내부 `unsafe {}` 블록, `remove_var`/`set_var` SAFETY 주석), clippy `collapsible_if` 17곳 let-chain 병합, rustfmt 2024 스타일 적용. 직접 의존성 14종은 메이저 포함 최신이라 변경 없고, transitive 패치 3건(`cc→1.9.0`, `find-msvc-tools→0.1.15`, `webpki-roots→1.0.10`)만 `Cargo.lock` 갱신. `cargo audit` 0 취약점(168 크레이트), `cargo machete` 이상 없음, `test` 170 passed / 0 failed, `clippy -D warnings`·`fmt --check`·릴리스 빌드 통과.

- **2026-10-14**: 의존성 전면 업그레이드. major 5건(`base64→0.23`,
  `sha2→0.11`, `ed25519-dalek→3`, `windows→0.62`, `ureq 2→3`) + 호환 31건.
  `ureq 3`는 `updater/http.rs` 어댑터(`Config::builder`, `into_body()` 경유)로
  대응, 타임아웃·상태코드 동작 불변. `cargo audit` 0 취약점,
  `cargo machete` 이상 없음, `test`/`clippy`/`fmt`/`self-check(strict)` 통과.

- **2026-09-25**: `rustls 0.23.43` 취약점 1건(`RUSTSEC-2026-0285`, medium,
  `ureq 2` 경유) → `0.23.45`로 업데이트. 미사용 선언 3건 제거
  (`kakao-app`의 `crossbeam-channel`, `kakao-core`·`kakao-win32`의
  `thiserror`). `startup.rs`(`from_raw_parts`), `tray/host.rs`(`&*host`)에
  `SAFETY` 주석 추가. CI에 `Audit`·`Machete` 스텝 신설.
