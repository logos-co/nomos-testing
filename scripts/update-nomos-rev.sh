#!/usr/bin/env bash
set -euo pipefail

# Update nomos-node source across versions.env and Cargo.toml.
# Usage:
#   scripts/update-nomos-rev.sh --rev <git_rev>
#   scripts/update-nomos-rev.sh --path <local_dir>
#
# Only one of --rev/--path may be supplied.

usage() {
  cat <<'EOF'
Usage:
  scripts/update-nomos-rev.sh --rev <git_rev>
  scripts/update-nomos-rev.sh --path <local_dir>

Notes:
  --rev   sets NOMOS_NODE_REV and updates Cargo.toml revs
  --path  sets NOMOS_NODE_PATH (clears NOMOS_NODE_REV) for local checkout
  Only one may be used at a time.
EOF
}

REV=""
LOCAL_PATH=""
while [ "$#" -gt 0 ]; do
  case "$1" in
    --rev) REV="${2:-}"; shift 2 ;;
    --path) LOCAL_PATH="${2:-}"; shift 2 ;;
    -h|--help) usage; exit 0 ;;
    *) echo "Unknown arg: $1" >&2; usage; exit 1 ;;
  esac
done

if [ -n "${REV}" ] && [ -n "${LOCAL_PATH}" ]; then
  echo "Use either --rev or --path, not both" >&2
  usage; exit 1
fi

if [ -z "${REV}" ] && [ -z "${LOCAL_PATH}" ]; then
  usage; exit 1
fi

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

if [ ! -f "${ROOT_DIR}/versions.env" ]; then
  echo "ERROR: versions.env missing; run from repo root." >&2
  exit 1
fi

# Ensure keys exist so sed replacements succeed even if they were absent.
ensure_env_key() {
  local key="$1" default_value="$2"
  if ! grep -Eq "^#?[[:space:]]*${key}=" "${ROOT_DIR}/versions.env"; then
    echo "${default_value}" >> "${ROOT_DIR}/versions.env"
  fi
}
ensure_env_key "NOMOS_NODE_REV" "# NOMOS_NODE_REV="
ensure_env_key "NOMOS_NODE_PATH" "# NOMOS_NODE_PATH="

if [ -n "${REV}" ]; then
  echo "Updating nomos-node rev to ${REV}"
  # Update versions.env NOMOS_NODE_REV entry, clear NOMOS_NODE_PATH if present.
  sed -i.bak -E \
    -e "s/^#?[[:space:]]*NOMOS_NODE_REV=.*/NOMOS_NODE_REV=${REV}/" \
    -e "s/^#?[[:space:]]*NOMOS_NODE_PATH=.*/# NOMOS_NODE_PATH=/" \
    "${ROOT_DIR}/versions.env"
  rm -f "${ROOT_DIR}/versions.env.bak"

  # Update all rev fields in Cargo.toml pointing to nomos-node.
  python3 - "$ROOT_DIR" "$REV" <<'PY'
import pathlib, re, sys
root = pathlib.Path(sys.argv[1])
rev = sys.argv[2]
cargo_toml = root / "Cargo.toml"
txt = cargo_toml.read_text()
txt = txt.replace("\\n", "\n")
txt = re.sub(
    r'(git = "https://github\.com/logos-co/nomos-node\.git", rev = ")[^"]+(")',
    r"\g<1>" + rev + r"\2",
    txt,
)
cargo_toml.write_text(txt.rstrip() + "\n")
PY
else
  echo "Pointing to local nomos-node at ${LOCAL_PATH}"
  if [ ! -d "${LOCAL_PATH}" ]; then
    echo "ERROR: path does not exist: ${LOCAL_PATH}" >&2
    exit 1
  fi
  CURRENT_REV="$(grep -E '^[#[:space:]]*NOMOS_NODE_REV=' "${ROOT_DIR}/versions.env" | head -n1 | sed -E 's/^#?[[:space:]]*NOMOS_NODE_REV=//')"
  # Update versions.env to favor the local path.
  sed -i.bak -E \
    -e "s/^#?[[:space:]]*NOMOS_NODE_PATH=.*/NOMOS_NODE_PATH=${LOCAL_PATH//\//\\/}/" \
    -e "s/^#?[[:space:]]*NOMOS_NODE_REV=.*/# NOMOS_NODE_REV=${CURRENT_REV}/" \
    "${ROOT_DIR}/versions.env"
  rm -f "${ROOT_DIR}/versions.env.bak"
fi

echo "Done. Consider updating Cargo.lock if needed (cargo fetch)."
