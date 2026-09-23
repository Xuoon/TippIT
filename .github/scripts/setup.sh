#!/usr/bin/env bash
# Ein Einstieg für Cloud-Agenten (Claude, Codex, Cursor) und Devcontainer.
# Linux gilt als Cloud-Container und erhält fehlende Toolchains; andere Systeme
# werden nur geprüft. Keine Server, keine Builds, keine Signatur.
# Rust bleibt bewusst außen vor: `src-tauri/src/platform/` gibt es nur für
# Windows und macOS, ein Linux-Container kann die App nicht kompilieren.
# Rust-Prüfungen laufen in der CI (ci.yml).
set -euo pipefail

cd "$(dirname "${BASH_SOURCE[0]}")/../.."
node_major="$(tr -d '[:space:]' < .node-version)"
[[ "$node_major" =~ ^[0-9]+$ ]] || { echo "Ungültige .node-version" >&2; exit 1; }
cloud=false
[[ "$(uname -s)" == Linux ]] && cloud=true
node_dir="$HOME/.local/toolchains/node-$node_major"
export BUN_INSTALL="${BUN_INSTALL:-$HOME/.bun}"
if $cloud; then
  export PATH="$node_dir/bin:$BUN_INSTALL/bin:$PATH"
fi

as_root() { if [[ "$(id -u)" == 0 ]]; then "$@"; else sudo "$@"; fi; }
fetch() { curl --fail --silent --show-error --location --retry 3 --connect-timeout 10 "$@"; }
tmp=""
trap '[[ -n "$tmp" ]] && rm -rf -- "$tmp"' EXIT
download_dir() { [[ -n "$tmp" ]] || tmp="$(mktemp -d)"; }

# Systempakete: Cloud-Basisimages bringen nicht immer Entpacker und CA-Zertifikate mit.
if $cloud; then
  packages=(ca-certificates curl unzip xz-utils)
  missing=()
  for package in "${packages[@]}"; do
    [[ "$(dpkg-query -W -f='${Status}' "$package" 2>/dev/null || true)" == 'install ok installed' ]] || missing+=("$package")
  done
  if ((${#missing[@]})); then
    as_root apt-get update -qq
    as_root env DEBIAN_FRONTEND=noninteractive apt-get install -y --no-install-recommends "${missing[@]}"
  fi
fi

# Node aus .node-version
if [[ "$(node --version 2>/dev/null || true)" != "v$node_major."* ]]; then
  $cloud || { printf 'Node.js %s wird benötigt (siehe .node-version).\n' "$node_major" >&2; exit 1; }
  case "$(uname -m)" in
    x86_64) arch=x64 ;;
    aarch64|arm64) arch=arm64 ;;
    *) echo "Nicht unterstützte Architektur." >&2; exit 1 ;;
  esac
  download_dir
  base="https://nodejs.org/dist/latest-v$node_major.x"
  fetch "$base/SHASUMS256.txt" -o "$tmp/SHASUMS256.txt"
  archive="$(awk -v arch="$arch" '$2 ~ ("^node-v[0-9.]+-linux-" arch "[.]tar[.]xz$") {print $2}' "$tmp/SHASUMS256.txt")"
  [[ -n "$archive" && "$archive" != *$'\n'* ]] || { echo "Node-Download nicht eindeutig." >&2; exit 1; }
  fetch "$base/$archive" -o "$tmp/$archive"
  (cd "$tmp" && awk -v archive="$archive" '$2 == archive' SHASUMS256.txt | sha256sum --check --strict)
  mkdir -p "$tmp/node" "$(dirname "$node_dir")"
  tar -xJf "$tmp/$archive" --strip-components=1 -C "$tmp/node"
  rm -rf "$node_dir"
  mv "$tmp/node" "$node_dir"
fi

# Bun aus packageManager
manager="$(node -p 'require("./package.json").packageManager')"
[[ "$manager" =~ ^bun@[0-9]+\.[0-9]+\.[0-9]+$ ]] || { echo "packageManager muss eine exakte Bun-Version nennen." >&2; exit 1; }
bun_version="${manager#bun@}"
if [[ "$(bun --version 2>/dev/null || true)" != "$bun_version" ]]; then
  $cloud || { printf 'Bun %s wird benötigt (siehe package.json).\n' "$bun_version" >&2; exit 1; }
  # Release-Archiv von GitHub gegen dessen SHASUMS256 prüfen, statt ein
  # veränderliches Installationsskript auszuführen (github.com steht zudem auf
  # der Allowlist der Cloud-Umgebungen, bun.sh nicht).
  case "$(uname -m)" in
    x86_64) bun_asset=bun-linux-x64 ;;
    aarch64|arm64) bun_asset=bun-linux-aarch64 ;;
    *) echo "Nicht unterstützte Architektur." >&2; exit 1 ;;
  esac
  download_dir
  bun_base="https://github.com/oven-sh/bun/releases/download/bun-v$bun_version"
  fetch "$bun_base/SHASUMS256.txt" -o "$tmp/bun-SHASUMS256.txt"
  fetch "$bun_base/$bun_asset.zip" -o "$tmp/$bun_asset.zip"
  (cd "$tmp" && awk -v archive="$bun_asset.zip" '$2 == archive' bun-SHASUMS256.txt | sha256sum --check --strict)
  unzip -oq "$tmp/$bun_asset.zip" -d "$tmp"
  mkdir -p "$BUN_INSTALL/bin"
  install -m 0755 "$tmp/$bun_asset/bun" "$BUN_INSTALL/bin/bun"
  hash -r
  [[ "$(bun --version)" == "$bun_version" ]] || { echo "Falsche Bun-Version nach Installation." >&2; exit 1; }
fi

# Exports überleben Snapshots nicht; neue nicht-interaktive Shells finden die
# Programme über /usr/local/bin, Claude-Sessions über CLAUDE_ENV_FILE.
if $cloud; then
  as_root mkdir -p /usr/local/bin
  for program in node npm npx bun; do
    program_path="$(command -v "$program" || true)"
    if [[ -n "$program_path" && "$program_path" != "/usr/local/bin/$program" ]]; then
      as_root ln -sfn "$program_path" "/usr/local/bin/$program"
    fi
  done
fi
if [[ -n "${CLAUDE_ENV_FILE:-}" ]]; then
  printf 'export PATH=%q\nexport BUN_INSTALL=%q\n' "$PATH" "$BUN_INSTALL" >> "$CLAUDE_ENV_FILE"
fi

NODE_ENV=development bun install --frozen-lockfile
printf 'Setup abgeschlossen: Node %s, Bun %s.\n' "$(node --version)" "$bun_version"
