#!/usr/bin/env bash
# Builds a release of ainitShot on Linux (decision 13): the program for Windows, its installer and
# the description of the release, latest.json, into dist/. Signing and publishing are
# ci/publish.sh's.
#
#   ci/build.sh v0.2.0   for the tag of a release, which must match the version in Cargo.toml
#   ci/build.sh          for a trial build of the version in Cargo.toml
#
# Needs cargo-xwin with the target x86_64-pc-windows-msvc, Wine, Xvfb and Python 3: the image of
# .sourcecraft/ci.yaml has them, with xvfb and xauth installed.
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

# Inno Setup under Wine, the same version every time: its installer is checked before it runs, and
# it installs itself only once into the Wine of the build. jrsoftware publishes no checksums; this
# file's Authenticode signature was checked on Windows on 2026-10-25: Valid, signed by Pyrsys B.V.
# (Sectigo Public Code Signing CA R36), with a Sectigo timestamp.
inno=6.7.3
inno_sha256=9c73c3bae7ed48d44112a0f48e66742c00090bdb5bef71d9d3c056c66e97b732
export WINEPREFIX="${WINEPREFIX:-$HOME/.wine-inno}" WINEDEBUG=-all WINEDLLOVERRIDES="mscoree,mshtml="
iscc="$WINEPREFIX/drive_c/inno-$inno/ISCC.exe"
if [ ! -f "$iscc" ]; then
  download=$(mktemp -d)
  curl -fsSL -o "$download/innosetup.exe" "https://github.com/jrsoftware/issrc/releases/download/is-${inno//./_}/innosetup-$inno.exe"
  echo "$inno_sha256  $download/innosetup.exe" | sha256sum -c --quiet
  # The installer of Inno Setup needs a screen, if only a virtual one, and starts a second process
  # that outlives the first: Wine is waited for until everything has ended.
  xvfb-run -a sh -c "wineboot --init; wineserver -w; wine '$download/innosetup.exe' /VERYSILENT /SUPPRESSMSGBOXES /NORESTART /SP- /CURRENTUSER '/DIR=C:\\inno-$inno'; wineserver -w"
  rm -rf "$download"
  [ -f "$iscc" ] || { echo "Inno Setup $inno didn't install under Wine." >&2; exit 1; }
fi

mkdir -p dist
setup="ainitShot-$version-windows-x64-setup"
python3 tools/installer-texts.py
wine "$iscc" /Q "/DVersion=$version" "/DExe=$(winepath -w "$(realpath "$exe")")" "/O$(winepath -w "$(realpath dist)")" \
  "/F$setup" "$(winepath -w "$(realpath installer/ainitShot.iss)")"
setup="$setup.exe"
[ -f "dist/$setup" ] || { echo "The installer wasn't made." >&2; exit 1; }

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
