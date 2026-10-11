# Builds the release app and CLI and packs them with the README and the CLI guide into
# dist\zapret-hub-<version>.zip. Unpack the zip somewhere else to run the app.
$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
Push-Location $root
try {
    # The first `version = "..."` line of the root Cargo.toml is [workspace.package]'s.
    $version = (Select-String -Path Cargo.toml -Pattern '^version = "(.+)"$' | Select-Object -First 1).Matches[0].Groups[1].Value
    npm run tauri build
    if ($LASTEXITCODE -ne 0) { throw "npm run tauri build exited with $LASTEXITCODE" }
    # tauri build makes the app's package only; the CLI is a package of its own.
    cargo build --release -p cli
    if ($LASTEXITCODE -ne 0) { throw "cargo build --release -p cli exited with $LASTEXITCODE" }

    # Only the zip is the output. The app keeps its settings and data next to its exe, so the
    # script never touches an unpacked folder someone may run the app from: it stages the files
    # in a folder of its own and removes only that.
    $zip = Join-Path $root "dist\zapret-hub-$version.zip"
    $stage = Join-Path $root "dist\.stage-$version"
    if (Test-Path $stage) { Remove-Item -Recurse -Force $stage }
    New-Item -ItemType Directory -Force $stage | Out-Null
    Copy-Item target\release\zapret-hub.exe, target\release\zapret-hub-cli.exe, README.md, docs\cli.md -Destination $stage
    if (Test-Path $zip) { Remove-Item -Force $zip }
    Compress-Archive -Path "$stage\*" -DestinationPath $zip
    Remove-Item -Recurse -Force $stage
    Write-Output "packed $zip"
} finally {
    Pop-Location
}
