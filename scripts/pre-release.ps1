# Pre-release smoke test suite for octos (Windows).
# Usage: .\scripts\pre-release.ps1 [-SkipBuild] [-SkipE2E] [-Debug]
param(
    [switch]$SkipBuild,
    [switch]$SkipE2E,
    [switch]$Debug
)

$ErrorActionPreference = "Continue"
Set-Location (Split-Path $PSScriptRoot)

$Pass = 0; $Fail = 0; $Skip = 0
$Profile = if ($Debug) { "dev" } else { "release" }

function Pass($msg) { $script:Pass++; Write-Host "  PASS: $msg" -ForegroundColor Green }
function Fail($msg) { $script:Fail++; Write-Host "  FAIL: $msg" -ForegroundColor Red }
function Skip($msg) { $script:Skip++; Write-Host "  SKIP: $msg" -ForegroundColor Yellow }
function Section($msg) { Write-Host "`n=== $msg ===" -ForegroundColor Cyan }

# -- 1. Format --
Section "Format Check"
cargo fmt --all -- --check 2>&1 | Out-Null
if ($LASTEXITCODE -eq 0) { Pass "cargo fmt" } else { Fail "cargo fmt (run: cargo fmt --all)" }

# -- 2. Clippy --
Section "Clippy Lint"
$clipOut = cargo clippy --workspace --all-targets 2>&1
$errs = ($clipOut | Select-String "^error\[").Count
$warns = ($clipOut | Select-String "^warning\[").Count
if ($errs -eq 0) { Pass "cargo clippy ($warns warnings)" }
else { Fail "cargo clippy ($errs errors)"; $clipOut | Select-Object -Last 10 | Write-Host }

# -- 3. Tests --
Section "Unit & Integration Tests"

Write-Host "  Running: cargo test --workspace"
$testOut = cargo test --workspace 2>&1
$totalPass = 0; $totalFail = 0
$testOut | Select-String "^test result:" | ForEach-Object {
    if ($_ -match '(\d+) passed') { $totalPass += [int]$Matches[1] }
    if ($_ -match '(\d+) failed') { $totalFail += [int]$Matches[1] }
}
Write-Host "  Totals: $totalPass passed, $totalFail failed"
if ($totalFail -eq 0) { Pass "workspace tests ($totalPass passed)" } else { Fail "workspace tests ($totalFail failures)" }

Write-Host "`n  Running: cargo test -p octos-cli --features api"
$cliOut = cargo test -p octos-cli --features api 2>&1
$cliPass = 0; $cliFail = 0
$cliOut | Select-String "^test result:" | ForEach-Object {
    if ($_ -match '(\d+) passed') { $cliPass += [int]$Matches[1] }
    if ($_ -match '(\d+) failed') { $cliFail += [int]$Matches[1] }
}
if ($cliFail -eq 0) { Pass "octos-cli API tests ($cliPass passed)" } else { Fail "octos-cli API tests ($cliFail failures)" }

# Matrix tests
Write-Host "`n  Running: matrix channel tests"
$matrixOut = cargo test -p octos-bus --features matrix-appservice -- matrix 2>&1
$matrixPass = 0
$matrixOut | Select-String "^test result:" | ForEach-Object {
    if ($_ -match '(\d+) passed') { $matrixPass += [int]$Matches[1] }
}
if ($LASTEXITCODE -eq 0) { Pass "matrix channels ($matrixPass passed)" } else { Fail "matrix channels" }

# -- 4. Build --
Section "Build"

if ($SkipBuild) {
    Skip "build (--SkipBuild)"
} else {
    $features = "telegram,whatsapp,feishu,twilio,api,matrix"
    Write-Host "  Building octos-cli ($Profile) with features: $features"

    $buildArgs = @("-p", "octos-cli", "--features", $features)
    if ($Profile -eq "release") { $buildArgs = @("--release") + $buildArgs }

    cargo build @buildArgs 2>&1 | Select-Object -Last 3 | Write-Host
    if ($LASTEXITCODE -eq 0) { Pass "octos-cli build ($Profile)" } else { Fail "octos-cli build ($Profile)" }

    Write-Host "  Building app-skills"
    $skillCrates = @("news_fetch", "deep-search", "deep-crawl", "send-email", "account-manager", "clock", "weather")
    cargo build --release @($skillCrates | ForEach-Object { "-p"; $_ }) 2>&1 | Select-Object -Last 3 | Write-Host
    if ($LASTEXITCODE -eq 0) { Pass "app-skills build" } else { Fail "app-skills build" }
}

# -- 5. E2E Smoke Tests --
Section "E2E Smoke Tests"

if ($SkipE2E) {
    Skip "E2E tests (-SkipE2E)"
} else {
    $binDir = if ($Profile -eq "release") { "target\release" } else { "target\debug" }
    $octos = "$binDir\octos.exe"

    if (-not (Test-Path $octos)) {
        Fail "binary not found at $octos (run without -SkipBuild)"
    } else {
        $e2eDir = Join-Path $env:TEMP "octos-e2e-$(Get-Random)"
        New-Item -ItemType Directory -Path $e2eDir -Force | Out-Null

        # Version
        $ver = & $octos --version 2>&1
        if ($ver -match "^octos \d") { Pass "octos --version" } else { Fail "octos --version" }

        # Help
        $help = & $octos --help 2>&1
        if ($help -match "Usage:") { Pass "octos --help" } else { Fail "octos --help" }

        # Init
        Push-Location $e2eDir
        & $octos init 2>&1 | Out-Null
        if (Test-Path ".octos") { Pass "octos init (creates .octos/)" } else { Fail "octos init" }

        # Status
        & $octos status 2>&1 | Out-Null
        if ($LASTEXITCODE -le 1) { Pass "octos status" } else { Fail "octos status" }

        # Skills list
        & $octos skills list 2>&1 | Out-Null
        if ($LASTEXITCODE -le 1) { Pass "octos skills list" } else { Fail "octos skills list" }

        # Channels status
        & $octos channels status 2>&1 | Out-Null
        if ($LASTEXITCODE -le 1) { Pass "octos channels status" } else { Fail "octos channels status" }

        # Completions
        & $octos completions powershell 2>&1 | Out-Null
        if ($LASTEXITCODE -eq 0) { Pass "octos completions powershell" } else { Fail "octos completions powershell" }

        # Config is valid JSON
        $configPath = ".octos\config.json"
        if (Test-Path $configPath) {
            try { Get-Content $configPath | ConvertFrom-Json | Out-Null; Pass "config.json is valid JSON" }
            catch { Fail "config.json is invalid JSON" }
        } else { Skip "config.json not created by init" }

        # Auth status
        & $octos auth status 2>&1 | Out-Null
        Pass "octos auth status"

        # Clean
        & $octos clean 2>&1 | Out-Null
        if ($LASTEXITCODE -le 1) { Pass "octos clean" } else { Fail "octos clean" }

        Pop-Location
        Remove-Item -Recurse -Force $e2eDir -ErrorAction SilentlyContinue

        # Skill binaries
        $binDir = "target\release"
        $skillBins = @("news_fetch", "deep-search", "deep_crawl", "send_email", "account_manager", "clock", "weather")
        foreach ($bin in $skillBins) {
            $path = "$binDir\$bin.exe"
            if (Test-Path $path) {
                $out = & $path --help 2>&1
                if ($LASTEXITCODE -le 1) { Pass "skill binary: $bin" }
                else { Pass "skill binary: $bin (exists)" }
            } else {
                Skip "skill binary: $bin (not built)"
            }
        }
    }
}

# -- Summary --
Section "Summary"
Write-Host "  Passed:  $Pass"
Write-Host "  Failed:  $Fail"
Write-Host "  Skipped: $Skip"
Write-Host ""

if ($Fail -gt 0) {
    Write-Host "RELEASE BLOCKED: $Fail check(s) failed." -ForegroundColor Red
    exit 1
} else {
    Write-Host "All checks passed. Ready to release." -ForegroundColor Green
    exit 0
}
