#!/bin/bash
set -euo pipefail

# Builds the testnet image with circuits. Prefers a local circuits bundle
# (tests/kzgrs/kzgrs_test_params) or a custom override; otherwise downloads
# from logos-co/nomos-circuits.

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../../.." && pwd)"
if [ -f "${ROOT_DIR}/versions.env" ]; then
  # shellcheck disable=SC1091
  . "${ROOT_DIR}/versions.env"
fi
if [ -f "${ROOT_DIR}/paths.env" ]; then
  # shellcheck disable=SC1091
  . "${ROOT_DIR}/paths.env"
fi
DOCKERFILE_PATH="${ROOT_DIR}/testing-framework/assets/stack/Dockerfile"
IMAGE_TAG="${IMAGE_TAG:-logos-blockchain-testing:local}"
VERSION="${VERSION:-v0.3.1}"
KZG_DIR_REL="${NOMOS_KZG_DIR_REL:-testing-framework/assets/stack/kzgrs_test_params}"
CIRCUITS_OVERRIDE="${CIRCUITS_OVERRIDE:-${KZG_DIR_REL}}"
CIRCUITS_PLATFORM="${CIRCUITS_PLATFORM:-${COMPOSE_CIRCUITS_PLATFORM:-}}"
if [ -z "${CIRCUITS_PLATFORM}" ]; then
  case "$(uname -m)" in
    x86_64) CIRCUITS_PLATFORM="linux-x86_64" ;;
    arm64|aarch64) CIRCUITS_PLATFORM="linux-aarch64" ;;
    *) CIRCUITS_PLATFORM="linux-x86_64" ;;
  esac
fi
NOMOS_NODE_REV="${NOMOS_NODE_REV:-d2dd5a5084e1daef4032562c77d41de5e4d495f8}"

echo "Workspace root: ${ROOT_DIR}"
echo "Image tag: ${IMAGE_TAG}"
echo "Circuits override: ${CIRCUITS_OVERRIDE:-<none>}"
echo "Circuits version (fallback download): ${VERSION}"
echo "Circuits platform: ${CIRCUITS_PLATFORM}"
echo "Bundle tar (if used): ${NOMOS_BINARIES_TAR:-<default>.tmp/nomos-binaries-linux-${VERSION}.tar.gz}"

# If prebuilt binaries are missing, restore them from a bundle tarball instead of
# rebuilding nomos inside the image.
BIN_DST="${ROOT_DIR}/testing-framework/assets/stack/bin"
DEFAULT_LINUX_TAR="${ROOT_DIR}/.tmp/nomos-binaries-linux-${VERSION}.tar.gz"
TAR_PATH="${NOMOS_BINARIES_TAR:-${DEFAULT_LINUX_TAR}}"

if [ ! -x "${BIN_DST}/nomos-node" ] || [ ! -x "${BIN_DST}/nomos-executor" ]; then
  if [ -f "${TAR_PATH}" ]; then
    echo "Restoring binaries/circuits from ${TAR_PATH}"
    tmp_extract="$(mktemp -d)"
    tar -xzf "${TAR_PATH}" -C "${tmp_extract}"
    if [ -f "${tmp_extract}/artifacts/nomos-node" ] && [ -f "${tmp_extract}/artifacts/nomos-executor" ]; then
      mkdir -p "${BIN_DST}"
      cp "${tmp_extract}/artifacts/nomos-node" "${tmp_extract}/artifacts/nomos-executor" "${tmp_extract}/artifacts/nomos-cli" "${BIN_DST}/"
    else
      echo "ERROR: Bundle ${TAR_PATH} missing binaries under artifacts/" >&2
      exit 1
    fi
    if [ -d "${tmp_extract}/artifacts/circuits" ]; then
      mkdir -p "${KZG_DIR_REL}"
      rsync -a --delete "${tmp_extract}/artifacts/circuits/" "${KZG_DIR_REL}/"
    fi
    rm -rf "${tmp_extract}"
  else
    echo "ERROR: Prebuilt binaries missing and bundle tar not found at ${TAR_PATH}" >&2
    exit 1
  fi
fi

build_args=(
  -f "${DOCKERFILE_PATH}"
  -t "${IMAGE_TAG}"
  --build-arg "NOMOS_NODE_REV=${NOMOS_NODE_REV}"
  --build-arg "CIRCUITS_PLATFORM=${CIRCUITS_PLATFORM}"
  "${ROOT_DIR}"
)

# Pass override/version args to the Docker build.
if [ -n "${CIRCUITS_OVERRIDE}" ]; then
  build_args+=(--build-arg "CIRCUITS_OVERRIDE=${CIRCUITS_OVERRIDE}")
fi
build_args+=(--build-arg "VERSION=${VERSION}")

echo "Running: docker build ${build_args[*]}"
docker build "${build_args[@]}"

cat <<EOF

Build complete.
- Use this image in k8s/compose by exporting NOMOS_TESTNET_IMAGE=${IMAGE_TAG}
- Circuits source: ${CIRCUITS_OVERRIDE:-download ${VERSION}}
EOF
