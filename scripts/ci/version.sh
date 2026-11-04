#!/usr/bin/env bash
# Single source of truth for the release version and artifact-name rules.
#
# Why this exists: v1.26.0 and v1.26.1 published nothing because a CRLF rewrite
# of app/Cargo.toml left a carriage return in the `sed` output, and that value
# was interpolated straight into artifact filenames, which upload-artifact and
# the release API reject. One parser for every OS that strips whitespace and
# control characters and validates the shape removes the whole class of bug.
#
# Usage:
#   scripts/ci/version.sh [manifest]         print the version (default app/Cargo.toml)
#   scripts/ci/version.sh --check-name NAME  exit 0 only if NAME is a safe basename
#   scripts/ci/version.sh --check-version V  exit 0 only if V is a usable version

set -euo pipefail

fail() { printf 'ERROR: %s\n' "$*" >&2; exit 1; }

# Remove every whitespace and control character (CR, LF and TAB included).
strip_noise() {
  printf '%s' "${1-}" | tr -d '[:space:][:cntrl:]'
}

read_manifest_version() {
  local manifest="${1:-app/Cargo.toml}" raw
  [ -f "$manifest" ] || fail "manifest not found: $manifest"
  raw="$(sed -n 's/^version = "\(.*\)"/\1/p' "$manifest" | head -n1)"
  strip_noise "$raw"
}

valid_version() {
  local value="${1-}"
  [ -n "$value" ] || return 1
  [[ "$value" =~ ^[0-9]+\.[0-9]+\.[0-9]+([-+][0-9A-Za-z.]+)?$ ]]
}

require_version() {
  local value="${1-}"
  valid_version "$value" || fail "unusable version: '${value}' (expected MAJOR.MINOR.PATCH)"
  printf '%s\n' "$value"
}

version_from_manifest() {
  require_version "$(read_manifest_version "${1:-app/Cargo.toml}")"
}

# Artifact basenames become upload paths and release asset keys, so refuse
# anything with control characters, spaces or shell-hostile bytes.
safe_artifact_name() {
  local name="${1-}"
  [ -n "$name" ] || return 1
  case "$name" in
    *$'\r'* | *$'\n'* | *$'\t'*) return 1 ;;
    *' '*) return 1 ;;
  esac
  [[ "$name" =~ ^[A-Za-z0-9._+-]+$ ]]
}

case "${1:-}" in
  --check-name)
    safe_artifact_name "${2-}" || fail "unsafe artifact name: '${2-}'"
    ;;
  --check-version)
    require_version "${2-}" >/dev/null
    ;;
  -h | --help)
    sed -n '2,16p' "$0"
    ;;
  *)
    version_from_manifest "${1:-app/Cargo.toml}"
    ;;
esac
