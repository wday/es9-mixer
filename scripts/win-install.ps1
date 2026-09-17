# Installs the ES-9 Mixer for the current user and puts a shortcut in the Start Menu.
#
# Per-user, so it needs no elevation: the binary goes to Local AppData and the shortcut to
# the user's own Start Menu. Re-running it upgrades in place.
#
#   powershell -File scripts\win-install.ps1
#
# `make win-install` from a WSL2 side runs exactly this, passing -Repo so the path comes
# from the checkout being built. Run directly, the checkout is found relative to this
# script. Prerequisites are discovered; see scripts\win-env.ps1.
#
# Windows deliberately blocks programmatic taskbar pinning, so the last step is yours:
# find "ES-9 Mixer" in the Start menu, right-click it and choose "Pin to taskbar".

param(
    [string]$Repo,
    [string]$TargetDir,
    [string]$AsioDir,
    [string]$LibClang,
    [switch]$SkipBuild
)

$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'win-env.ps1')

$repo    = if ($Repo) { $Repo } else { Split-Path -Parent $PSScriptRoot }
$appName = 'ES-9 Mixer'
$installDir = Join-Path $env:LOCALAPPDATA 'Programs\ES-9 Mixer'

# -SkipBuild installs a binary that is already there, so it must not demand a toolchain
# it is not going to use.
$TargetDir = if ($SkipBuild) { Resolve-TargetDir -TargetDir $TargetDir }
             else { Resolve-BuildEnv -TargetDir $TargetDir -AsioDir $AsioDir -LibClang $LibClang }

if (-not $SkipBuild) {
    Push-Location $repo
    try {
        # Tauri bakes web/ into the binary, and crates/es9-app/build.rs watches that
        # directory, so a frontend-only change still triggers a real recompile here.
        Write-Host "Building release binary from $repo ..."
        cargo build --release --manifest-path crates\es9-app\Cargo.toml
        if ($LASTEXITCODE -ne 0) { throw 'release build failed' }
    }
    finally { Pop-Location }
}

$exe = Join-Path $TargetDir 'release\es9-app.exe'
if (-not (Test-Path $exe)) { throw "no release binary at $exe" }

New-Item -ItemType Directory -Force -Path $installDir | Out-Null
$installed = Join-Path $installDir 'ES-9 Mixer.exe'

# The app may be running from a previous install; stop it so the copy can succeed.
Get-Process -Name 'ES-9 Mixer', 'es9-app' -ErrorAction SilentlyContinue | Stop-Process -Force
Start-Sleep -Milliseconds 400

Copy-Item $exe $installed -Force
Write-Host "Installed to $installed"

$startMenu = Join-Path $env:APPDATA 'Microsoft\Windows\Start Menu\Programs'
$lnk = Join-Path $startMenu "$appName.lnk"
$shell = New-Object -ComObject WScript.Shell
$shortcut = $shell.CreateShortcut($lnk)
$shortcut.TargetPath       = $installed
$shortcut.WorkingDirectory = $installDir
$shortcut.IconLocation     = "$installed,0"
$shortcut.Description      = 'Control surface for the Expert Sleepers ES-9'
$shortcut.Save()
Write-Host "Start Menu shortcut: $lnk"

Write-Host ''
Write-Host 'To put it on the taskbar: open the Start menu, type "ES-9 Mixer",'
Write-Host 'right-click the result and choose "Pin to taskbar".'
Write-Host 'Windows blocks applications from pinning themselves, so that step is manual.'
