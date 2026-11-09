#!/usr/bin/env bash
# Render the Homebrew formula for a release.
#
# Usage: scripts/render-homebrew-formula.sh <version> <sha256sums-file>
#
# Prints the rendered formula to stdout. The checksums file must contain the
# four release tarballs produced by .github/workflows/release.yml.
set -euo pipefail

if [ "$#" -ne 2 ]; then
  printf 'usage: %s <version> <sha256sums-file>\n' "$0" >&2
  exit 2
fi

version="${1#v}"
sums="$2"

if [ ! -f "$sums" ]; then
  printf 'checksums file not found: %s\n' "$sums" >&2
  exit 1
fi

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
template="${script_dir}/../packaging/homebrew/naskah.rb.tmpl"

sha_for() {
  local target="$1"
  local name="naskah-v${version}-${target}.tar.gz"
  local hash
  hash="$(awk -v name="$name" '
    {
      file = $2
      sub(/^\*/, "", file)
      if (file == name) {
        print $1
        exit
      }
    }
  ' "$sums")"

  if [ -z "$hash" ]; then
    printf 'missing checksum for %s in %s\n' "$name" "$sums" >&2
    exit 1
  fi

  printf '%s' "$hash"
}

macos_arm64="$(sha_for aarch64-apple-darwin)"
macos_x86_64="$(sha_for x86_64-apple-darwin)"
linux_arm64="$(sha_for aarch64-unknown-linux-gnu)"
linux_x86_64="$(sha_for x86_64-unknown-linux-gnu)"

sed \
  -e "s|__VERSION__|${version}|g" \
  -e "s|__SHA256_MACOS_ARM64__|${macos_arm64}|g" \
  -e "s|__SHA256_MACOS_X86_64__|${macos_x86_64}|g" \
  -e "s|__SHA256_LINUX_ARM64__|${linux_arm64}|g" \
  -e "s|__SHA256_LINUX_X86_64__|${linux_x86_64}|g" \
  "$template"
