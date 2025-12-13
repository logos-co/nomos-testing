#!/usr/bin/env bash
set -euo pipefail

# Query all metric names from a Prometheus endpoint and print one sample value
# per metric (if present).
#
# Usage:
#   PROM_URL=http://127.0.0.1:9090 ./scripts/query-prom-metrics.sh
#   ./scripts/query-prom-metrics.sh http://127.0.0.1:59804

PROM_URL="${1:-${PROM_URL:-http://127.0.0.1:9090}}"

require() { command -v "$1" >/dev/null 2>&1 || { echo "$1 is required but not installed" >&2; exit 1; }; }
require jq
require python3

echo "Querying Prometheus at ${PROM_URL}"
python3 - <<'PY'
import os, sys, json, urllib.parse, urllib.request

prom = os.environ.get("PROM_URL")
if not prom:
    sys.exit("PROM_URL is not set")

def fetch(path, params=None):
    url = prom + path
    if params:
        url += "?" + urllib.parse.urlencode(params)
    with urllib.request.urlopen(url, timeout=10) as resp:
        return json.load(resp)

names = fetch("/api/v1/label/__name__/values").get("data", [])
if not names:
    sys.exit("No metrics found or failed to reach Prometheus")

jobs = fetch("/api/v1/label/job/values").get("data", [])
if jobs:
    print("Jobs seen:", ", ".join(sorted(jobs)))
else:
    print("Jobs seen: <none>")

by_job = {j: [] for j in jobs} if jobs else {}

for name in sorted(names):
    data = fetch("/api/v1/query", {"query": name}).get("data", {}).get("result", [])
    for series in data:
        labels = series.get("metric", {})
        value = series.get("value", ["", "N/A"])[1]
        job = labels.get("job", "<no-job>")
        by_job.setdefault(job, []).append((name, value))

if not by_job:
    sys.exit("No metric samples returned")

for job in sorted(by_job):
    print(f"{job}:")
    samples = by_job[job]
    if not samples:
        print("  <no samples>")
    else:
        for name, value in sorted(samples):
            print(f"  {name}: {value}")
PY
