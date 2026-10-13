# Matchwise GPU build script (Windows PowerShell).
# Wraps build-gpu.bat. Signing is opt-in and off by default, matching the
# `sign-build` input in .github/workflows (which also defaults to false).

param(
    [switch]$Sign
)

Write-Host ""
Write-Host "========================================"
Write-Host "   Matchwise GPU Build"
Write-Host "========================================"
Write-Host ""

# Local configuration (database key, Telegram token, signing credentials), if a
# .env is present. It lives at the repo root, not in frontend/. A variable that
# is already set in the environment wins; a missing file is not an error.
$repoRoot = Resolve-Path "$PSScriptRoot\.."
if (Test-Path "$repoRoot\.env") {
    . "$PSScriptRoot\scripts\load-env.ps1"
    Load-EnvFile -EnvFilePath "$repoRoot\.env" -NoOverride -Verbose
    Write-Host ""
}

# The Windows signCommand (src-tauri/tauri.conf.json) calls
# src-tauri/scripts/sign-windows.ps1, which skips silently when this is unset.
if ($Sign) {
    if ($env:DIGICERT_KEYPAIR_ALIAS) {
        Write-Host "Signing enabled - alias: $($env:DIGICERT_KEYPAIR_ALIAS)" -ForegroundColor Green
    } else {
        Write-Host "WARNING: -Sign was passed but DIGICERT_KEYPAIR_ALIAS is not set." -ForegroundColor Yellow
        Write-Host "         The build will be unsigned. Set it in .env or the environment," -ForegroundColor Yellow
        Write-Host "         and make sure smctl is installed and signed in." -ForegroundColor Yellow
    }
} else {
    Write-Host "Building unsigned (pass -Sign to sign; requires DigiCert KeyLocker)." -ForegroundColor Cyan
}
Write-Host ""

Write-Host "Starting build process..."
Write-Host ""

& "$PSScriptRoot\build-gpu.bat" @args
$buildExitCode = $LASTEXITCODE

if ($buildExitCode -eq 0) {
    Write-Host ""
    Write-Host "========================================" -ForegroundColor Green
    Write-Host "Build completed successfully!" -ForegroundColor Green
    Write-Host "========================================" -ForegroundColor Green
    Write-Host ""
} else {
    Write-Host ""
    Write-Host "Build failed with exit code: $buildExitCode" -ForegroundColor Red
}

exit $buildExitCode
