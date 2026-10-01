#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
PREFIX="${JEVISH_PREFIX:-${HOME}/.local}"
BIN_DIR=""
BINARY_SOURCE=""
BUILD_PROFILE="release"
DRY_RUN=0
FORCE=0
UNINSTALL=0
NO_CLI=0
NO_SKILLS=0
declare -a SKILLS_DIRS=()
declare -a HARNESSES=()

usage() {
  cat <<'EOF'
Install jevish CLI and portable browser skills.

Usage:
  scripts/install.sh [options]

Options:
  --prefix PATH          Installation prefix (default: $JEVISH_PREFIX or ~/.local)
  --bin-dir PATH         CLI destination (default: PREFIX/bin)
  --skills-dir PATH      Harness skill directory; repeat for multiple harnesses
  --harness NAME         codex, claude, agents, project, or generic; repeatable
  --binary PATH          Install a prebuilt jevish binary instead of building
  --profile NAME         Cargo profile: release or debug (default: release)
  --no-cli               Install skills only
  --no-skills            Install CLI only
  --force                Back up conflicting unmanaged skill directories
  --dry-run              Print planned changes without writing
  --uninstall            Remove files recorded by this prefix's manifest
  -h, --help             Show this help

Examples:
  scripts/install.sh --harness codex
  scripts/install.sh --skills-dir /opt/my-harness/skills
  scripts/install.sh --harness codex --harness claude
  scripts/install.sh --prefix /opt/jevish --no-skills
EOF
}

die() {
  printf 'jevish installer: %s\n' "$*" >&2
  exit 1
}

contains_control_path_chars() {
  case "$1" in
    *$'\n'*|*$'\r'*|*$'\t'*) return 0 ;;
    *) return 1 ;;
  esac
}

show_command() {
  printf '+'
  printf ' %q' "$@"
  printf '\n'
}

run() {
  show_command "$@"
  if [[ "$DRY_RUN" -eq 0 ]]; then "$@"; fi
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --prefix) [[ $# -ge 2 ]] || die "--prefix requires a path"; PREFIX="$2"; shift 2 ;;
    --bin-dir) [[ $# -ge 2 ]] || die "--bin-dir requires a path"; BIN_DIR="$2"; shift 2 ;;
    --skills-dir) [[ $# -ge 2 ]] || die "--skills-dir requires a path"; SKILLS_DIRS+=("$2"); shift 2 ;;
    --harness) [[ $# -ge 2 ]] || die "--harness requires a name"; HARNESSES+=("$2"); shift 2 ;;
    --binary) [[ $# -ge 2 ]] || die "--binary requires a path"; BINARY_SOURCE="$2"; shift 2 ;;
    --profile) [[ $# -ge 2 ]] || die "--profile requires release or debug"; BUILD_PROFILE="$2"; shift 2 ;;
    --no-cli) NO_CLI=1; shift ;;
    --no-skills) NO_SKILLS=1; shift ;;
    --force) FORCE=1; shift ;;
    --dry-run) DRY_RUN=1; shift ;;
    --uninstall) UNINSTALL=1; shift ;;
    -h|--help) usage; exit 0 ;;
    *) die "unknown option: $1" ;;
  esac
done

[[ "$BUILD_PROFILE" == "release" || "$BUILD_PROFILE" == "debug" ]] || die "--profile must be release or debug"
[[ "$NO_CLI" -eq 0 || "$NO_SKILLS" -eq 0 ]] || die "--no-cli and --no-skills cannot be combined"
BIN_DIR="${BIN_DIR:-$PREFIX/bin}"
SHARE_DIR="$PREFIX/share/jevish"
MANIFEST="$SHARE_DIR/install.manifest"

for path in "$PREFIX" "$BIN_DIR" "$SHARE_DIR" "${SKILLS_DIRS[@]:-}"; do
  [[ -z "$path" ]] || ! contains_control_path_chars "$path" || die "paths cannot contain tabs or newlines"
done

for harness in "${HARNESSES[@]:-}"; do
  [[ -z "$harness" ]] && continue
  case "$harness" in
    codex) SKILLS_DIRS+=("${CODEX_HOME:-${HOME}/.codex}/skills") ;;
    claude) SKILLS_DIRS+=("${CLAUDE_CONFIG_DIR:-${HOME}/.claude}/skills") ;;
    agents) SKILLS_DIRS+=("${HOME}/.agents/skills") ;;
    project) SKILLS_DIRS+=("${PWD}/.agents/skills") ;;
    generic) ;;
    *) die "unknown harness '$harness'; use codex, claude, agents, project, or generic" ;;
  esac
done

remove_installation() {
  [[ -f "$MANIFEST" ]] || die "no install manifest found at $MANIFEST"
  while IFS=$'\t' read -r kind path; do
    [[ -n "$kind" && -n "$path" ]] || continue
    case "$kind" in
      skill)
        if [[ -f "$path/.jevish-managed" ]]; then run rm -rf -- "$path"; fi
        ;;
      binary)
        [[ "$(basename "$path")" == "jevish" ]] || die "unsafe binary path in manifest: $path"
        if [[ -f "$path" ]]; then run rm -f -- "$path"; fi
        ;;
      shared)
        [[ "$path" == "$SHARE_DIR" ]] || die "unsafe shared path in manifest: $path"
        ;;
      *) die "unknown manifest entry: $kind" ;;
    esac
  done < "$MANIFEST"
  run rm -rf -- "$SHARE_DIR"
  printf 'jevish uninstalled from %s\n' "$PREFIX"
}

if [[ "$UNINSTALL" -eq 1 ]]; then
  remove_installation
  exit 0
fi

if [[ "$NO_SKILLS" -eq 0 ]]; then
  [[ -d "$PROJECT_DIR/skills" ]] || die "skills directory missing from installer bundle"
  [[ -f "$PROJECT_DIR/dist/harness-manifest.json" ]] || die "harness manifest missing from installer bundle"
  [[ -f "$PROJECT_DIR/docs/adapter-protocol.md" ]] || die "adapter protocol missing from installer bundle"
fi
if [[ "$NO_CLI" -eq 0 && -z "$BINARY_SOURCE" ]]; then
  [[ -f "$PROJECT_DIR/Cargo.toml" ]] || die "Cargo.toml missing; supply a prebuilt binary with --binary"
fi

if [[ "$NO_CLI" -eq 0 ]]; then
  if [[ -z "$BINARY_SOURCE" ]]; then
    if [[ "$DRY_RUN" -eq 1 ]]; then
      if [[ "$BUILD_PROFILE" == "release" ]]; then
        show_command cargo build --locked --release
      else
        show_command cargo build --locked
      fi
    elif [[ "$BUILD_PROFILE" == "release" ]]; then
      (cd "$PROJECT_DIR" && cargo build --locked --release)
    else
      (cd "$PROJECT_DIR" && cargo build --locked)
    fi
    BINARY_SOURCE="$PROJECT_DIR/target/$BUILD_PROFILE/jevish"
  elif [[ "$BINARY_SOURCE" != /* ]]; then
    BINARY_SOURCE="$PWD/$BINARY_SOURCE"
  fi
  [[ "$DRY_RUN" -eq 1 || -x "$BINARY_SOURCE" ]] || die "binary is missing or not executable: $BINARY_SOURCE"
fi

timestamp="$(date -u +%Y%m%dT%H%M%SZ)"

install_skill_tree() {
  local source_root="$1"
  local destination_root="$2"
  local skill source destination backup
  run mkdir -p -- "$destination_root"
  for source in "$source_root"/*; do
    [[ -d "$source" && -f "$source/SKILL.md" ]] || continue
    skill="$(basename "$source")"
    destination="$destination_root/$skill"
    if [[ -e "$destination" && ! -f "$destination/.jevish-managed" ]]; then
      [[ "$FORCE" -eq 1 ]] || die "refusing to overwrite unmanaged skill $destination; rerun with --force"
      backup="$destination.jevish-backup-$timestamp"
      run mv -- "$destination" "$backup"
    fi
    run mkdir -p -- "$destination"
    run cp -R -- "$source/." "$destination/"
    if [[ "$DRY_RUN" -eq 0 ]]; then printf 'jevish.browser/v1\n' > "$destination/.jevish-managed"; fi
    MANIFEST_ENTRIES+=("skill"$'\t'"$destination")
  done
}

declare -a MANIFEST_ENTRIES=()
run mkdir -p -- "$SHARE_DIR"

if [[ "$NO_CLI" -eq 0 ]]; then
  run mkdir -p -- "$BIN_DIR"
  run cp -- "$BINARY_SOURCE" "$BIN_DIR/jevish"
  run chmod 0755 "$BIN_DIR/jevish"
  MANIFEST_ENTRIES+=("binary"$'\t'"$BIN_DIR/jevish")
fi

if [[ "$NO_SKILLS" -eq 0 ]]; then
  install_skill_tree "$PROJECT_DIR/skills" "$SHARE_DIR/skills"
  for skills_dir in "${SKILLS_DIRS[@]:-}"; do
    [[ -z "$skills_dir" ]] || install_skill_tree "$PROJECT_DIR/skills" "$skills_dir"
  done
  run cp -- "$PROJECT_DIR/dist/harness-manifest.json" "$SHARE_DIR/harness-manifest.json"
  run cp -- "$PROJECT_DIR/docs/adapter-protocol.md" "$SHARE_DIR/adapter-protocol.md"
fi

MANIFEST_ENTRIES+=("shared"$'\t'"$SHARE_DIR")
if [[ "$DRY_RUN" -eq 0 ]]; then
  : > "$MANIFEST"
  for entry in "${MANIFEST_ENTRIES[@]}"; do printf '%s\n' "$entry" >> "$MANIFEST"; done
fi

if [[ "$NO_CLI" -eq 0 && "$DRY_RUN" -eq 0 ]]; then "$BIN_DIR/jevish" --version; fi
printf 'jevish installed under %s\n' "$PREFIX"
if [[ "$NO_CLI" -eq 0 && ":$PATH:" != *":$BIN_DIR:"* ]]; then
  printf 'Add %s to PATH.\n' "$BIN_DIR"
fi
if [[ "$NO_SKILLS" -eq 0 && "${#SKILLS_DIRS[@]}" -eq 0 ]]; then
  printf 'Portable skills are in %s; copy them into your harness skill directory or rerun with --skills-dir.\n' "$SHARE_DIR/skills"
fi
