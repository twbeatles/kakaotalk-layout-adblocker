param(
    [string]$PythonExe = "python",
    [string]$ExeName = "KakaoTalkLayoutAdBlocker_v11.exe",
    [string]$DistDir = "dist",
    [string]$WorkDir = "build",
    [switch]$NoSign,
    [switch]$SkipSmokeCheck,
    [switch]$StrictStartupSmoke
)

$ErrorActionPreference = "Stop"

$repoRoot = Resolve-Path (Join-Path $PSScriptRoot "..")

function Get-SigningConfig {
    $signtool = Get-Command signtool.exe -ErrorAction SilentlyContinue
    if (-not $signtool) {
        throw "signtool.exe not found. Install Windows SDK Signing Tools, or run with -NoSign."
    }

    $signtoolArgs = @("sign", "/fd", "SHA256")

    if ($env:SIGN_PFX_PATH) {
        if (-not (Test-Path $env:SIGN_PFX_PATH)) {
            throw "SIGN_PFX_PATH not found: $($env:SIGN_PFX_PATH)"
        }
        $signtoolArgs += @("/f", $env:SIGN_PFX_PATH)
        if ($env:SIGN_PFX_PASSWORD) {
            $signtoolArgs += @("/p", $env:SIGN_PFX_PASSWORD)
        }
    } elseif ($env:SIGN_CERT_SHA1) {
        $signtoolArgs += @("/sha1", $env:SIGN_CERT_SHA1)
        if ($env:SIGN_CERT_STORE) {
            $signtoolArgs += @("/s", $env:SIGN_CERT_STORE)
        }
        if ($env:SIGN_CERT_SUBJECT) {
            $signtoolArgs += @("/n", $env:SIGN_CERT_SUBJECT)
        }
    } else {
        throw "Signing configuration missing. Set SIGN_PFX_PATH (and optional SIGN_PFX_PASSWORD) or SIGN_CERT_SHA1."
    }

    $timestampUrl = if ($env:SIGN_TIMESTAMP_URL) { $env:SIGN_TIMESTAMP_URL } else { "http://timestamp.digicert.com" }
    $signtoolArgs += @("/tr", $timestampUrl, "/td", "SHA256")
    return [PSCustomObject]@{
        SignTool = $signtool.Source
        SignArgs = $signtoolArgs
    }
}

function Invoke-Sign {
    param(
        [Parameter(Mandatory = $true)]
        [string]$ExePath,
        [Parameter(Mandatory = $true)]
        [PSCustomObject]$SigningConfig
    )
    Write-Host "Signing $ExePath"
    $signArgs = @($SigningConfig.SignArgs + $ExePath)
    & $SigningConfig.SignTool @signArgs
    if ($LASTEXITCODE -ne 0) {
        throw "signtool sign failed with exit code $LASTEXITCODE"
    }

    & $SigningConfig.SignTool verify /pa /v $ExePath
    if ($LASTEXITCODE -ne 0) {
        throw "signtool verify failed with exit code $LASTEXITCODE"
    }
}

function New-TempDir {
    param(
        [Parameter(Mandatory = $true)]
        [string]$Prefix
    )
    $path = Join-Path ([System.IO.Path]::GetTempPath()) ("{0}-{1}" -f $Prefix, [System.Guid]::NewGuid().ToString("N"))
    New-Item -ItemType Directory -Force -Path $path | Out-Null
    return $path
}

function Remove-TempDir {
    param(
        [Parameter(Mandatory = $true)]
        [string]$Path
    )
    if (-not (Test-Path $Path)) {
        return
    }
    try {
        Remove-Item -LiteralPath $Path -Recurse -Force
    } catch {
        Write-Warning "Failed to clean temp dir ${Path}: $($_.Exception.Message)"
    }
}

function Test-InteractiveShell {
    if ($env:CI) {
        return $false
    }
    try {
        return [Environment]::UserInteractive -and ((Get-Process -Id $PID).SessionId -gt 0)
    } catch {
        return $false
    }
}

function Test-VersionMetadata {
    $pathsFile = Join-Path $repoRoot "rust\crates\kakao-app\src\config\paths.rs"
    $versionInfoFile = Join-Path $repoRoot "packaging\windows_version_info.txt"
    $iconFile = Join-Path $repoRoot "packaging\app_icon.ico"
    $pathsText = Get-Content $pathsFile -Raw
    $versionInfoText = Get-Content $versionInfoFile -Raw
    if (-not (Test-Path $iconFile)) {
        throw "Missing application icon: $iconFile"
    }

    $versionMatch = [regex]::Match($pathsText, 'pub const VERSION: &str = "([^"]+)"')
    if (-not $versionMatch.Success) {
        throw "VERSION not found in $pathsFile"
    }
    $version = $versionMatch.Groups[1].Value
    $parts = @($version.Split(".") | ForEach-Object { [int]$_ })
    if ($parts.Count -ne 3) {
        throw "VERSION must be major.minor.patch: $version"
    }
    $resourceVersion = "$version.0"
    $tupleText = "($($parts[0]), $($parts[1]), $($parts[2]), 0)"

    if ($versionInfoText -notmatch [regex]::Escape("filevers=$tupleText")) {
        throw "windows_version_info.txt filevers does not match VERSION $version"
    }
    if ($versionInfoText -notmatch [regex]::Escape("prodvers=$tupleText")) {
        throw "windows_version_info.txt prodvers does not match VERSION $version"
    }
    if ($versionInfoText -notmatch [regex]::Escape('FileVersion", u"' + $resourceVersion)) {
        throw "windows_version_info.txt FileVersion does not match VERSION $version"
    }
    if ($versionInfoText -notmatch [regex]::Escape('ProductVersion", u"' + $resourceVersion)) {
        throw "windows_version_info.txt ProductVersion does not match VERSION $version"
    }
}

function Invoke-PackagedSelfCheck {
    param(
        [Parameter(Mandatory = $true)]
        [string]$ExePath
    )

    $tempAppData = New-TempDir -Prefix "ktalb-smoke-appdata"
    $reportPath = Join-Path $tempAppData "self-check.json"
    $previousAppData = $env:APPDATA
    try {
        $env:APPDATA = $tempAppData
        Write-Host "Running packaged smoke check (--self-check --strict-self-check --json): $ExePath"
        $proc = Start-Process `
            -FilePath $ExePath `
            -ArgumentList @("--self-check", "--strict-self-check", "--json", "--self-check-report", $reportPath) `
            -PassThru `
            -WindowStyle Hidden
        if (-not $proc.WaitForExit(60000)) {
            try {
                $proc.Kill()
            } catch {
            }
            throw "packaged --self-check timed out"
        }
        $exitCode = $proc.ExitCode
    } finally {
        $env:APPDATA = $previousAppData
    }

    if (-not (Test-Path $reportPath)) {
        throw "packaged --self-check did not produce JSON report: $reportPath"
    }
    try {
        $result = Get-Content $reportPath -Raw -Encoding UTF8 | ConvertFrom-Json
        if ($null -eq $result.summary) {
            throw "packaged --self-check JSON summary missing"
        }
        if ([int]$result.summary.exit_code -ne 0 -or $exitCode -ne 0) {
            throw "packaged --self-check failed (exit_code=$($result.summary.exit_code), process_exit=$exitCode)"
        }
        if ([int]$result.summary.optional_failed -gt 0) {
            Write-Warning "Optional self-check failures detected: $($result.summary.optional_failed)"
        }
    } finally {
        Remove-TempDir -Path $tempAppData
    }
}

function Invoke-StartupSmoke {
    param(
        [Parameter(Mandatory = $true)]
        [string]$ExePath
    )

    if (-not (Test-InteractiveShell)) {
        return [PSCustomObject]@{
            status = "skipped"
            reason = "interactive shell not detected"
            tray_available = $null
            tray_start_error = ""
        }
    }

    $tempAppData = New-TempDir -Prefix "ktalb-startup-appdata"
    $tracePath = Join-Path $tempAppData "startup-trace.json"
    $previousAppData = $env:APPDATA
    try {
        $env:APPDATA = $tempAppData
        Write-Host "Running packaged startup smoke (--startup-launch --minimized)..."
        $proc = Start-Process `
            -FilePath $ExePath `
            -ArgumentList @("--startup-launch", "--minimized", "--startup-trace", $tracePath, "--exit-after-startup-ms", "1500") `
            -PassThru `
            -WindowStyle Hidden
        if (-not $proc.WaitForExit(60000)) {
            try {
                $proc.Kill()
            } catch {
            }
            throw "packaged startup smoke timed out"
        }
    } finally {
        $env:APPDATA = $previousAppData
    }
    try {
        if ($proc.ExitCode -ne 0) {
            throw "packaged startup smoke failed with exit code $($proc.ExitCode)"
        }
        if (-not (Test-Path $tracePath)) {
            throw "packaged startup smoke did not produce startup trace: $tracePath"
        }

        $trace = Get-Content $tracePath -Raw | ConvertFrom-Json
        if (-not $trace.startup_launch) {
            throw "startup trace did not record startup_launch=true"
        }
        if (-not $trace.shell_wait_attempted) {
            throw "startup trace did not record shell wait"
        }
        $trayAvailable = [bool]$trace.tray_available
        $trayStartError = ""
        if ($null -ne $trace.tray_start_error) {
            $trayStartError = [string]$trace.tray_start_error
        }
        if (-not $trace.tray_available) {
            Write-Warning "Startup smoke completed but tray was unavailable on this host."
        } elseif ($trayStartError) {
            Write-Warning "Startup smoke completed with tray warning: $trayStartError"
        }
        return [PSCustomObject]@{
            status = "completed"
            reason = ""
            tray_available = $trayAvailable
            tray_start_error = $trayStartError
        }
    } finally {
        Remove-TempDir -Path $tempAppData
    }
}

Push-Location $repoRoot
try {
    Test-VersionMetadata

    $signingConfig = $null
    if (-not $NoSign) {
        # Fail fast before build when signing prerequisites are missing.
        $signingConfig = Get-SigningConfig
    }

    Write-Host "Building Rust release binaries (kakao-app, kakao-updater)"
    $rustDir = Join-Path $repoRoot "rust"
    Push-Location $rustDir
    try {
        $link = Get-Command link.exe -ErrorAction SilentlyContinue
        if ($link) {
            cargo build --release -p kakao-app -p kakao-updater
        } else {
            $vcvarsCandidates = @(
                "C:\Program Files (x86)\Microsoft Visual Studio\2022\BuildTools\VC\Auxiliary\Build\vcvarsarm64_amd64.bat",
                "C:\Program Files\Microsoft Visual Studio\2022\Community\VC\Auxiliary\Build\vcvars64.bat",
                "C:\Program Files (x86)\Microsoft Visual Studio\2022\Community\VC\Auxiliary\Build\vcvars64.bat",
                "C:\Program Files (x86)\Microsoft Visual Studio\2022\BuildTools\VC\Auxiliary\Build\vcvars64.bat",
                "C:\Program Files (x86)\Microsoft Visual Studio\18\BuildTools\VC\Auxiliary\Build\vcvars64.bat",
                "C:\Program Files (x86)\Microsoft Visual Studio\18\BuildTools\VC\Auxiliary\Build\vcvarsall.bat"
            )
            $vcvars = $vcvarsCandidates | Where-Object { Test-Path $_ } | Select-Object -First 1
            if ($vcvars) {
                $vcvarsSuffix = ""
                if ([IO.Path]::GetFileName($vcvars) -ieq "vcvarsall.bat") {
                    $vcvarsSuffix = " x64"
                }
                cmd.exe /c "`"$vcvars`"$vcvarsSuffix && rustup run stable-x86_64-pc-windows-msvc cargo build --release -p kakao-app -p kakao-updater"
            } else {
                rustup run stable-x86_64-pc-windows-msvc cargo build --release -p kakao-app -p kakao-updater
            }
        }
        if ($LASTEXITCODE -ne 0) {
            throw "cargo build --release failed with exit code $LASTEXITCODE"
        }
    } finally {
        Pop-Location
    }

    $builtExe = Join-Path $rustDir "target\release\kakao-adblock-rs.exe"
    if (-not (Test-Path $builtExe)) {
        throw "Built EXE not found: $builtExe"
    }
    $builtUpdater = Join-Path $rustDir "target\release\kakao-updater.exe"
    $distPath = Join-Path $repoRoot $DistDir
    New-Item -ItemType Directory -Force -Path $distPath | Out-Null
    $exePath = Join-Path $distPath $ExeName
    Copy-Item -LiteralPath $builtExe -Destination $exePath -Force
    Write-Host "Copied $builtExe -> $exePath"
    if (Test-Path $builtUpdater) {
        $updaterDist = Join-Path $distPath "kakao-updater.exe"
        Copy-Item -LiteralPath $builtUpdater -Destination $updaterDist -Force
        Write-Host "Copied $builtUpdater -> $updaterDist"
    }

    if ($SkipSmokeCheck) {
        Write-Host "Skipping packaged smoke check (-SkipSmokeCheck)."
    } else {
        Invoke-PackagedSelfCheck -ExePath $exePath
        $startupSmoke = Invoke-StartupSmoke -ExePath $exePath
        Write-Host "Startup smoke status: $($startupSmoke.status)"
        if ($startupSmoke.reason) {
            Write-Host "Startup smoke detail: $($startupSmoke.reason)"
        }
        if ($StrictStartupSmoke -and $startupSmoke.status -eq "completed") {
            if (-not $startupSmoke.tray_available) {
                throw "Strict startup smoke failed: tray unavailable during interactive startup smoke"
            }
            if ($startupSmoke.tray_start_error) {
                throw "Strict startup smoke failed: $($startupSmoke.tray_start_error)"
            }
        }
    }

    if ($NoSign) {
        Write-Host "Skipping code signing (-NoSign)."
    } else {
        Invoke-Sign -ExePath $exePath -SigningConfig $signingConfig
        $updaterDist = Join-Path $distPath "kakao-updater.exe"
        if (Test-Path $updaterDist) {
            Invoke-Sign -ExePath $updaterDist -SigningConfig $signingConfig
        }
    }

    $updaterDist = Join-Path $distPath "kakao-updater.exe"
    $zipPath = Join-Path $distPath "KakaoTalkLayoutAdBlocker_v11.zip"
    if (Test-Path $updaterDist) {
        Compress-Archive -Force -Path $exePath, $updaterDist -DestinationPath $zipPath
        Write-Host "Packed $zipPath"
    } else {
        Write-Host "kakao-updater.exe missing; zip package was not created."
    }

    Write-Host "Done: $exePath"
} finally {
    Pop-Location
}
