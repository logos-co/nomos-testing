#!/usr/bin/env bash
set -euo pipefail

# Update nomos-node revision across versions.env and Cargo.toml.
# Usage: scripts/update-nomos-rev.sh <new_rev>

if [ "$#" -ne 1 ]; then
  echo "Usage: $0 <new_nomós_node_rev>" >&2
  exit 1
fi

NEW_REV="$1"
ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

if [ ! -f "${ROOT_DIR}/versions.env" ]; then
  echo "ERROR: versions.env missing; run from repo root." >&2
  exit 1
fi

echo "Updating nomos-node rev to ${NEW_REV}"

# Update versions.env NOMOS_NODE_REV entry (keep other lines intact).
sed -i.bak -E "s/^NOMOS_NODE_REV=.*/NOMOS_NODE_REV=${NEW_REV}/" "${ROOT_DIR}/versions.env"
rm -f "${ROOT_DIR}/versions.env.bak"

# Update all rev fields in Cargo.toml pointing to nomos-node.
sed -i.bak -E "s/(git = \"https:\/\/github.com\/logos-co\/nomos-node\.git\", rev = \")[^\"]+(\".*)/\1${NEW_REV}\2/" "${ROOT_DIR}/Cargo.toml"
rm -f "${ROOT_DIR}/Cargo.toml.bak"

echo "Done. Consider updating Cargo.lock if needed (cargo fetch)."
