param(
    [switch]$SkipTests,
    [switch]$SkipRust,
    [switch]$FixFmt
)

$ErrorActionPreference = "Stop"
# cargo는 진행 상황을 stderr에 쓰므로, 네이티브 stderr 출력이 게이트를
# 깨뜨리지 않도록 한다. 실패 판단은 $LASTEXITCODE로만 한다 (PS 7.4+ 대응).
if (Test-Path variable:PSNativeCommandUseErrorActionPreference) {
    $PSNativeCommandUseErrorActionPreference = $false
}

$repoRoot = Resolve-Path (Join-Path $PSScriptRoot "..")
$rustDir = Join-Path $repoRoot "rust"

Push-Location $repoRoot
try {
    # 1. Rust checks matching CI (windows-ci.yml: rust-core)
    if (-not $SkipRust -and (Test-Path $rustDir)) {
        Push-Location $rustDir
        try {
            if ($FixFmt) {
                Write-Host "Formatting Rust codebase (cargo fmt --all)..."
                cargo fmt --all
            }
            Write-Host "Checking Rust formatting (cargo fmt --all -- --check)..."
            cargo fmt --all -- --check
            if ($LASTEXITCODE -ne 0) {
                throw "cargo fmt check failed. Run 'cargo fmt --all' or '.\scripts\dev_check.ps1 -FixFmt' to fix."
            }

            Write-Host "Running cargo clippy (--all-targets --all-features -- -D warnings)..."
            cargo clippy --all-targets --all-features -- -D warnings
            if ($LASTEXITCODE -ne 0) {
                throw "cargo clippy failed with exit code $LASTEXITCODE"
            }

            # windows-ci와 동일: 취약한 transitive 의존성이 있으면 여기서 먼저 실패한다.
            # DB를 못 받으면(오프라인 등) 경고만 하고 넘어간다.
            if (Get-Command cargo-audit -ErrorAction SilentlyContinue) {
                Write-Host "Running cargo audit..."
                $prevEap = $ErrorActionPreference
                $ErrorActionPreference = "Continue"
                $auditOutput = cargo audit 2>&1 | Out-String
                $auditExit = $LASTEXITCODE
                $ErrorActionPreference = $prevEap
                if ($auditExit -ne 0) {
                    if ($auditOutput -match "vulnerabilit") {
                        Write-Host $auditOutput
                        throw "cargo audit found vulnerabilities. Run 'cargo update' in rust/ and re-check."
                    }
                    Write-Warning "cargo audit could not complete (likely offline). CI will still enforce it."
                }
            } else {
                Write-Warning "cargo-audit not installed; skipping local audit (CI still enforces it). Install with 'cargo install cargo-audit --locked'."
            }

            if (Get-Command cargo-machete -ErrorAction SilentlyContinue) {
                Write-Host "Running cargo machete..."
                cargo machete
                if ($LASTEXITCODE -ne 0) {
                    throw "cargo machete found unused dependencies (exit code $LASTEXITCODE)"
                }
            } else {
                Write-Warning "cargo-machete not installed; skipping local check (CI still enforces it). Install with 'cargo install cargo-machete --locked'."
            }

            if ($SkipTests) {
                Write-Host "Skipping cargo test (-SkipTests)."
            } else {
                Write-Host "Running cargo test (--workspace)..."
                cargo test --workspace
                if ($LASTEXITCODE -ne 0) {
                    throw "cargo test failed with exit code $LASTEXITCODE"
                }
            }
        } finally {
            Pop-Location
        }
    }

} finally {
    Pop-Location
}

Write-Host "All dev checks passed successfully!" -ForegroundColor Green
