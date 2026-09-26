<#
.SYNOPSIS
Builds the WebAssembly version of the game and assembles a static site.

.DESCRIPTION
Runs `cargo build -p hero-game --target wasm32-unknown-unknown` and copies eiketsuden.wasm,
web/index.html, web/mq_js_bundle.js, web/hero_web.js and the data pack into the output folder
(default target/web-dist, which is git-ignored) — the same layout the GitHub Pages workflow
publishes. Browsers cannot load WebAssembly from file:// URLs, so serve the folder over HTTP
(-Serve does that with tools/web/serve.py, a no-cache variant of Python's built-in server).

.PARAMETER Data
Data pack directory copied to <Out>/data/base (default: data/base of this repository). A relative
path is taken relative to the current directory.

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
$Marker = ".eiketsuden-web-dist"
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

    if (Test-Path $Out) {
        if (-not (Test-Path (Join-Path $Out $Marker))) {
            throw "$Out exists but was not created by this script; refusing to overwrite it"
        }
        Remove-Item -Recurse -Force $Out
    }
    New-Item -ItemType Directory -Force (Join-Path $Out "data") | Out-Null
    New-Item -ItemType File (Join-Path $Out $Marker) | Out-Null

    Copy-Item "web/index.html", "web/mq_js_bundle.js", "web/hero_web.js" $Out
    Copy-Item "target/wasm32-unknown-unknown/$buildProfile/eiketsuden.wasm" $Out

    if (Test-Path $Data -PathType Container) {
        Copy-Item -Recurse $Data (Join-Path $Out "data/base")
        if (-not (Test-Path (Join-Path $Data "pack.toml"))) {
            Write-Warning "$Data has no pack.toml: only the UI gallery (#gallery) will work."
        }
    } else {
        Write-Warning "data pack $Data not found: the page will show the 'pack not found' error screen."
    }

    $wasm = Get-Item (Join-Path $Out "eiketsuden.wasm")
    Write-Host ("site ready in {0} (eiketsuden.wasm {1:N0} KB)" -f (Resolve-Path $Out), ($wasm.Length / 1KB))

    if ($Serve -gt 0) {
        # `python3` on Windows is often only the Microsoft Store stub, so try `python` first.
        $python = @("python", "python3") |
            Where-Object { Get-Command $_ -ErrorAction SilentlyContinue } |
            Where-Object { (Get-Command $_).Source -notlike "*\WindowsApps\*" } |
            Select-Object -First 1
        if (-not $python) { throw "python is needed for -Serve (or serve $Out with any static web server)" }
        & $python (Join-Path $PSScriptRoot "serve.py") --port $Serve --dir $Out
    }
} finally {
    Pop-Location
}
