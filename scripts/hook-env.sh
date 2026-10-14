# SPDX-License-Identifier: Apache-2.0
# Sourced by every git hook (lefthook `rc`, see lefthook.yml) before the checks run.
#
# Git apps such as VS Code, Fork or Tower start hooks without reading ~/.zshrc, so
# Homebrew, rustup and the Node version from .nvmrc may be missing from PATH. This
# file adds them. POSIX sh, no output, safe to source more than once.

_schnipsle_prepend() {
  if [ -d "$1" ]; then
    case ":$PATH:" in
      *":$1:"*) ;;
      *) PATH="$1:$PATH" ;;
    esac
  fi
}

# Homebrew (Apple silicon, then Intel)
_schnipsle_prepend /usr/local/bin
_schnipsle_prepend /opt/homebrew/bin

# Rust: Homebrew's keg-only rustup proxies, then cargo-installed tools
_schnipsle_prepend /usr/local/opt/rustup/bin
_schnipsle_prepend /opt/homebrew/opt/rustup/bin
_schnipsle_prepend "$HOME/.cargo/bin"
export PATH

# Node: activate the version pinned in .nvmrc (hooks run in the repository root)
if command -v fnm >/dev/null 2>&1; then
  eval "$(fnm env --shell bash)"
  fnm use --silent-if-unchanged >/dev/null 2>&1 || fnm use --install-if-missing >/dev/null 2>&1 || true
fi

unset -f _schnipsle_prepend
