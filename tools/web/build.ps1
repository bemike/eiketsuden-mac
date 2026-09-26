<#
.SYNOPSIS
Builds the WebAssembly version of the game and assembles a static site.

.DESCRIPTION
Runs `cargo build -p hero-game --target wasm32-unknown-unknown` and copies eiketsuden.wasm,
web/index.html, web/mq_js_bundle.js, web/hero_web.js and the data pack into the output folder
(default target/web-dist, which is git-ignored) — the same layout the GitHub Pages workflow
publishes. Browsers cannot load WebAssembly from file:// URLs, so serve the folder over HTTP
(-Serve does that with Python's built-in server).

.PARAMETER Data
Data pack directory copied to <Out>/data/base (default: data/base).

.PARAMETER Out
Output directory (default: target/web-dist). It is only ever cleared if an earlier run of this
script created it.

.PARAMETER Dev
Build the debug profile (faster to compile, much slower to run).

.PARAMETER Serve
After building, serve the site with `python -m http.server` on this port (0 = do not serve).

.EXAMPLE
pwsh tools/web/build.ps1 -Serve 8080
# then open http://localhost:8080/ or http://localhost:8080/#gallery
#>
param(
    [string]$Data = "data/base",
    [string]$Out = "target/web-dist",
    [switch]$Dev,
    [int]$Serve = 0
)

$ErrorActionPreference = "Stop"
$Marker = ".eiketsuden-web-dist"
$Root = (Resolve-Path (Join-Path $PSScriptRoot "../..")).Path
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
        Write-Host "serving on http://localhost:$Serve/  (UI gallery: http://localhost:$Serve/#gallery; Ctrl+C stops)"
        & $python -m http.server $Serve --directory $Out
    }
} finally {
    Pop-Location
}
