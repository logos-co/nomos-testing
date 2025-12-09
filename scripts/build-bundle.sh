#!/usr/bin/env bash
set -euo pipefail

# Build a nomos-binaries.tar.gz for the specified platform.
#
# Usage: scripts/build-bundle.sh [--platform host|linux] [--output PATH]
#   --platform   Target platform for binaries (default: host)
#   --output     Output path for the tarball (default: .tmp/nomos-binaries-<platform>-<version>.tar.gz)

usage() {
  cat <<'EOF'
Usage: scripts/build-bundle.sh [--platform host|linux] [--output PATH]

Options:
  --platform   Target platform for binaries (default: host)
  --output     Output path for the tarball (default: .tmp/nomos-binaries-<platform>-<version>.tar.gz)

Notes:
  - For compose/k8s, use platform=linux. If running on macOS, this script will
    run inside a Linux Docker container to produce Linux binaries.
  - VERSION and NOMOS_NODE_REV env vars are honored (defaults align with run-examples.sh).
EOF
}

fail() { echo "$1" >&2; exit 1; }

if [ "${1:-}" = "-h" ] || [ "${1:-}" = "--help" ]; then
  usage; exit 0
fi

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
DEFAULT_VERSION="v0.3.1"
DEFAULT_NODE_REV="d2dd5a5084e1daef4032562c77d41de5e4d495f8"
PLATFORM="host"
OUTPUT=""

while [ "$#" -gt 0 ]; do
  case "$1" in
    --platform)
      PLATFORM="${2:-}"; shift 2 ;;
    --output)
      OUTPUT="${2:-}"; shift 2 ;;
    *) fail "Unknown argument: $1" ;;
  esac
done

case "$PLATFORM" in
  host|linux) ;;
  *) fail "--platform must be host or linux" ;;
esac

VERSION="${VERSION:-${DEFAULT_VERSION}}"
NOMOS_NODE_REV="${NOMOS_NODE_REV:-${DEFAULT_NODE_REV}}"
if [ -z "${OUTPUT}" ]; then
  OUTPUT="${ROOT_DIR}/.tmp/nomos-binaries-${PLATFORM}-${VERSION}.tar.gz"
fi

if [ "$PLATFORM" = "linux" ] && [ "$(uname -s)" != "Linux" ] && [ -z "${BUNDLE_IN_CONTAINER:-}" ]; then
  # Re-run inside a Linux container to produce Linux binaries.
  if ! command -v docker >/dev/null 2>&1; then
    fail "Docker is required to build a Linux bundle from non-Linux host"
  fi
  echo "==> Building Linux bundle inside Docker"
  mkdir -p "${ROOT_DIR}/.tmp/cargo-linux" "${ROOT_DIR}/.tmp/nomos-node-linux-target"
  docker run --rm \
    -e VERSION="$VERSION" \
    -e NOMOS_NODE_REV="$NOMOS_NODE_REV" \
    -e BUNDLE_IN_CONTAINER=1 \
    -e CARGO_HOME=/workspace/.tmp/cargo-linux \
    -e CARGO_TARGET_DIR=/workspace/.tmp/nomos-node-linux-target \
    -v "${ROOT_DIR}/.tmp/cargo-linux":/workspace/.tmp/cargo-linux \
    -v "${ROOT_DIR}/.tmp/nomos-node-linux-target":/workspace/.tmp/nomos-node-linux-target \
    -v "$ROOT_DIR":/workspace \
    -w /workspace \
    rust:1.80-bullseye \
    bash -c "apt-get update && apt-get install -y clang llvm-dev libclang-dev pkg-config cmake libssl-dev rsync libgmp10 libgmp-dev libgomp1 nasm && ./scripts/build-bundle.sh --platform linux --output /workspace/.tmp/nomos-binaries-linux-${VERSION}.tar.gz"
  exit 0
fi

echo "==> Preparing circuits (version ${VERSION})"
HOST_BUNDLE_PATH="${ROOT_DIR}/testing-framework/assets/stack/kzgrs_test_params"
mkdir -p "${ROOT_DIR}/.tmp"
"${ROOT_DIR}/scripts/setup-circuits-stack.sh" "${VERSION}" </dev/null

HOST_SRC="${ROOT_DIR}/.tmp/nomos-node-host-src"
HOST_TARGET="${ROOT_DIR}/.tmp/nomos-node-host-target"
HOST_NODE_BIN="${HOST_TARGET}/debug/nomos-node"
HOST_EXEC_BIN="${HOST_TARGET}/debug/nomos-executor"
HOST_CLI_BIN="${HOST_TARGET}/debug/nomos-cli"

echo "==> Building host binaries (platform=${PLATFORM})"
mkdir -p "${HOST_SRC}"
if [ ! -d "${HOST_SRC}/.git" ]; then
  git clone https://github.com/logos-co/nomos-node.git "${HOST_SRC}"
fi
(
  cd "${HOST_SRC}"
  git fetch --depth 1 origin "${NOMOS_NODE_REV}"
  git checkout "${NOMOS_NODE_REV}"
  git reset --hard
  git clean -fdx
  RUSTFLAGS='--cfg feature="pol-dev-mode"' NOMOS_CIRCUITS="${HOST_BUNDLE_PATH}" \
    cargo build --features testing \
    -p nomos-node -p nomos-executor -p nomos-cli \
    --target-dir "${HOST_TARGET}"
)

echo "==> Packaging bundle"
bundle_dir="${ROOT_DIR}/.tmp/nomos-bundle"
rm -rf "${bundle_dir}"
mkdir -p "${bundle_dir}/artifacts/circuits"
cp -a "${HOST_BUNDLE_PATH}/." "${bundle_dir}/artifacts/circuits/"
mkdir -p "${bundle_dir}/artifacts"
cp "${HOST_NODE_BIN}" "${bundle_dir}/artifacts/"
cp "${HOST_EXEC_BIN}" "${bundle_dir}/artifacts/"
cp "${HOST_CLI_BIN}" "${bundle_dir}/artifacts/"

mkdir -p "$(dirname "${OUTPUT}")"
tar --no-mac-metadata --no-xattrs -czf "${OUTPUT}" -C "${bundle_dir}" artifacts
echo "Bundle created at ${OUTPUT}"
