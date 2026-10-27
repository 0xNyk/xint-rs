#!/usr/bin/env bash
# xint-rs installer
#
#   curl --proto '=https' --tlsv1.2 -fsSL https://raw.githubusercontent.com/0xNyk/xint-rs/main/install.sh | bash
#   curl --proto '=https' --tlsv1.2 -fsSL https://raw.githubusercontent.com/0xNyk/xint-rs/main/install.sh | bash -s -- --version 2026.5.16
#
# Flags:
#   --version <tag>     release to install (default: the pinned DEFAULT_VERSION below; "latest" allowed)
#   --bin-dir <dir>     default: ~/.local/bin
#   --dry-run           print what would be downloaded and verified, then exit
#   -h, --help
#
# Options (env):
#   XINT_RS_INSTALL_VERSION=<tag>        same as --version
#   XINT_RS_INSTALL_BIN_DIR=<dir>        same as --bin-dir
#   XINT_RS_VERIFY_ATTESTATION=auto      auto | require | skip  (GitHub artifact attestation via `gh`)
#
# Every archive is verified against a SHA-256 before it is unpacked: the digest
# pinned in this script for older releases, otherwise the SHA256SUMS file
# published with the same release. A missing or mismatched checksum aborts.
# (XINT_RS_INSTALL_REQUIRE_CHECKSUM is obsolete: checksums are always required.)
set -euo pipefail

OWNER="0xNyk"
REPO="xint-rs"
BIN_NAME="xint"
# Bump together with each release (and drop it from pinned_sha256 once that release ships SHA256SUMS).
DEFAULT_VERSION="2026.5.16"
SIGNER_WORKFLOW="${OWNER}/${REPO}/.github/workflows/release.yml"

INSTALL_BIN_DIR="${XINT_RS_INSTALL_BIN_DIR:-$HOME/.local/bin}"
REQUESTED_VERSION="${XINT_RS_INSTALL_VERSION:-$DEFAULT_VERSION}"
ATTEST_MODE="${XINT_RS_VERIFY_ATTESTATION:-auto}"
DRY_RUN=0

die() {
  echo "error: $*" >&2
  exit 1
}

usage() {
  cat <<'USAGE'
usage: install.sh [--version <tag>|latest] [--bin-dir <dir>] [--dry-run]

env: XINT_RS_INSTALL_VERSION, XINT_RS_INSTALL_BIN_DIR,
     XINT_RS_VERIFY_ATTESTATION=auto|require|skip
USAGE
}

require_cmd() {
  command -v "$1" >/dev/null 2>&1 || die "missing required command '$1'"
}

curl_https() {
  curl --proto '=https' --proto-redir '=https' --tlsv1.2 -fsSL --retry 3 "$@"
}

sha256_file() {
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$1" | awk '{print $1}'
  elif command -v shasum >/dev/null 2>&1; then
    shasum -a 256 "$1" | awk '{print $1}'
  else
    die "need sha256sum or shasum to verify the download"
  fi
}

# Releases published before the release workflow emitted SHA256SUMS and
# attestations. Digest matches the one pinned in the 0xNyk/homebrew-xint formula.
pinned_sha256() {
  case "$1" in
    xint-rs-macos-arm64-2026.5.16.tar.gz) echo "8253b3a3e13ebdc26674ab7a6b753eb9869b506cd37a204a589b867b0407b5be" ;;
    *) return 1 ;;
  esac
}

# Print the digest listed for $2 in checksum file $1 (exact filename match).
checksum_entry() {
  awk -v name="$2" '($2 == name || $2 == "*" name) { print tolower($1); exit }' "$1"
}

is_sha256() {
  [[ "$1" =~ ^[0-9a-f]{64}$ ]]
}

detect_platform() {
  local os arch
  os="$(uname -s)"
  arch="$(uname -m)"

  case "$os" in
    Darwin) os="macos" ;;
    Linux) os="linux" ;;
    *) die "unsupported OS '$os'" ;;
  esac

  case "$arch" in
    x86_64|amd64) arch="x86_64" ;;
    arm64|aarch64) arch="arm64" ;;
    *) die "unsupported architecture '$arch'" ;;
  esac

  printf '%s-%s' "$os" "$arch"
}

resolve_tag() {
  local effective
  if [[ "$REQUESTED_VERSION" == "latest" ]]; then
    effective="$(curl_https -o /dev/null -w '%{url_effective}' "https://github.com/${OWNER}/${REPO}/releases/latest")" \
      || die "could not resolve latest release"
    printf '%s' "${effective##*/}"
  else
    printf '%s' "$REQUESTED_VERSION"
  fi
}

verify_attestation() {
  local file="$1" tag="$2" pinned="$3"

  if [[ "$ATTEST_MODE" == "skip" ]]; then
    echo "==> Attestation skipped (XINT_RS_VERIFY_ATTESTATION=skip)"
    return
  fi
  if [[ -n "$pinned" ]]; then
    echo "==> No attestation for ${tag} (predates attestations; pinned digest checked)"
    return
  fi
  if command -v gh >/dev/null 2>&1 && gh auth status >/dev/null 2>&1; then
    gh attestation verify "$file" \
      --repo "${OWNER}/${REPO}" \
      --signer-workflow "$SIGNER_WORKFLOW" \
      --source-ref "refs/tags/${tag}" \
      --deny-self-hosted-runners >/dev/null \
      || die "GitHub attestation verification failed for $(basename "$file")"
    echo "==> Attestation verified (${SIGNER_WORKFLOW} @ ${tag})"
  elif [[ "$ATTEST_MODE" == "require" ]]; then
    die "XINT_RS_VERIFY_ATTESTATION=require needs an authenticated 'gh' CLI"
  else
    echo "==> Attestation skipped (install and log in to 'gh' to verify provenance)"
  fi
}

main() {
  while [[ "$#" -gt 0 ]]; do
    case "$1" in
      --version) [[ "$#" -ge 2 ]] || die "--version needs a value"; REQUESTED_VERSION="$2"; shift 2 ;;
      --version=*) REQUESTED_VERSION="${1#--version=}"; shift ;;
      --bin-dir) [[ "$#" -ge 2 ]] || die "--bin-dir needs a value"; INSTALL_BIN_DIR="$2"; shift 2 ;;
      --bin-dir=*) INSTALL_BIN_DIR="${1#--bin-dir=}"; shift ;;
      --dry-run) DRY_RUN=1; shift ;;
      -h|--help) usage; exit 0 ;;
      *) die "unknown argument: $1 (try --help)" ;;
    esac
  done

  case "$ATTEST_MODE" in
    auto|require|skip) ;;
    *) die "XINT_RS_VERIFY_ATTESTATION must be auto, require, or skip" ;;
  esac

  require_cmd uname
  require_cmd curl

  local platform tag
  platform="$(detect_platform)"
  tag="$(resolve_tag)"
  [[ "$tag" =~ ^[0-9]+\.[0-9]+\.[0-9]+(\.[0-9]+)?$ ]] \
    || die "invalid release tag '${tag}' (expected YYYY.M.D[.N] or latest)"

  local asset_name="${REPO}-${platform}-${tag}.tar.gz"
  local base_url="https://github.com/${OWNER}/${REPO}/releases/download/${tag}"
  local pinned=""
  pinned="$(pinned_sha256 "$asset_name")" || pinned=""
  if [[ -n "$pinned" && "$ATTEST_MODE" == "require" ]]; then
    die "${tag} predates release attestations; pick a newer --version"
  fi

  if [[ "$DRY_RUN" == "1" ]]; then
    echo "==> Dry run: ${BIN_NAME} ${tag} for ${platform}"
    echo "   archive:   ${base_url}/${asset_name}"
    if [[ -n "$pinned" ]]; then
      echo "   sha256:    ${pinned} (pinned in installer)"
    else
      echo "   checksums: ${base_url}/SHA256SUMS (required)"
    fi
    echo "   attestation mode: ${ATTEST_MODE} (signer ${SIGNER_WORKFLOW})"
    echo "   dest:      ${INSTALL_BIN_DIR}/${BIN_NAME}"
    return
  fi

  require_cmd tar
  require_cmd awk

  local tmpdir
  tmpdir="$(mktemp -d "${TMPDIR:-/tmp}/xint-rs-install.XXXXXX")"
  # shellcheck disable=SC2064 # expand now: tmpdir is local to main
  trap "rm -rf -- '${tmpdir}'" EXIT

  local asset_file="${tmpdir}/${asset_name}"
  echo "==> Downloading ${asset_name} (${tag})"
  curl_https "${base_url}/${asset_name}" -o "$asset_file" \
    || die "no prebuilt ${asset_name} in release ${tag}; build from source with: cargo install --git https://github.com/${OWNER}/${REPO} --tag ${tag} --locked"

  local listed="" expected source_label
  if curl_https "${base_url}/SHA256SUMS" -o "${tmpdir}/SHA256SUMS" 2>/dev/null; then
    listed="$(checksum_entry "${tmpdir}/SHA256SUMS" "$asset_name")"
    [[ -n "$listed" || -n "$pinned" ]] || die "SHA256SUMS for ${tag} has no entry for ${asset_name}; refusing to install"
  elif [[ -z "$pinned" ]]; then
    die "release ${tag} publishes no SHA256SUMS; refusing to install an unverified binary"
  fi

  if [[ -n "$pinned" ]]; then
    [[ -z "$listed" || "$listed" == "$pinned" ]] \
      || die "SHA256SUMS disagrees with the pinned digest for ${asset_name}; refusing to install"
    expected="$pinned"
    source_label="pinned digest"
  else
    expected="$listed"
    source_label="SHA256SUMS"
  fi
  is_sha256 "$expected" || die "malformed checksum for ${asset_name}"

  local actual
  actual="$(sha256_file "$asset_file")"
  [[ "$actual" == "$expected" ]] || die "checksum mismatch for ${asset_name} (expected ${expected}, got ${actual})"
  echo "==> Checksum verified (${source_label})"

  verify_attestation "$asset_file" "$tag" "$pinned"

  local extract_dir="${tmpdir}/extract"
  mkdir -p "$extract_dir"
  tar -xzf "$asset_file" -C "$extract_dir"
  local found="${extract_dir}/${BIN_NAME}"
  if [[ ! -f "$found" ]]; then
    found="$(find "$extract_dir" -type f -name "$BIN_NAME" | head -n1)"
  fi
  [[ -n "$found" && -f "$found" ]] || die "binary '${BIN_NAME}' not found in archive ${asset_name}"

  mkdir -p "$INSTALL_BIN_DIR"
  local final_bin="${INSTALL_BIN_DIR}/${BIN_NAME}"
  install -m 755 "$found" "$final_bin"

  echo "==> Installed ${BIN_NAME} ${tag}"
  echo "   Binary: ${final_bin}"
  if [[ ":$PATH:" != *":${INSTALL_BIN_DIR}:"* ]]; then
    echo "   Add to PATH: export PATH=\"${INSTALL_BIN_DIR}:\$PATH\""
  fi
}

main "$@"
