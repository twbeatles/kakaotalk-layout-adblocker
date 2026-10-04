param(
    [switch]$RunTests
)

$ErrorActionPreference = "Stop"

$repoRoot = Resolve-Path (Join-Path $PSScriptRoot "..")
$exe = Join-Path $repoRoot "dist\KakaoTalkLayoutAdBlocker_v11.exe"

Push-Location $repoRoot
try {
    if (Test-Path $exe) {
        Write-Host "Running self-check..."
        & $exe --self-check
        if ($LASTEXITCODE -ne 0) {
            throw "--self-check failed with exit code $LASTEXITCODE"
        }
    } else {
        Write-Warning "Packaged EXE not found; skipping self-check. Build it with scripts/build_release.ps1 -NoSign first."
    }

    if ($RunTests) {
        Write-Host "Running cargo test (--workspace)..."
        Push-Location (Join-Path $repoRoot "rust")
        try {
            cargo test --workspace
            if ($LASTEXITCODE -ne 0) {
                throw "cargo test failed with exit code $LASTEXITCODE"
            }
        } finally {
            Pop-Location
        }
    }
    Write-Host ""
    Write-Host "Manual smoke checklist:"
    Write-Host "1. Tray visibility: start with --minimized and verify the tray icon is actually visible without opening the main window."
    Write-Host "2. Tray unavailable fallback: start with --minimized in tray-disabled env and verify main window is shown."
    Write-Host "3. Popup negative guard: verify a non-ad popup host with non-empty title is not dismissed by default even if it contains AdFitWebView."
    Write-Host "4. Token-less bottom panel: verify a bottom Chrome widget without ad token is not hidden by default."
    Write-Host "5. OFF/ON transition: toggle blocking OFF then ON and verify engine resumes immediately."
    Write-Host "6. Stop/exit restore: exit during active blocking and verify hidden windows are not re-hidden after restore."
    Write-Host "7. Restore failure reset: trigger/observe restore failure and verify '복원 실패 초기화' clears the counters."
    Write-Host "8. Logging startup fallback: simulate log-file lock/permission issue and verify startup survives with status/log warning."
    Write-Host "9. Tk/logging self-check: break Tk runtime or logging bootstrap intentionally and verify --self-check reports failure."
    Write-Host "10. Popup fallback restore: verify a popup that survives WM_CLOSE but is hidden/zero-sized is restored when blocking is turned OFF."
    Write-Host "11. Hung close handling: simulate or observe an unresponsive popup close and verify the engine reports timeout without freezing."
    Write-Host "12. Dynamic custom-scroll guard: verify an empty EVA child is not closed if a custom scroll child appears between ticks."
    Write-Host "13. Strict self-check: run --self-check --strict-self-check --json and verify core warnings fail the check."
} finally {
    Pop-Location
}
