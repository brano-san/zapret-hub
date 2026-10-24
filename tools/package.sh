#!/usr/bin/env bash
# Builds the release app and CLI on Linux and packs them with the docs into
# dist/zapret-hub-<version>-linux-x86_64.tar.gz (the Linux twin of package.ps1).
set -euo pipefail
cd "$(dirname "$0")/.."

# The first `version = "..."` line of the root Cargo.toml is [workspace.package]'s.
version=$(sed -n 's/^version = "\(.*\)"$/\1/p' Cargo.toml | head -n1)
npm run tauri build
# tauri build makes the app's package only; the CLI is a package of its own.
cargo build --release -p cli

name="zapret-hub-$version-linux-x86_64"
stage="dist/.stage-$version/$name"
rm -rf "dist/.stage-$version"
mkdir -p "$stage"
cp target/release/zapret-hub target/release/zapret-hub-cli README.md docs/cli.md docs/linux.md \
    src-tauri/icons/icon.png "$stage/"
tar -czf "dist/$name.tar.gz" -C "dist/.stage-$version" "$name"
rm -rf "dist/.stage-$version"
echo "packed dist/$name.tar.gz"
