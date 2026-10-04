# Rust workspace

This is the **default** KakaoTalk Layout AdBlocker implementation.

Legacy Python v11 reference is local-only (`legacy/`, not tracked); golden fixtures stay tracked under `../tests/fixtures/`.

## Status

- `kakao-core` golden 10/10
- `kakao-win32` + tray
- `kakao-app` (`kakao-adblock-rs`): `--shadow`, `--apply`, dump, self-check, tray UI
- Release EXE: `../dist/KakaoTalkLayoutAdBlocker_v11.exe` via `../scripts/build_release.ps1 -NoSign`

## Docs

- Overview: [`../README.md`](../README.md)
- Changelog: [`../CHANGELOG.md`](../CHANGELOG.md)
- Algorithm freeze: [`../CLAUDE.md`](../CLAUDE.md)

## Commands

```powershell
cd rust
cargo test --workspace
cargo clippy --all-targets --all-features -- -D warnings
cargo fmt --all -- --check
```
