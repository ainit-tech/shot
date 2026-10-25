#!/usr/bin/env bash
# Builds a release of ainitShot on Linux (decision 13): the program for Windows, its installer and
# the description of the release, latest.json, into dist/. Signing and publishing are
# ci/publish.sh's.
#
#   ci/build.sh v0.2.0   for the tag of a release, which must match the version in Cargo.toml
#   ci/build.sh          for a trial build of the version in Cargo.toml
#
# Needs cargo-xwin with the target x86_64-pc-windows-msvc, and NSIS (makensis).
set -euo pipefail

version=$(sed -n 's/^version = "\(.*\)"$/\1/p' Cargo.toml | head -n 1)
if [ -n "${1:-}" ] && [ "$1" != "v$version" ]; then
  echo "The tag $1 doesn't match the version $version in Cargo.toml." >&2
  exit 1
fi

# The image's Rust is older than ort needs: a release builds with the Rust the program is
# developed with.
toolchain=1.95.0
rustup toolchain install "$toolchain" --profile minimal --target x86_64-pc-windows-msvc
cargo "+$toolchain" xwin build --release --locked --target x86_64-pc-windows-msvc -p ainitshot
exe="${CARGO_TARGET_DIR:-target}/x86_64-pc-windows-msvc/release/ainitshot.exe"

mkdir -p dist
setup="ainitShot-$version-windows-x64-setup.exe"
makensis -INPUTCHARSET UTF8 -V2 -DVERSION="$version" -DEXE="$(realpath "$exe")" -DOUT="$(realpath dist)/$setup" \
  installer/ainitShot.nsi

size=$(stat -c %s "dist/$setup")
sha256=$(sha256sum "dist/$setup" | cut -d ' ' -f 1)
cat > dist/latest.json <<EOF
{
  "name": "ainitShot",
  "version": "$version",
  "date": "$(date -u +%Y-%m-%d)",
  "files": {
    "windows-x64": {
      "url": "https://shot.ainit.tech/download/$version/$setup",
      "size": $size,
      "sha256": "$sha256"
    }
  }
}
EOF
echo "dist/$setup: $size bytes; dist/latest.json"
