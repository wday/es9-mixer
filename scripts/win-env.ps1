# Shared build environment for the Windows-side scripts. Dot-source it, then call
# Resolve-BuildEnv.
#
# Everything here is discovered rather than hardcoded, because the three things the ES-9
# build needs — the ASIO SDK, LLVM, and a Windows-local target directory — sit wherever
# the person building it put them. Each can be overridden by parameter or by having the
# corresponding environment variable already set; an override always wins over discovery.

function Find-AsioSdk {
    param([string]$Preferred)

    $candidates = @()
    if ($Preferred)          { $candidates += $Preferred }
    if ($env:CPAL_ASIO_DIR)  { $candidates += $env:CPAL_ASIO_DIR }
    $candidates += @(
        (Join-Path $env:USERPROFILE 'asiosdk'),
        'C:\SDKs\asiosdk',
        'C:\asiosdk'
    )

    foreach ($c in $candidates) {
        # The SDK is identified by its contents, not its name: cpal wants the directory
        # holding `common/`, and the archive unpacks under a version-stamped folder that
        # people rename or don't.
        if ($c -and (Test-Path (Join-Path $c 'common\asio.h'))) { return (Resolve-Path $c).Path }
        if ($c -and (Test-Path $c)) {
            $inner = Get-ChildItem -Path $c -Directory -ErrorAction SilentlyContinue |
                Where-Object { Test-Path (Join-Path $_.FullName 'common\asio.h') } |
                Select-Object -First 1
            if ($inner) { return $inner.FullName }
        }
    }
    return $null
}

function Find-LibClang {
    param([string]$Preferred)

    $candidates = @()
    if ($Preferred)           { $candidates += $Preferred }
    if ($env:LIBCLANG_PATH)   { $candidates += $env:LIBCLANG_PATH }

    # Whatever `clang` is on PATH, if any — scoop, winget, chocolatey and the Visual
    # Studio installer all put it somewhere different.
    $onPath = Get-Command clang.exe -ErrorAction SilentlyContinue
    if ($onPath) { $candidates += (Split-Path -Parent $onPath.Source) }

    $candidates += @(
        (Join-Path $env:USERPROFILE 'scoop\apps\llvm\current\bin'),
        'C:\Program Files\LLVM\bin',
        "${env:ProgramFiles(x86)}\LLVM\bin"
    )

    foreach ($c in $candidates) {
        if ($c -and (Test-Path (Join-Path $c 'libclang.dll'))) { return (Resolve-Path $c).Path }
    }
    return $null
}

function Resolve-TargetDir {
    <#
      A Windows-local target directory, never the repo's own. Sharing `target/` with a
      Linux-side build makes the two fight over the same fingerprints, and writing object
      files over the 9p share to \\wsl$ is slow enough to notice.
    #>
    param([string]$TargetDir)

    if ($TargetDir) { return $TargetDir }
    if ($env:CARGO_TARGET_DIR) { return $env:CARGO_TARGET_DIR }
    return (Join-Path $env:LOCALAPPDATA 'es9-mixer\build')
}

function Resolve-BuildEnv {
    <#
      Sets CARGO_TARGET_DIR, CPAL_ASIO_DIR and LIBCLANG_PATH for the current process, and
      throws with instructions if a prerequisite is missing. Returns the target directory.
    #>
    param(
        [string]$TargetDir,
        [string]$AsioDir,
        [string]$LibClang
    )

    $TargetDir = Resolve-TargetDir -TargetDir $TargetDir

    $asio = Find-AsioSdk -Preferred $AsioDir
    if (-not $asio) {
        throw @"
ASIO SDK not found.

Download it from Steinberg (https://www.steinberg.net/developers/) and unpack it, or
clone a mirror:

    git clone --depth 1 https://github.com/audiosdk/asio.git "$env:USERPROFILE\asiosdk"

Then re-run, or pass -AsioDir <path>, or set CPAL_ASIO_DIR. The directory wanted is the
one containing common\asio.h.

The SDK is dual-licensed proprietary / GPLv3. This project takes the GPLv3 option, which
is why crates/es9-audio and the desktop shell are GPL-3.0-only.
"@
    }

    $clang = Find-LibClang -Preferred $LibClang
    if (-not $clang) {
        throw @"
libclang.dll not found. The ASIO backend generates its bindings with bindgen, which needs
LLVM.

    winget install LLVM.LLVM        (or: scoop install llvm)

Then re-run, or pass -LibClang <path to the bin directory>, or set LIBCLANG_PATH.
"@
    }

    $env:CARGO_TARGET_DIR = $TargetDir
    $env:CPAL_ASIO_DIR    = $asio
    $env:LIBCLANG_PATH    = $clang

    # Static CRT, set here rather than left to .cargo/config.toml alone.
    #
    # The config file gets rustc right, but cc-rs compiles the ASIO SDK's C++ with the
    # dynamic CRT unless it sees crt-static in RUSTFLAGS itself. The result links, runs,
    # and imports the Universal CRT alongside Rust's static one — two C runtimes, each
    # with its own heap, in a binary where the C++ side allocates. That is a memory
    # corruption bug waiting for the right allocation to cross the boundary, and nothing
    # about the build output says it happened.
    #
    # RUSTFLAGS overrides the config files rather than adding to them. The value is
    # deliberately identical, so a build through these scripts and a hand-run cargo from
    # the repo root produce the same binary.
    $env:RUSTFLAGS = '-C target-feature=+crt-static'

    Write-Host "target   $TargetDir"
    Write-Host "asio sdk $asio"
    Write-Host "libclang $clang"
    return $TargetDir
}
