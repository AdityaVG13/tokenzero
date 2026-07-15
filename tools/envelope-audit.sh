#!/usr/bin/env bash
# Envelope overhead audit report (6ot).
# Reads CodeMode telemetry JSON from stdin or a file and produces a
# per-session breakdown of visible tokens by attribution bucket.
#
# Usage:
#   cat telemetry.json | ./tools/envelope-audit.sh
#   ./tools/envelope-audit.sh telemetry.json
#
# Buckets: ack, ref_string, framing, preview, payload, envelope
set -euo pipefail

input="${1:-/dev/stdin}"

if ! command -v jq &>/dev/null; then
  echo "error: jq is required" >&2
  exit 1
fi

jq -r '
def lpad($width):
  tostring as $value
  | (" " * ([$width - ($value | length), 0] | max)) + $value;

  .telemetry as $t |
  [
    "ack",         ($t.ack_tokens // 0),
    "ref_string",  ($t.ref_string_tokens // 0),
    "framing",     ($t.framing_tokens // 0),
    "preview",     ($t.preview_tokens // 0),
    "payload",     ($t.payload_tokens // 0),
    "envelope",    ($t.envelope_tokens // 0),
    "visible",     ($t.visible_tokens // 0),
    "raw",         ($t.raw_tokens // 0)
  ] as $rows |
  "bucket           tokens  pct-of-visible",
  "──────────────────────────────────────",
  ($rows | range(0; length; 2) as $i | .[$i] as $k | .[$i + 1] as $v |
    "\(($k | lpad(16))) \(($v | tostring | lpad(8)))  \(if ($t.visible_tokens // 0) > 0 then ((($v / ($t.visible_tokens // 1)) * 100) | round | tostring | lpad(6)) else "    -" end)%"
  ),
  "",
  "ops=\($t.logical_ops // 0) refs=\($t.refs_count // 0) wall_ms=\($t.wall_ms // 0) bytes_materialized=\($t.bytes_materialized // 0)"
' "$input"
