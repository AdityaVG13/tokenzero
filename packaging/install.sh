#!/usr/bin/env bash
# TokenZero package installer (tokenzero-irx9.3) — macOS / Linux.
#
# Installs exactly one surface artifact: tokenzero-mcp OR tokenzero-codemode.
# Replaces any prior registration. Never registers both catalogs.
#
# CRITICAL: install state + client-config are written by THIS SCRIPT only
# (installer-native atomic path). Never invoke the surface binary for
# `install` during packaging lifecycle ownership — surface bins implement
# non-hanging install as a convenience, but the shell installer is the
# canonical release path and never starts a stdio server.
#
# Usage:
#   ./packaging/install.sh --surface mcp|codemode [--prefix DIR] [--bin-dir DIR]
#   ./packaging/install.sh --uninstall [--prefix DIR]
#   ./packaging/install.sh --sbom --surface mcp|codemode
#   ./packaging/install.sh --surface mcp --skip-build
#
# Selection matrix:
#   native CodeMode client  -> install tokenzero-mcp
#   legacy MCP-only client  -> install tokenzero-codemode
#
# Platform simulation for e2e: TOKENZERO_INSTALL_PLATFORM=macos|linux

set -euo pipefail

SURFACE=""
PREFIX="${TOKENZERO_INSTALL_PREFIX:-${HOME}/.tokenzero-install}"
BIN_DIR="${TOKENZERO_BIN_DIR:-${HOME}/.local/bin}"
ACTION="install"
SKIP_BUILD=0
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
VERSION="1.4.0"

usage() {
  cat <<EOF
usage: $0 --surface mcp|codemode [--prefix DIR] [--bin-dir DIR] [--skip-build]
       $0 --uninstall [--prefix DIR]
       $0 --sbom --surface mcp|codemode

Artifacts: tokenzero-mcp | tokenzero-codemode | tokenzero (compat shim symlink)
Never install both surfaces. Dual client registration is unsupported.
Installer writes state/client-config itself (never hangs on server stdio).
Selection: native CodeMode client -> tokenzero-mcp; otherwise -> tokenzero-codemode.
EOF
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --surface) SURFACE="${2:-}"; shift 2 ;;
    --surface=*) SURFACE="${1#*=}"; shift ;;
    --prefix) PREFIX="${2:-}"; shift 2 ;;
    --prefix=*) PREFIX="${1#*=}"; shift ;;
    --bin-dir) BIN_DIR="${2:-}"; shift 2 ;;
    --bin-dir=*) BIN_DIR="${1#*=}"; shift ;;
    --uninstall) ACTION="uninstall"; shift ;;
    --sbom) ACTION="sbom"; shift ;;
    --skip-build) SKIP_BUILD=1; shift ;;
    -h|--help) usage; exit 0 ;;
    *) echo "unknown arg: $1" >&2; usage; exit 2 ;;
  esac
done

os_name() {
  if [[ -n "${TOKENZERO_INSTALL_PLATFORM:-}" ]]; then
    case "${TOKENZERO_INSTALL_PLATFORM}" in
      macos|linux|windows|other) echo "${TOKENZERO_INSTALL_PLATFORM}" ;;
      *) echo "invalid TOKENZERO_INSTALL_PLATFORM=${TOKENZERO_INSTALL_PLATFORM}" >&2; exit 2 ;;
    esac
    return
  fi
  case "$(uname -s)" in
    Darwin) echo macos ;;
    Linux) echo linux ;;
    *) echo other ;;
  esac
}

artifact_for_surface() {
  case "$1" in
    mcp) echo tokenzero-mcp ;;
    codemode) echo tokenzero-codemode ;;
    *) echo "bad surface: $1" >&2; exit 2 ;;
  esac
}

feature_for_surface() {
  case "$1" in
    mcp) echo surface-mcp ;;
    codemode) echo surface-codemode ;;
  esac
}

atomic_write() {
  local path="$1"
  local dir tmp
  dir="$(dirname "$path")"
  mkdir -p "$dir"
  tmp="${path}.tmp.$$"
  cat >"$tmp"
  mv -f "$tmp" "$path"
}

write_install_state() {
  local surface="$1" artifact="$2" binary="$3" digest="$4" platform="$5"
  local now
  now="$(date +%s)"
  atomic_write "$PREFIX/client-config.json" <<EOF
{
  "name": "TokenZero (${surface})",
  "surface": "${surface}",
  "command": "${binary}",
  "args": ["--mode=${surface}"],
  "semantic_contract_digest": "${digest}",
  "package_version": "${VERSION}"
}
EOF
  atomic_write "$PREFIX/install-state.json" <<EOF
{
  "surface": "${surface}",
  "artifact": "${artifact}",
  "binary_path": "${binary}",
  "prefix": "${PREFIX}",
  "semantic_contract_digest": "${digest}",
  "package_version": "${VERSION}",
  "installed_at_unix": ${now},
  "platform": "${platform}",
  "client_config": "${PREFIX}/client-config.json"
}
EOF
  atomic_write "$PREFIX/shim-target" <<EOF
${surface}
EOF
}

read_digest_from_sbom() {
  local bin="$1"
  if [[ ! -x "$bin" ]]; then
    echo "unknown"
    return
  fi
  if command -v python3 >/dev/null 2>&1; then
    python3 - "$bin" <<'PY' 2>/dev/null || echo "unknown"
import json, subprocess, sys
bin_path = sys.argv[1]
try:
    p = subprocess.run([bin_path, "sbom"], capture_output=True, text=True, timeout=15, check=False)
except Exception:
    print("unknown")
    raise SystemExit(0)
text = (p.stdout or "") + "\n" + (p.stderr or "")
for line in text.splitlines():
    line = line.strip()
    if line.startswith("{"):
        try:
            doc = json.loads(line)
            print(doc.get("semantic_contract_digest") or "unknown")
            raise SystemExit(0)
        except json.JSONDecodeError:
            pass
try:
    doc = json.loads(p.stdout or "")
    print(doc.get("semantic_contract_digest") or "unknown")
except Exception:
    print("unknown")
PY
  else
    echo "unknown"
  fi
}

json_field() {
  local file="$1" field="$2"
  if [[ ! -f "$file" ]]; then
    echo ""
    return
  fi
  if command -v python3 >/dev/null 2>&1; then
    python3 - "$file" "$field" <<'PY' 2>/dev/null || true
import json, sys
path, field = sys.argv[1], sys.argv[2]
try:
    with open(path) as f:
        doc = json.load(f)
    v = doc.get(field, "")
    if isinstance(v, str):
        print(v)
    else:
        print(v if v is not None else "")
except Exception:
    pass
PY
  fi
}

if [[ "$ACTION" == "uninstall" ]]; then
  # Installer-native uninstall — never invoke surface server binaries.
  prev_surface="$(json_field "$PREFIX/install-state.json" surface)"
  prev_artifact="$(json_field "$PREFIX/install-state.json" artifact)"
  prev_digest="$(json_field "$PREFIX/install-state.json" semantic_contract_digest)"
  rm -f "$PREFIX/install-state.json" "$PREFIX/client-config.json" "$PREFIX/shim-target"
  rm -f "$BIN_DIR/tokenzero-mcp" "$BIN_DIR/tokenzero-codemode" "$BIN_DIR/tokenzero"
  if [[ -n "${prev_artifact:-}" ]]; then
    echo "uninstall: ok uninstalled=true artifact=${prev_artifact} surface=${prev_surface:-?} semantic_contract_digest=${prev_digest:-?} prefix=$PREFIX platform=$(os_name)"
  else
    echo "uninstall: ok uninstalled=false reason=no_install_state prefix=$PREFIX platform=$(os_name)"
  fi
  exit 0
fi

if [[ -z "$SURFACE" ]]; then
  echo "require --surface mcp|codemode" >&2
  usage
  exit 2
fi

case "$SURFACE" in
  mcp|codemode) ;;
  both|all|mcp+codemode|codemode+mcp)
    echo "tokenzero: dual package surface rejected (fail closed): install requests both surfaces" >&2
    exit 2
    ;;
  *) echo "surface must be mcp or codemode (not both)" >&2; exit 2 ;;
esac

if [[ -n "${TOKENZERO_ENABLE_MCP:-}" && -n "${TOKENZERO_ENABLE_CODEMODE:-}" ]]; then
  echo "tokenzero: dual package surface rejected (fail closed): both TOKENZERO_ENABLE_MCP and TOKENZERO_ENABLE_CODEMODE are set" >&2
  exit 2
fi

ARTIFACT="$(artifact_for_surface "$SURFACE")"
FEATURE="$(feature_for_surface "$SURFACE")"
PLATFORM="$(os_name)"

if [[ "$ACTION" == "sbom" ]]; then
  CANDIDATES=("$BIN_DIR/$ARTIFACT" "$ROOT/target/release/$ARTIFACT" "$ROOT/target/debug/$ARTIFACT")
  for c in "${CANDIDATES[@]}"; do
    if [[ -x "$c" ]]; then
      "$c" sbom
      exit 0
    fi
  done
  echo "sbom: binary $ARTIFACT not found; build first" >&2
  exit 1
fi

echo "install: surface=$SURFACE artifact=$ARTIFACT platform=$PLATFORM prefix=$PREFIX"

if [[ "$SKIP_BUILD" -eq 0 ]]; then
  echo "install: building $ARTIFACT (feature $FEATURE)"
  (
    cd "$ROOT"
    cargo build --release \
      -p tokenzero \
      --bin "$ARTIFACT" \
      --no-default-features \
      --features "${FEATURE}"
  )
  SRC="$ROOT/target/release/$ARTIFACT"
else
  if [[ -x "$ROOT/target/release/$ARTIFACT" ]]; then
    SRC="$ROOT/target/release/$ARTIFACT"
  elif [[ -x "$ROOT/target/debug/$ARTIFACT" ]]; then
    SRC="$ROOT/target/debug/$ARTIFACT"
  else
    echo "install: --skip-build but no prebuilt $ARTIFACT under target/" >&2
    exit 1
  fi
  echo "install: using prebuilt $SRC"
fi

if [[ ! -x "$SRC" ]]; then
  echo "install: FAIL source binary not executable: $SRC" >&2
  exit 1
fi

mkdir -p "$PREFIX" "$BIN_DIR"
# Snapshot prior state for rollback if post-copy verification fails.
ROLLBACK_STATE=""
ROLLBACK_CFG=""
ROLLBACK_SHIM=""
if [[ -f "$PREFIX/install-state.json" ]]; then
  ROLLBACK_STATE="$(cat "$PREFIX/install-state.json")"
fi
if [[ -f "$PREFIX/client-config.json" ]]; then
  ROLLBACK_CFG="$(cat "$PREFIX/client-config.json")"
fi
if [[ -f "$PREFIX/shim-target" ]]; then
  ROLLBACK_SHIM="$(cat "$PREFIX/shim-target")"
fi

install -m 755 "$SRC" "$BIN_DIR/$ARTIFACT"

# Peer removal before shim so only one surface remains.
PEER="$([[ "$SURFACE" == mcp ]] && echo tokenzero-codemode || echo tokenzero-mcp)"
if [[ -e "$BIN_DIR/$PEER" ]]; then
  echo "install: replacing peer artifact $PEER (mutual exclusion)"
  rm -f "$BIN_DIR/$PEER"
fi
# Compatibility shim: selected symlink only — never a dual-surface binary.
ln -sfn "$BIN_DIR/$ARTIFACT" "$BIN_DIR/tokenzero"

DIGEST="$(read_digest_from_sbom "$BIN_DIR/$ARTIFACT")"
write_install_state "$SURFACE" "$ARTIFACT" "$BIN_DIR/$ARTIFACT" "$DIGEST" "$PLATFORM"

if [[ ! -f "$PREFIX/install-state.json" || ! -f "$PREFIX/client-config.json" ]]; then
  echo "install: FAIL state/config not written; restoring prior if any" >&2
  if [[ -n "$ROLLBACK_STATE" ]]; then
    atomic_write "$PREFIX/install-state.json" <<<"$ROLLBACK_STATE"
  fi
  if [[ -n "$ROLLBACK_CFG" ]]; then
    atomic_write "$PREFIX/client-config.json" <<<"$ROLLBACK_CFG"
  fi
  if [[ -n "$ROLLBACK_SHIM" ]]; then
    atomic_write "$PREFIX/shim-target" <<<"$ROLLBACK_SHIM"
  fi
  exit 1
fi

# Single-surface client config check (structured via python when available).
if command -v python3 >/dev/null 2>&1; then
  if ! python3 - "$PREFIX/client-config.json" "$SURFACE" <<'PY'
import json, sys
path, want = sys.argv[1], sys.argv[2]
with open(path) as f:
    doc = json.load(f)
assert doc.get("surface") == want, doc
args = doc.get("args") or []
assert args == [f"--mode={want}"], args
assert not any("mcp" in a and "codemode" in a for a in args)
print("ok")
PY
  then
    echo "install: FAIL client-config dual/malformed surface; restoring prior if any" >&2
    if [[ -n "$ROLLBACK_STATE" ]]; then
      atomic_write "$PREFIX/install-state.json" <<<"$ROLLBACK_STATE"
    fi
    if [[ -n "$ROLLBACK_CFG" ]]; then
      atomic_write "$PREFIX/client-config.json" <<<"$ROLLBACK_CFG"
    fi
    if [[ -n "$ROLLBACK_SHIM" ]]; then
      atomic_write "$PREFIX/shim-target" <<<"$ROLLBACK_SHIM"
    fi
    exit 1
  fi
fi

echo "install: ok surface=$SURFACE artifact=$ARTIFACT prefix=$PREFIX bin=$BIN_DIR/$ARTIFACT shim=$BIN_DIR/tokenzero platform=$PLATFORM semantic_contract_digest=$DIGEST"
echo "client_config: $PREFIX/client-config.json"
echo "selection: native CodeMode client -> tokenzero-mcp; otherwise -> tokenzero-codemode"
