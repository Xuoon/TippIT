#!/usr/bin/env bash
# Cloud Agent install for TippIT.
#
# TippIT is a Windows/macOS-only Tauri v2 desktop app: the Rust backend in
# src-tauri/ is gated to those platforms (src-tauri/src/platform/ only wires
# win.rs / mac.rs) and CI runs cargo exclusively on windows-latest/macos-latest.
# On a Linux Cloud Agent the meaningful, OS-independent development surface is
# the frontend toolchain (Bun + Vite + SvelteKit) plus the lint/typecheck the CI
# frontend leg runs. This script prepares exactly that. It does not attempt to
# build the Rust backend, which cannot compile on Linux.
set -euo pipefail

# Bun, pinned to package.json's "packageManager" field.
BUN_VERSION="1.3.3"
if ! command -v bun >/dev/null 2>&1 || [ "$(bun --version 2>/dev/null || true)" != "$BUN_VERSION" ]; then
  curl -fsSL https://bun.sh/install | bash -s "bun-v${BUN_VERSION}"
fi
export BUN_INSTALL="${BUN_INSTALL:-$HOME/.bun}"
export PATH="$BUN_INSTALL/bin:$PATH"

# Expose bun on PATH for every (non-login) shell the agent spawns.
if command -v sudo >/dev/null 2>&1 && sudo -n true 2>/dev/null; then
  sudo ln -sf "$BUN_INSTALL/bin/bun" /usr/local/bin/bun
  sudo ln -sf "$BUN_INSTALL/bin/bunx" /usr/local/bin/bunx
fi

# Frontend dependencies. "prepare" runs `svelte-kit sync`, generating .svelte-kit/.
bun install --frozen-lockfile

# Build the SPA: validates the frontend build and produces build/ + .svelte-kit/.
bun run build
