# Builds the Windows-side binaries from the WSL2 working copy.
#
# The repo lives in WSL2 but the ES-9, its ASIO driver and the MSVC toolchain are all on
# the Windows host, so Windows cargo compiles straight from the \\wsl$ path. Artefacts go
# to a Windows-local target directory: sharing `target/` with the Linux build would make
# the two fight over the same fingerprints, and writing objects over 9p is slow.
#
#   powershell -File scripts\win-build.ps1              # build
#   powershell -File scripts\win-build.ps1 -Probe       # build, then run the MIDI probe
#   powershell -File scripts\win-build.ps1 -AudioProbe  # build, then run the audio probe

param(
    [switch]$Probe,
    [switch]$AudioProbe,
    [switch]$App,
    [switch]$Run,
    [string]$Distro = 'Ubuntu',
    [string]$TargetDir = 'C:\Users\alien\es9-build',
    [string]$AsioDir = 'C:\Users\alien\asiosdk',
    [string]$LibClang = 'C:\Users\alien\scoop\apps\llvm\current\bin'
)

$ErrorActionPreference = 'Stop'
$repo = "\\wsl`$\$Distro\home\alien\dev\es9-mixer"

if (-not (Test-Path $AsioDir)) {
    Write-Error @"
ASIO SDK not found at $AsioDir.
Clone it first:  git clone --depth 1 https://github.com/audiosdk/asio.git $AsioDir
It is the Steinberg SDK, dual-licensed proprietary / GPLv3; this project takes the GPLv3
option, which is why crates/es9-audio is GPL-3.0-only.
"@
}

$env:CARGO_TARGET_DIR = $TargetDir
$env:CPAL_ASIO_DIR    = $AsioDir
$env:LIBCLANG_PATH    = $LibClang

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

# The ES-9 is published through Windows MIDI Services under a generic jack name, so the
# probe cannot find it by name and needs explicit port indices. Run it with no arguments
# first to see the list.
if ($Probe)      { & "$TargetDir\debug\probe.exe" --in 1 --out 2 }
if ($AudioProbe) { & "$TargetDir\debug\audioprobe.exe" --rate 48000 }
