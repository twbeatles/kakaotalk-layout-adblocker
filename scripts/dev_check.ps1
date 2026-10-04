param(
    [switch]$SkipTests,
    [switch]$SkipRust,
    [switch]$FixFmt
)

$ErrorActionPreference = "Stop"

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
