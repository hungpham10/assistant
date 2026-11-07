#!/usr/bin/env bash
# bump.sh - bump the Rust workspace version and (optionally) tag + push it.
#
# Usage:
#   scripts/bump.sh patch          # 0.0.44 -> 0.0.45
#   scripts/bump.sh minor|major
#   scripts/bump.sh 0.1.0          # explicit version
#   scripts/bump.sh patch --tag    # also tag rust-v<version> and push
#   scripts/bump.sh patch --tag-push
#
# Updates:
#   - codex-rs/Cargo.toml  (workspace package version)
#   - codex-rs/Cargo.lock  (workspace member entries)
#   - tui's VERSION env / release workflow derive release version from tags
set -euo pipefail

CARGO_TOML="codex-rs/Cargo.toml"
CARGO_LOCK="codex-rs/Cargo.lock"

usage() {
  grep '^#' "$0" | cut -c3- | sed -n '2,20p'
  exit 1
}

[[ $# -ge 1 ]] || usage
BUMP="$1"
shift || true

TAG=false
PUSH=false
for arg in "$@"; do
  case "$arg" in
    --tag) TAG=true ;;
    --tag-push) TAG=true; PUSH=true ;;
    *) echo "❌  Unknown option: $arg"; usage ;;
  esac
done

CURRENT=$(awk '
  /^\[workspace\]/ {in_ws=1; next}
  in_ws && /^version = "/ {match($0, /"[^"]+"/); print substr($0, RSTART+1, RLENGTH-2); exit}
' "$CARGO_TOML")
[[ -n "$CURRENT" ]] || { echo "❌  Could not read version from $CARGO_TOML"; exit 1; }

if [[ "$BUMP" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
  NEW="$BUMP"
else
  IFS='.' read -r MAJOR MINOR PATCH <<<"$CURRENT"
  case "$BUMP" in
    patch) PATCH=$((PATCH + 1)) ;;
    minor) MINOR=$((MINOR + 1)); PATCH=0 ;;
    major) MAJOR=$((MAJOR + 1)); MINOR=0; PATCH=0 ;;
    *) echo "❌  Expected patch|minor|major|X.Y.Z, got: $BUMP"; usage ;;
  esac
  NEW="$MAJOR.$MINOR.$PATCH"
fi

echo "Bump $CURRENT -> $NEW  ($CARGO_TOML)"

# 1) Cargo.toml: first `version = "..."` after [package] header in workspace pkg section.
awk -v new="$NEW" '
  /^version = "/ && ws_pkg {sub(/"[^"]+"/, "\"" new "\""); ws_pkg=0}
  /^\[workspace\]/ && !pkg_seen {ws_pkg=1}
  /^\[package\]/ {pkg_seen=1; ws_pkg=1}
  {print}
' "$CARGO_TOML" > "$CARGO_TOML.tmp" && mv "$CARGO_TOML.tmp" "$CARGO_TOML"

# 2) Cargo.lock: workspace member entries whose version equals the old one.
awk -v old="$CURRENT" -v new="$NEW" '
  /^name = / {member=($0 ~ /^name = "codex[a-z-]*"$/)}
  /^version = / && member && $0 == "version = \"" old "\"" {sub(/"[^"]+"/, "\"" new "\"")}
  /^\[\[/ {member=0}
  {print}
' "$CARGO_LOCK" > "$CARGO_LOCK.tmp" && mv "$CARGO_LOCK.tmp" "$CARGO_LOCK"

VERIFY=$(awk '/^\[workspace\]/ {in_ws=1; next} in_ws && /^version = "/ {match($0, /"[^"]+"/); print substr($0, RSTART+1, RLENGTH-2); exit}' "$CARGO_TOML")
[[ "$VERIFY" == "$NEW" ]] || { echo "❌  Verification failed, expected $NEW"; exit 1; }

git diff --stat -- "$CARGO_TOML" "$CARGO_LOCK"

if $TAG; then
  git commit -m "chore: bump rust version to $NEW"
  git tag -a "rust-v$NEW" -m "Release $NEW"
  echo "✅  Committed and tagged rust-v$NEW"
  if $PUSH; then
    DEFAULT_BRANCH=$(git symbolic-ref --short HEAD)
    git push origin "rust-v$NEW"
    echo "🚀  Pushed tag rust-v$NEW (release workflow will trigger)"
  fi
else
  echo
  echo "Next steps:"
  echo "  git commit -m 'chore: bump rust version to $NEW'"
  echo "  git tag -a rust-v$NEW -m 'Release $NEW'"
  echo "  git push origin rust-v$NEW"
fi
