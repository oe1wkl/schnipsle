#!/usr/bin/env bash
# SPDX-License-Identifier: Apache-2.0
#
# Sets up a Mac for Schnipsle development. Safe to run again at any time.
#
#   cd ~/Developer/schnipsle && scripts/bootstrap-macos.sh
#
# What it does (details in docs/dev-setup.md):
#   1. checks that full Xcode is installed (needed for iOS)
#   2. installs Homebrew if missing, then everything in Brewfile
#   3. installs the pinned Rust toolchain and Apple targets (rust-toolchain.toml)
#   4. installs the pinned Node version (.nvmrc) and the JavaScript dependencies
#   5. installs the git hooks
#   6. runs all checks once, so you know the setup works
set -euo pipefail

cd "$(dirname "$0")/.."

step() { printf '\n\033[1;34m==> %s\033[0m\n' "$*"; }
fail() { printf '\n\033[1;31mError:\033[0m %s\n' "$*" >&2; exit 1; }

[[ "$(uname -s)" == "Darwin" ]] || fail "This script is for macOS."

step "Xcode"
if ! xcode_path="$(xcode-select -p 2>/dev/null)" || [[ "$xcode_path" != *Xcode*.app* ]]; then
  fail "Full Xcode is required for iOS builds (Command Line Tools alone are not enough).
  1. Install Xcode from the App Store.
  2. Open it once and let it install its components (including the iOS platform).
  3. Run: sudo xcode-select -s /Applications/Xcode.app/Contents/Developer
  4. Run this script again."
fi
xcodebuild -version | head -n1
if ! xcodebuild -checkFirstLaunchStatus >/dev/null 2>&1; then
  echo "Xcode needs its first-launch setup and licence acceptance:"
  sudo xcodebuild -runFirstLaunch
fi

step "Homebrew"
if ! command -v brew >/dev/null 2>&1; then
  /bin/bash -c "$(curl -fsSL https://raw.githubusercontent.com/Homebrew/install/HEAD/install.sh)"
  eval "$(/opt/homebrew/bin/brew shellenv 2>/dev/null || /usr/local/bin/brew shellenv)"
fi
brew --version | head -n1
brew bundle --file=Brewfile

step "Rust"
# Homebrew's rustup is keg-only; make its proxies available in this shell.
rustup_prefix="$(brew --prefix rustup)"
export PATH="$rustup_prefix/bin:$HOME/.cargo/bin:$PATH"
# Without arguments this installs what rust-toolchain.toml pins: version, components, targets.
rustup toolchain install
rustup show active-toolchain
rustc --version

step "Node and JavaScript dependencies"
eval "$(fnm env --use-on-cd --shell bash)"
fnm use --install-if-missing "$(cat .nvmrc)"
node --version
pnpm install --frozen-lockfile

step "Git hooks"
lefthook install

step "Checks (Rust core, UI)"
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
pnpm check
pnpm build

cat <<'EOF'

Setup complete.

Add these lines to ~/.zshrc so new terminals find the tools:
  export PATH="$(brew --prefix rustup)/bin:$HOME/.cargo/bin:$PATH"
  eval "$(fnm env --use-on-cd --shell zsh)"
  export APPLE_DEVELOPMENT_TEAM=<your team id>   # for iPhone builds, see docs/dev-setup.md §6

Next:
  pnpm tauri dev                     run the desktop app
  pnpm tauri ios dev                 run in the iOS simulator
  pnpm tauri ios dev --open --host   build for your iPhone and open Xcode
EOF
