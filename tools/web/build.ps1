<#
.SYNOPSIS
Builds the WebAssembly version of the game and assembles a static site.

.DESCRIPTION
Runs `cargo build -p hero-game --target wasm32-unknown-unknown`, then tools/web/assemble.py
(Python 3.11+, the same step the GitHub Pages workflow runs) lays out the site in the output folder
(default target/web-dist, which is git-ignored): the page, the wasm and the data pack with the packs
it extends. Browsers cannot load WebAssembly from file:// URLs, so serve the folder over HTTP
(-Serve does that with tools/web/serve.py, a no-cache variant of Python's built-in server).

.PARAMETER Data
Data pack directory copied to <Out>/data/base, with the packs it extends (default: data/base of this
repository). A relative path is taken relative to the current directory.

.PARAMETER Out
Output directory (default: target/web-dist of this repository; relative paths as for -Data). It
is only ever cleared if an earlier run of this script created it.

.PARAMETER Dev
Build the debug profile (faster to compile, much slower to run).

.PARAMETER Serve
After building, serve the site on http://localhost:<port>/ with tools/web/serve.py (0 = do not
serve).

.EXAMPLE
pwsh tools/web/build.ps1 -Serve 8080
# then open http://localhost:8080/ or http://localhost:8080/#gallery
#>
param(
    [string]$Data = "",
    [string]$Out = "",
    [switch]$Dev,
    [int]$Serve = 0
)

$ErrorActionPreference = "Stop"
$Root = (Resolve-Path (Join-Path $PSScriptRoot "../..")).Path
# Explicit paths are relative to where the script was started, the defaults to the repository.
$Data = if ($Data) { [IO.Path]::GetFullPath($Data, (Get-Location).Path) } else { Join-Path $Root "data/base" }
$Out = if ($Out) { [IO.Path]::GetFullPath($Out, (Get-Location).Path) } else { Join-Path $Root "target/web-dist" }
Push-Location $Root
try {
    $buildProfile = if ($Dev) { "debug" } else { "release" }
    $cargoArgs = @("build", "-p", "hero-game", "--target", "wasm32-unknown-unknown")
    if (-not $Dev) { $cargoArgs += "--release" }
    Write-Host "cargo $($cargoArgs -join ' ')"
    & cargo @cargoArgs
    if ($LASTEXITCODE -ne 0) { throw "cargo build failed (exit code $LASTEXITCODE)" }

    # `python3` on Windows is often only the Microsoft Store stub, so try `python` first.
    $python = @("python", "python3") |
        Where-Object { Get-Command $_ -ErrorAction SilentlyContinue } |
        Where-Object { (Get-Command $_).Source -notlike "*\WindowsApps\*" } |
        Select-Object -First 1
    if (-not $python) { throw "python 3.11+ is needed to assemble the site (tools/web/assemble.py)" }
    $wasm = "target/wasm32-unknown-unknown/$buildProfile/eiketsuden.wasm"
    & $python (Join-Path $PSScriptRoot "assemble.py") --wasm $wasm --out $Out --data $Data
    if ($LASTEXITCODE -ne 0) { throw "assembling the site failed (exit code $LASTEXITCODE)" }

    if ($Serve -gt 0) {
        & $python (Join-Path $PSScriptRoot "serve.py") --port $Serve --dir $Out
    }
} finally {
    Pop-Location
}
