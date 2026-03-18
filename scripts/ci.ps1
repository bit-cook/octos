# Local CI for Windows — mirrors scripts/ci.sh.
# Usage: .\scripts\ci.ps1 [-Fix] [-Quick] [-Serial] [-Subsystem <name>]
#   -Fix       : auto-fix formatting instead of checking
#   -Quick     : skip clippy (just fmt + test)
#   -Serial    : run tests single-threaded
#   -Subsystem : run only tests for a specific subsystem (core, llm, agent, pipeline, bus, cli, memory)
param(
    [switch]$Fix,
    [switch]$Quick,
    [switch]$Serial,
    [string]$Subsystem = ""
)

$ErrorActionPreference = "Continue"
Set-Location (Split-Path $PSScriptRoot)

$Pass = 0
$Fail = 0
$Started = Get-Date

function Pass($msg) { $script:Pass++; Write-Host "  " -NoNewline; Write-Host "OK" -ForegroundColor Green -NoNewline; Write-Host " $msg" }
function Fail($msg) { $script:Fail++; Write-Host "  " -NoNewline; Write-Host "FAIL" -ForegroundColor Red -NoNewline; Write-Host " $msg" }
function Section($msg) { Write-Host "`n-- $msg --" -ForegroundColor Cyan }

$TestThreads = @()
if ($Serial) { $TestThreads = @("--", "--test-threads=1") }

# -- 1. Format --
Section "Format"
if ($Fix) {
    & cargo fmt --all 2>&1 | Out-Null
    if ($LASTEXITCODE -eq 0) { Pass "cargo fmt --all (fixed)" } else { Fail "cargo fmt" }
} else {
    $fmtOut = & cargo fmt --all -- --check 2>&1
    if ($LASTEXITCODE -eq 0) { Pass "cargo fmt" } else { Fail "cargo fmt (run with -Fix or: cargo fmt --all)" }
}

# -- 2. Clippy --
if (-not $Quick) {
    Section "Clippy"
    $clipOut = cargo clippy --workspace -- -D warnings 2>&1
    if ($LASTEXITCODE -eq 0) { Pass "cargo clippy" } else { Fail "cargo clippy"; $clipOut | Select-Object -Last 10 | Write-Host }
}

# -- 3. Tests --
if ($Subsystem) {
    Section "Subsystem Tests: $Subsystem"
    $crate = "octos-$Subsystem"
    # All crates use octos- prefix
    $testOut = cargo test -p $crate @TestThreads 2>&1
    if ($LASTEXITCODE -eq 0) {
        $passed = ($testOut | Select-String "^test result:" | ForEach-Object {
            if ($_ -match '(\d+) passed') { $Matches[1] } else { "?" }
        }) -join "+"
        Pass "$crate tests ($passed passed)"
    } else {
        Fail "$crate tests"
        $testOut | Select-Object -Last 10 | Write-Host
    }
} else {
    Section "Tests"

    # 3a. Workspace tests
    Write-Host "  Running: cargo test --workspace"
    $testOut = cargo test --workspace @TestThreads 2>&1
    if ($LASTEXITCODE -eq 0) {
        $total = 0
        $testOut | Select-String "^test result:" | ForEach-Object {
            if ($_ -match '(\d+) passed') { $total += [int]$Matches[1] }
        }
        Pass "cargo test --workspace ($total passed)"
    } else {
        Fail "cargo test --workspace"
        $testOut | Select-Object -Last 20 | Write-Host
    }

    # 3b. Focused test groups
    Section "Focused Test Groups"

    # Session actor tests (single-threaded to avoid race)
    Write-Host "  Running: session actor tests"
    $actorOut = cargo test -p octos-cli session_actor::tests -- --test-threads=1 2>&1
    if ($LASTEXITCODE -eq 0) {
        $n = 0; $actorOut | Select-String "^test result:" | ForEach-Object { if ($_ -match '(\d+) passed') { $n += [int]$Matches[1] } }
        Pass "session actor ($n tests)"
    } else { Fail "session actor" }

    # Session persistence
    Write-Host "  Running: session persistence tests"
    $sessOut = cargo test -p octos-bus session::tests @TestThreads 2>&1
    if ($LASTEXITCODE -eq 0) {
        $n = 0; $sessOut | Select-String "^test result:" | ForEach-Object { if ($_ -match '(\d+) passed') { $n += [int]$Matches[1] } }
        Pass "session persistence ($n tests)"
    } else { Fail "session persistence" }

    # Matrix channel tests
    Write-Host "  Running: matrix channel tests"
    $matrixOut = cargo test -p octos-bus --features matrix-appservice -- matrix @TestThreads 2>&1
    if ($LASTEXITCODE -eq 0) {
        $n = 0; $matrixOut | Select-String "^test result:" | ForEach-Object { if ($_ -match '(\d+) passed') { $n += [int]$Matches[1] } }
        Pass "matrix channels ($n tests)"
    } else { Fail "matrix channels" }

    # octos-cli with API feature
    if (-not $Quick) {
        Write-Host "  Running: octos-cli with API feature"
        $apiOut = cargo test -p octos-cli --features api @TestThreads 2>&1
        if ($LASTEXITCODE -eq 0) { Pass "octos-cli --features api" } else { Fail "octos-cli --features api" }
    }
}

# -- 4. Build check (quick mode skips) --
if (-not $Quick -and -not $Subsystem) {
    Section "Build Check"
    cargo build --workspace 2>&1 | Out-Null
    if ($LASTEXITCODE -eq 0) { Pass "workspace build" } else { Fail "workspace build" }
}

# -- Summary --
$Elapsed = [math]::Round(((Get-Date) - $Started).TotalSeconds)
Section "Done"
Write-Host "  $Pass passed, $Fail failed (${Elapsed}s)"
Write-Host ""

if ($Fail -gt 0) { exit 1 } else { Write-Host "All checks passed." -ForegroundColor Green; exit 0 }
