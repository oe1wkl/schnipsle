#!/usr/bin/env bash
# SPDX-License-Identifier: Apache-2.0
#
# Regenerates every app icon from the SVG masters in src-tauri/icons/source/.
# Run it on a Mac after editing a master:
#
#   scripts/make-icons.sh
#
# What it does:
#   1. renders both masters to 1024 x 1024 PNGs with the Tauri CLI (resvg inside)
#   2. `tauri icon` on the macOS PNG writes src-tauri/icons/ (icns, ico, PNGs)
#      and the iOS AppIcon set in src-tauri/gen/apple/Assets.xcassets
#   3. replaces that iOS set with one made from the full-bleed master, since iOS
#      masks the icon itself, and removes the alpha channel App Store Connect rejects
#   4. drops the Android icons `tauri icon` adds; there is no Android project yet
set -euo pipefail

cd "$(dirname "$0")/.."
source scripts/hook-env.sh

src=src-tauri/icons/source
appiconset=src-tauri/gen/apple/Assets.xcassets/AppIcon.appiconset
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT

pnpm -s tauri icon "$src/schnipsle-macos.svg" -p 1024 -o "$tmp/macos"
pnpm -s tauri icon "$src/schnipsle.svg" -p 1024 -o "$tmp/ios"

pnpm -s tauri icon "$tmp/macos/1024x1024.png"

pnpm -s tauri icon "$tmp/ios/1024x1024.png" -o "$tmp/ios-set"
cp "$tmp"/ios-set/ios/AppIcon-*.png "$appiconset/"
swift scripts/strip-png-alpha.swift "$appiconset"/AppIcon-*.png

rm -rf src-tauri/icons/android
