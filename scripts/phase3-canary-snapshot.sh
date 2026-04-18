#!/usr/bin/env bash
# Capture one Phase 3 canary soak snapshot.
#
# This is the evidence-capture entry point for M3.1 soak closure.
# It records the public canary surface, auth status, frontend asset hashes,
# and the operator summary when an auth token is available.
set -euo pipefail

BASE_URL="${OCTOS_TEST_URL:-https://dspfac.crew.ominix.io}"
OUTPUT_DIR=""
AUTH_TOKEN="${OCTOS_AUTH_TOKEN:-}"

usage() {
    cat <<'EOF'
Usage: ./scripts/phase3-canary-snapshot.sh [--base-url URL] [--output-dir DIR] [--auth-token TOKEN]

Environment:
  OCTOS_TEST_URL    Default base URL (default: https://dspfac.crew.ominix.io)
  OCTOS_AUTH_TOKEN  Optional bearer token for /api/admin/operator/summary

Outputs:
  index.html
  auth-status.json
  operator-summary.json or operator-summary.skipped.txt
  asset-hashes.txt
  manifest.txt
EOF
}

while [[ $# -gt 0 ]]; do
    case "$1" in
        --base-url)
            BASE_URL="$2"
            shift 2
            ;;
        --output-dir)
            OUTPUT_DIR="$2"
            shift 2
            ;;
        --auth-token)
            AUTH_TOKEN="$2"
            shift 2
            ;;
        --help|-h)
            usage
            exit 0
            ;;
        *)
            echo "Unknown argument: $1" >&2
            usage >&2
            exit 1
            ;;
    esac
done

timestamp="$(date -u +%Y%m%dT%H%M%SZ)"
if [[ -z "$OUTPUT_DIR" ]]; then
    OUTPUT_DIR="artifacts/phase3-soak/${timestamp}"
fi

mkdir -p "$OUTPUT_DIR"

homepage_tmp="${OUTPUT_DIR}/index.html.tmp"
auth_tmp="${OUTPUT_DIR}/auth-status.json.tmp"

curl -fsSL "${BASE_URL}/" -o "$homepage_tmp"
mv "$homepage_tmp" "${OUTPUT_DIR}/index.html"

curl -fsSL "${BASE_URL}/api/auth/status" -o "$auth_tmp"
mv "$auth_tmp" "${OUTPUT_DIR}/auth-status.json"

{
    echo "timestamp=${timestamp}"
    echo "base_url=${BASE_URL}"
    echo "pwd=$(pwd)"
    echo "hostname=$(hostname)"
} > "${OUTPUT_DIR}/manifest.txt"

grep -Eo 'assets/[^"'"'"']+\.(js|css)' "${OUTPUT_DIR}/index.html" \
    | sort -u \
    > "${OUTPUT_DIR}/asset-hashes.txt" || true

if [[ -n "$AUTH_TOKEN" ]]; then
    summary_tmp="${OUTPUT_DIR}/operator-summary.json.tmp"
    curl -fsSL \
        -H "Authorization: Bearer ${AUTH_TOKEN}" \
        "${BASE_URL}/api/admin/operator/summary" \
        -o "$summary_tmp"
    mv "$summary_tmp" "${OUTPUT_DIR}/operator-summary.json"
else
    cat > "${OUTPUT_DIR}/operator-summary.skipped.txt" <<'EOF'
Skipped operator summary capture because OCTOS_AUTH_TOKEN/--auth-token was not provided.
EOF
fi

echo "Captured Phase 3 soak snapshot in ${OUTPUT_DIR}"
