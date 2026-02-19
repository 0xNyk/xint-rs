#!/usr/bin/env bash
set -euo pipefail

OWNER="0xNyk"
REPO="xint-rs"
BIN_NAME="xint"

INSTALL_BIN_DIR="${XINT_RS_INSTALL_BIN_DIR:-$HOME/.local/bin}"
REQUESTED_VERSION="${XINT_RS_INSTALL_VERSION:-latest}"
REQUIRE_CHECKSUM="${XINT_RS_INSTALL_REQUIRE_CHECKSUM:-0}"

require_cmd() {
  if ! command -v "$1" >/dev/null 2>&1; then
    echo "error: missing required command '$1'" >&2
    exit 1
  fi
}

detect_platform() {
  local os arch
  os="$(uname -s | tr '[:upper:]' '[:lower:]')"
  arch="$(uname -m)"

  case "$os" in
    darwin|linux) ;;
    *)
      echo "error: unsupported OS '$os'" >&2
      exit 1
      ;;
  esac

  case "$arch" in
    x86_64|amd64) arch="x86_64" ;;
    arm64|aarch64) arch="aarch64" ;;
    *)
      echo "error: unsupported architecture '$arch'" >&2
      exit 1
      ;;
  esac

  printf '%s %s' "$os" "$arch"
}

release_api_url() {
  if [[ "$REQUESTED_VERSION" == "latest" ]]; then
    printf 'https://api.github.com/repos/%s/%s/releases/latest' "$OWNER" "$REPO"
  else
    printf 'https://api.github.com/repos/%s/%s/releases/tags/%s' "$OWNER" "$REPO" "$REQUESTED_VERSION"
  fi
}

select_assets() {
  local release_json="$1"
  local os="$2"
  local arch="$3"

  python3 - "$release_json" "$os" "$arch" <<'PY'
import json
import os
import sys

release_path, os_name, arch_name = sys.argv[1], sys.argv[2], sys.argv[3]
with open(release_path, "r", encoding="utf-8") as fh:
    release = json.load(fh)

assets = release.get("assets", [])
tag = release.get("tag_name", "")

asset_map = {a.get("name"): a.get("browser_download_url") for a in assets if a.get("name")}
names = [a.get("name", "") for a in assets]

candidates = [
    f"xint-{os_name}-{arch_name}",
    f"xint_{os_name}_{arch_name}",
    f"xint-{arch_name}-{os_name}",
    "xint",
]
suffixes = ["", ".tar.gz", ".tgz", ".zip"]

selected_name = None
for base in candidates:
    for suffix in suffixes:
        name = f"{base}{suffix}"
        if name in asset_map:
            selected_name = name
            break
    if selected_name:
        break

if not selected_name:
    for name in names:
        if name.startswith("xint") and "checksum" not in name and "sha256" not in name:
            selected_name = name
            break

checksum_name = None
for name in names:
    lowered = name.lower()
    if "checksum" in lowered or "sha256" in lowered:
        checksum_name = name
        break

if not selected_name:
    raise SystemExit("no_install_asset")

print(tag)
print(selected_name)
print(asset_map[selected_name])
print(checksum_name or "")
print(asset_map.get(checksum_name, "") if checksum_name else "")
PY
}

verify_checksum_if_available() {
  local asset_file="$1"
  local checksums_file="$2"
  local asset_name="$3"

  if [[ ! -f "$checksums_file" ]]; then
    if [[ "$REQUIRE_CHECKSUM" == "1" ]]; then
      echo "error: checksum required but checksums file not provided" >&2
      exit 1
    fi
    echo "==> No checksums asset found; skipping checksum verification"
    return
  fi

  local expected
  expected="$(awk -v name="$asset_name" '$0 ~ name {print $1; exit}' "$checksums_file" || true)"
  if [[ -z "$expected" ]]; then
    if [[ "$REQUIRE_CHECKSUM" == "1" ]]; then
      echo "error: checksum required but asset entry not found in checksums file" >&2
      exit 1
    fi
    echo "==> Checksums file present, but no entry for $asset_name; skipping verification"
    return
  fi

  local actual=""
  if command -v sha256sum >/dev/null 2>&1; then
    actual="$(sha256sum "$asset_file" | awk '{print $1}')"
  elif command -v shasum >/dev/null 2>&1; then
    actual="$(shasum -a 256 "$asset_file" | awk '{print $1}')"
  else
    if [[ "$REQUIRE_CHECKSUM" == "1" ]]; then
      echo "error: checksum required but neither sha256sum nor shasum is available" >&2
      exit 1
    fi
    echo "==> Checksum tool unavailable; skipping verification"
    return
  fi

  if [[ "$actual" != "$expected" ]]; then
    echo "error: checksum mismatch for $asset_name" >&2
    exit 1
  fi
  echo "==> Checksum verified"
}

extract_binary() {
  local asset_file="$1"
  local asset_name="$2"
  local workdir="$3"

  local out_file="${workdir}/${BIN_NAME}"
  case "$asset_name" in
    *.tar.gz|*.tgz)
      local extract_dir="${workdir}/extract"
      mkdir -p "$extract_dir"
      tar -xzf "$asset_file" -C "$extract_dir"
      local found
      found="$(find "$extract_dir" -type f -name "$BIN_NAME" | head -n1)"
      if [[ -z "$found" ]]; then
        echo "error: binary '$BIN_NAME' not found in archive $asset_name" >&2
        exit 1
      fi
      cp "$found" "$out_file"
      ;;
    *.zip)
      require_cmd unzip
      local extract_dir="${workdir}/extract"
      mkdir -p "$extract_dir"
      unzip -q "$asset_file" -d "$extract_dir"
      local found
      found="$(find "$extract_dir" -type f -name "$BIN_NAME" | head -n1)"
      if [[ -z "$found" ]]; then
        echo "error: binary '$BIN_NAME' not found in archive $asset_name" >&2
        exit 1
      fi
      cp "$found" "$out_file"
      ;;
    *)
      cp "$asset_file" "$out_file"
      ;;
  esac

  chmod +x "$out_file"
  printf '%s' "$out_file"
}

main() {
  require_cmd curl
  require_cmd python3

  read -r os arch <<<"$(detect_platform)"

  local tmpdir
  tmpdir="$(mktemp -d)"
  trap 'rm -rf "$tmpdir"' EXIT

  local release_json="${tmpdir}/release.json"
  echo "==> Fetching release metadata"
  curl -fsSL "$(release_api_url)" -o "$release_json"

  local tag asset_name asset_url checksum_name checksum_url
  mapfile -t meta < <(select_assets "$release_json" "$os" "$arch")
  if [[ "${#meta[@]}" -lt 5 ]]; then
    echo "error: failed to resolve release assets for ${os}/${arch}" >&2
    exit 1
  fi
  tag="${meta[0]}"
  asset_name="${meta[1]}"
  asset_url="${meta[2]}"
  checksum_name="${meta[3]}"
  checksum_url="${meta[4]}"

  local asset_file="${tmpdir}/${asset_name}"
  echo "==> Downloading ${asset_name} (${tag})"
  curl -fsSL "$asset_url" -o "$asset_file"

  local checksums_file=""
  if [[ -n "$checksum_url" ]]; then
    checksums_file="${tmpdir}/${checksum_name}"
    curl -fsSL "$checksum_url" -o "$checksums_file"
  fi

  verify_checksum_if_available "$asset_file" "$checksums_file" "$asset_name"

  local extracted_bin
  extracted_bin="$(extract_binary "$asset_file" "$asset_name" "$tmpdir")"

  mkdir -p "$INSTALL_BIN_DIR"
  local final_bin="${INSTALL_BIN_DIR}/${BIN_NAME}"
  install -m 755 "$extracted_bin" "$final_bin"

  echo "==> Installed ${BIN_NAME} ${tag}"
  echo "   Binary: ${final_bin}"
  if [[ ":$PATH:" != *":${INSTALL_BIN_DIR}:"* ]]; then
    echo "   Add to PATH: export PATH=\"${INSTALL_BIN_DIR}:\$PATH\""
  fi
}

main "$@"
