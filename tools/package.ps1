# Builds the release exe and packs it with the README into dist\zapret-hub-<version>.zip.
$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
Push-Location $root
try {
    # The first `version = "..."` line of the root Cargo.toml is [workspace.package]'s.
    $version = (Select-String -Path Cargo.toml -Pattern '^version = "(.+)"$' | Select-Object -First 1).Matches[0].Groups[1].Value
    npm run tauri build
    if ($LASTEXITCODE -ne 0) { throw "npm run tauri build exited with $LASTEXITCODE" }

    $stage = Join-Path $root "dist\zapret-hub-$version"
    $zip = "$stage.zip"
    Remove-Item -Recurse -Force $stage, $zip -ErrorAction SilentlyContinue
    New-Item -ItemType Directory -Force $stage | Out-Null
    Copy-Item target\release\zapret-hub.exe, README.md -Destination $stage
    Compress-Archive -Path "$stage\*" -DestinationPath $zip
    Write-Output "packed $zip"
} finally {
    Pop-Location
}
