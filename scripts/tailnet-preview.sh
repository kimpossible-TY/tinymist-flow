#!/bin/sh

set -eu

usage() {
  cat <<'EOF'
Usage: scripts/tailnet-preview.sh INPUT [TINYMIST PREVIEW OPTIONS]

Start a live Typst preview on this host's MagicDNS name.

Environment overrides:
  TINYMIST_BIN           Tinymist executable to run
  TINYMIST_TAILNET_HOST  MagicDNS hostname; skips CLI discovery
  TINYMIST_PREVIEW_PORT  Browser-facing port (default: 23625)
  TAILSCALE_BIN          Tailscale executable (default: tailscale)
EOF
}

fail() {
  printf 'tailnet-preview: %s\n' "$*" >&2
  exit 1
}

is_single_hostname() {
  printf '%s\n' "$1" | awk '
    NR > 1 { exit 1 }
    {
      if (length($0) > 253 || $0 !~ /^[A-Za-z0-9.-]+$/ || $0 ~ /\.\./) {
        exit 1
      }
      count = split($0, labels, ".")
      for (i = 1; i <= count; i++) {
        if (length(labels[i]) < 1 || length(labels[i]) > 63 ||
            labels[i] !~ /^[A-Za-z0-9]([A-Za-z0-9-]*[A-Za-z0-9])?$/) {
          exit 1
        }
      }
    }
    END { if (NR != 1) exit 1 }
  '
}

case "${1:-}" in
  -h|--help)
    usage
    exit 0
    ;;
esac

[ "$#" -ge 1 ] || {
  usage >&2
  exit 2
}

input=$1
shift

script_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
repo_root=$(CDPATH= cd -- "$script_dir/.." && pwd)

tinymist_bin=${TINYMIST_BIN:-}
if [ -z "$tinymist_bin" ]; then
  if [ -x "$repo_root/target/release/tinymist" ]; then
    tinymist_bin="$repo_root/target/release/tinymist"
  elif command -v tinymist >/dev/null 2>&1; then
    tinymist_bin=$(command -v tinymist)
  else
    fail "cannot find tinymist; build target/release/tinymist or set TINYMIST_BIN"
  fi
elif command -v "$tinymist_bin" >/dev/null 2>&1; then
  tinymist_bin=$(command -v "$tinymist_bin")
fi
[ -x "$tinymist_bin" ] || fail "TINYMIST_BIN is not executable: $tinymist_bin"

port=${TINYMIST_PREVIEW_PORT:-23625}
case "$port" in
  ''|*[!0-9]*) fail "TINYMIST_PREVIEW_PORT must be an integer from 1 to 65535" ;;
esac
[ "$port" -ge 1 ] && [ "$port" -le 65535 ] || \
  fail "TINYMIST_PREVIEW_PORT must be an integer from 1 to 65535"

tailnet_host=${TINYMIST_TAILNET_HOST:-}
if [ -z "$tailnet_host" ]; then
  tailscale_bin=${TAILSCALE_BIN:-tailscale}
  command -v "$tailscale_bin" >/dev/null 2>&1 || \
    fail "cannot find tailscale; set TAILSCALE_BIN or TINYMIST_TAILNET_HOST"
  status_json=$("$tailscale_bin" status --json) || \
    fail "cannot read Tailscale status; is Tailscale connected?"

  if command -v jq >/dev/null 2>&1; then
    tailnet_host=$(printf '%s\n' "$status_json" | \
      jq -er '.Self.DNSName | strings | sub("\\.$"; "") | select(length > 0)') || \
      fail "cannot discover this host's MagicDNS name from Tailscale status"
  elif command -v plutil >/dev/null 2>&1; then
    tailnet_host=$(printf '%s\n' "$status_json" | \
      plutil -extract Self.DNSName raw -o - -) || \
      fail "cannot discover this host's MagicDNS name from Tailscale status"
    tailnet_host=${tailnet_host%.}
  else
    fail "MagicDNS discovery requires jq or plutil; set TINYMIST_TAILNET_HOST"
  fi
fi

tailnet_host=${tailnet_host%.}
is_single_hostname "$tailnet_host" || \
  fail "expected exactly one valid MagicDNS hostname, got: $tailnet_host"
[ "$tailnet_host" != "0.0.0.0" ] || fail "refusing to use the wildcard address 0.0.0.0"

preview_url="http://$tailnet_host:$port/"
printf 'Tailnet preview: %s\n' "$preview_url"
printf 'Input: %s\n' "$input"

exec "$tinymist_bin" preview "$input" \
  "--data-plane-host=$tailnet_host:$port" \
  "--control-plane-host=127.0.0.1:0" \
  --no-open \
  "$@"
