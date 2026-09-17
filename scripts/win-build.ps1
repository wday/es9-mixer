# Builds the Windows-side binaries.
#
# The ES-9, its ASIO driver and the MSVC toolchain are all on Windows. If the working copy
# lives in WSL2, Windows cargo compiles straight from the \\wsl$ path — the repo is found
# relative to this script either way. Artefacts go to a Windows-local target directory:
# sharing `target/` with a Linux-side build makes the two fight over the same fingerprints,
# and writing objects over 9p is slow.
#
#   powershell -File scripts\win-build.ps1              # build
#   powershell -File scripts\win-build.ps1 -App -Run    # build the desktop shell and run it
#   powershell -File scripts\win-build.ps1 -Probe       # build, then run the MIDI probe
#   powershell -File scripts\win-build.ps1 -AudioProbe  # build, then run the audio probe
#
# Prerequisites are discovered, not assumed: see scripts\win-env.ps1. Pass -AsioDir,
# -LibClang or -TargetDir to override, or set CPAL_ASIO_DIR / LIBCLANG_PATH /
# CARGO_TARGET_DIR.

param(
    [switch]$Probe,
    [switch]$AudioProbe,
    [switch]$App,
    [switch]$Run,
    # Port indices for -Probe. The ES-9 is published through Windows MIDI Services under a
    # generic jack name, so it cannot be found by name; run probe.exe with no arguments
    # once to see the list.
    [int]$In = -1,
    [int]$Out = -1,
    [int]$Rate = 48000,
    [string]$Repo,
    [string]$TargetDir,
    [string]$AsioDir,
    [string]$LibClang
)

$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'win-env.ps1')

# The script lives in <repo>\scripts, so the checkout is its parent — which works whether
# it was launched from a Windows path or from \\wsl$.
$repo = if ($Repo) { $Repo } else { Split-Path -Parent $PSScriptRoot }
$TargetDir = Resolve-BuildEnv -TargetDir $TargetDir -AsioDir $AsioDir -LibClang $LibClang

Push-Location $repo
try {
    cargo build -p es9-midi -p es9-audio
    if ($LASTEXITCODE -ne 0) { throw "cargo build failed" }

    # The desktop shell is its own workspace (Tauri will not build on the Linux side of
    # this repo), so it is built by manifest path rather than by package name.
    if ($App -or $Run) {
        cargo build --manifest-path crates\es9-app\Cargo.toml
        if ($LASTEXITCODE -ne 0) { throw "shell build failed" }
    }
    if ($Run) {
        cargo run --manifest-path crates\es9-app\Cargo.toml
    }
}
finally { Pop-Location }

if ($Probe) {
    $probeArgs = @()
    if ($In -ge 0)  { $probeArgs += @('--in', $In) }
    if ($Out -ge 0) { $probeArgs += @('--out', $Out) }
    # With no indices the probe lists the ports and stops, which is the right first run.
    & "$TargetDir\debug\probe.exe" @probeArgs
}
if ($AudioProbe) { & "$TargetDir\debug\audioprobe.exe" --rate $Rate }
