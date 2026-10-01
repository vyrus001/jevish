#!/usr/bin/env bash
set -euo pipefail

PROJECT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
BINARY="${1:-$PROJECT_DIR/target/debug/jevish}"
TEST_ROOT="$(mktemp -d "${TMPDIR:-/tmp}/jevish-installer-test.XXXXXX")"
PREFIX="$TEST_ROOT/prefix"
HARNESS="$TEST_ROOT/harness"

cleanup() {
  rm -rf -- "$TEST_ROOT"
}
trap cleanup EXIT

"$PROJECT_DIR/scripts/install.sh" --binary "$BINARY" --prefix "$PREFIX" --skills-dir "$HARNESS"
"$PREFIX/bin/jevish" --version >/dev/null
test -x "$PREFIX/bin/jevish"
test -f "$PREFIX/share/jevish/install.manifest"
test -f "$PREFIX/share/jevish/harness-manifest.json"
test -f "$HARNESS/browser-gate/SKILL.md"
test -f "$HARNESS/browser-gate/.jevish-managed"

"$PROJECT_DIR/scripts/install.sh" --prefix "$PREFIX" --uninstall
test ! -e "$PREFIX/bin/jevish"
test ! -e "$PREFIX/share/jevish"
test ! -e "$HARNESS/browser-gate"

CONFLICT_PREFIX="$TEST_ROOT/conflict-prefix"
CONFLICT_HARNESS="$TEST_ROOT/conflict-harness"
mkdir -p "$CONFLICT_HARNESS/browser-bind"

if "$PROJECT_DIR/scripts/install.sh" --no-cli --prefix "$CONFLICT_PREFIX" --skills-dir "$CONFLICT_HARNESS" >/dev/null 2>&1; then
  printf 'expected unmanaged skill conflict to fail\n' >&2
  exit 1
fi

"$PROJECT_DIR/scripts/install.sh" --no-cli --force --prefix "$CONFLICT_PREFIX" --skills-dir "$CONFLICT_HARNESS" >/dev/null
test -f "$CONFLICT_HARNESS/browser-bind/.jevish-managed"
backup_count="$(find "$CONFLICT_HARNESS" -maxdepth 1 -type d -name 'browser-bind.jevish-backup-*' | wc -l | tr -d ' ')"
test "$backup_count" = "1"

printf 'installer tests passed\n'
