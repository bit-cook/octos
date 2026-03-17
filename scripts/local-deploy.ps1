# Local deployment for octos on Windows.
# Usage: .\scripts\local-deploy.ps1 [-Full] [-Channels <list>] [-NoSkills] [-NoService] [-Uninstall] [-Debug] [-Prefix <dir>]
#
# Options:
#   -Full          All channels + dashboard + app-skills
#   -Channels      Comma-separated channels (telegram,discord,slack,matrix,etc.)
#   -NoSkills      Skip building app-skills
#   -NoService     Skip Windows service/scheduled task setup
#   -Uninstall     Remove binaries and scheduled task
#   -Debug         Build in debug mode (faster compile, larger binary)
#   -Prefix        Install prefix (default: $env:USERPROFILE\.cargo\bin)
param(
    [switch]$Full,
    [string]$Channels = "",
    [switch]$NoSkills,
    [switch]$NoService,
    [switch]$Uninstall,
    [switch]$Debug,
    [string]$Prefix = ""
)

$ErrorActionPreference = "Stop"
Set-Location (Split-Path $PSScriptRoot)

if (-not $Prefix) {
    $cargoHome = if ($env:CARGO_HOME) { $env:CARGO_HOME } else { "$env:USERPROFILE\.cargo" }
    $Prefix = "$cargoHome\bin"
}
$DataDir = if ($env:OCTOS_HOME) { $env:OCTOS_HOME } else { "$env:USERPROFILE\.octos" }

function Section($msg) { Write-Host "`n==> $msg" -ForegroundColor Cyan }
function Ok($msg) { Write-Host "    OK: $msg" -ForegroundColor Green }
function Warn($msg) { Write-Host "    WARN: $msg" -ForegroundColor Yellow }
function Err($msg) { Write-Host "    ERROR: $msg" -ForegroundColor Red; exit 1 }

# -- Uninstall --
if ($Uninstall) {
    Section "Uninstalling octos"

    # Remove scheduled task
    $task = Get-ScheduledTask -TaskName "OctosServe" -ErrorAction SilentlyContinue
    if ($task) {
        Unregister-ScheduledTask -TaskName "OctosServe" -Confirm:$false
        Ok "scheduled task removed"
    }

    # Remove binaries
    $bins = @("octos.exe", "news_fetch.exe", "deep-search.exe", "deep_crawl.exe", "send_email.exe", "account_manager.exe", "clock.exe", "weather.exe")
    foreach ($bin in $bins) {
        $path = Join-Path $Prefix $bin
        if (Test-Path $path) { Remove-Item $path -Force }
    }
    Ok "binaries removed from $Prefix"

    Write-Host ""
    Write-Host "Binaries and service files removed."
    Write-Host "Data directory ($DataDir) was NOT removed. Delete manually if desired:"
    Write-Host "  Remove-Item -Recurse -Force $DataDir"
    exit 0
}

# -- Prerequisites --
Section "Checking prerequisites"

$cargoVer = cargo --version 2>&1
if ($LASTEXITCODE -ne 0) { Err "Rust not found. Install from https://rustup.rs" }
Ok "Rust: $cargoVer"

$arch = if ([Environment]::Is64BitOperatingSystem) { "x64" } else { "x86" }
$osVer = [System.Environment]::OSVersion.Version
Ok "Windows $($osVer.Major).$($osVer.Minor) ($arch)"

if (Get-Command node -ErrorAction SilentlyContinue) {
    Ok "Node.js $(node --version) (for WhatsApp bridge)"
} else {
    Warn "Node.js not found (optional: WhatsApp bridge)"
}

if (Get-Command ffmpeg -ErrorAction SilentlyContinue) {
    Ok "ffmpeg found (for media skills)"
} else {
    Warn "ffmpeg not found (optional: media skills)"
}

# -- Resolve features --
Section "Resolving build configuration"

$cliFeatures = ""
if ($Full) {
    $cliFeatures = "api,telegram,discord,slack,whatsapp,feishu,email,twilio,wecom,matrix"
    $NoSkills = $false
    Write-Host "    Mode: full (all channels + dashboard + skills)"
} else {
    Write-Host "    Mode: minimal (CLI + chat only)"
}

if ($Channels) {
    if ($cliFeatures) { $cliFeatures += ",$Channels" } else { $cliFeatures = $Channels }
}

# Auto-add api feature if any channel is set
if ($cliFeatures -and $cliFeatures -notmatch "api") {
    $cliFeatures = "api,$cliFeatures"
}

if ($cliFeatures) { Write-Host "    Features: $cliFeatures" }
else { Write-Host "    Features: (none -- CLI only)" }

# -- Build --
Section "Building octos"

$installFlag = @()
if ($Debug) { $installFlag = @("--debug") }

if ($cliFeatures) {
    Write-Host "    cargo install octos-cli with features: $cliFeatures"
    cargo install --path crates/octos-cli --features $cliFeatures @installFlag
} else {
    Write-Host "    cargo install octos-cli (no extra features)"
    cargo install --path crates/octos-cli @installFlag
}
if ($LASTEXITCODE -ne 0) { Err "Build failed" }
Ok "octos binary installed to $Prefix\octos.exe"

# App-skills
if (-not $NoSkills) {
    Section "Building app-skills"

    $buildFlag = if ($Debug) { @() } else { @("--release") }
    $skillCrates = @("news_fetch", "deep-search", "deep-crawl", "send-email", "account-manager", "clock", "weather")

    foreach ($crate in $skillCrates) {
        Write-Host "    Building $crate..."
        cargo build @buildFlag -p $crate 2>&1 | Select-Object -Last 1
    }

    $binDir = if ($Debug) { "target\debug" } else { "target\release" }
    $skillBins = @("news_fetch", "deep-search", "deep_crawl", "send_email", "account_manager", "clock", "weather")
    foreach ($bin in $skillBins) {
        $src = "$binDir\$bin.exe"
        if (Test-Path $src) {
            Copy-Item $src "$Prefix\$bin.exe" -Force
        }
    }
    Ok "app-skill binaries copied to $Prefix"
}

# -- Initialize --
Section "Initializing octos workspace"

if (-not (Test-Path $DataDir)) {
    & "$Prefix\octos.exe" init --defaults 2>$null
    if (Test-Path $DataDir) { Ok "created $DataDir" }
    else { & "$Prefix\octos.exe" init 2>$null; Ok "created $DataDir" }
} else {
    Ok "$DataDir already exists (skipping init)"
}

# -- Service setup (Windows Scheduled Task) --
if (-not $NoService -and $cliFeatures) {
    Section "Setting up background service"

    $octosBin = "$Prefix\octos.exe"
    $taskName = "OctosServe"

    # Remove existing task if present
    $existing = Get-ScheduledTask -TaskName $taskName -ErrorAction SilentlyContinue
    if ($existing) {
        Unregister-ScheduledTask -TaskName $taskName -Confirm:$false
    }

    $action = New-ScheduledTaskAction -Execute $octosBin -Argument "serve --port 8080"
    $trigger = New-ScheduledTaskTrigger -AtLogOn
    $settings = New-ScheduledTaskSettingsSet -AllowStartIfOnBatteries -DontStopIfGoingOnBatteries -RestartCount 3 -RestartInterval (New-TimeSpan -Minutes 1) -ExecutionTimeLimit (New-TimeSpan -Days 365)

    Register-ScheduledTask -TaskName $taskName -Action $action -Trigger $trigger -Settings $settings -Description "octos serve (dashboard + gateway)" | Out-Null
    Ok "scheduled task '$taskName' registered (runs at logon)"
    Write-Host "    To start now:  Start-ScheduledTask -TaskName $taskName"
    Write-Host "    To stop:       Stop-ScheduledTask -TaskName $taskName"
    Write-Host "    To remove:     Unregister-ScheduledTask -TaskName $taskName"
    Write-Host "    Logs:          $DataDir\serve.log"
} elseif (-not $NoService) {
    Write-Host "`n    Service setup skipped (no features enabled -- use -Full or -Channels)"
}

# -- Summary --
Section "Deployment complete"
Write-Host ""
Write-Host "    Binary:     $Prefix\octos.exe"
Write-Host "    Data dir:   $DataDir"
Write-Host "    Config:     $DataDir\config.json"
Write-Host ""
Write-Host "  Next steps:"
Write-Host "    1. Set your API key:  `$env:ANTHROPIC_API_KEY = 'sk-...'"
Write-Host "    2. Start chatting:    octos chat"
if ($cliFeatures) {
    Write-Host "    3. Start dashboard:   octos serve"
    Write-Host "    4. Open browser:      http://localhost:8080/admin/"
}
Write-Host ""
