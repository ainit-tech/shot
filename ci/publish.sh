#!/usr/bin/env bash
# Signs a release of ainitShot and publishes it on the ainit server (decision 13). CI runs it after
# ci/build.sh:
#
#   bash ci/publish.sh vX.Y.Z
#   bash ci/publish.sh          in a run started by hand, which names no tag
#
# By then dist/ holds latest.json and the installer it points to. Needs minisign, ssh, tar and git,
# and two secrets in the environment:
#   MINISIGN_KEY     the secret minisign key, without a password, that releases are signed with;
#   PUBLISH_SSH_KEY  the secret SSH key the server lets do nothing but publish.
#
# The server checks the signatures and the SHA-256 in latest.json, takes only a version newer than
# the one published, and lays the files out: the version's folder first, latest.json and its
# signature last (server/publish in the infra repository).
set -euo pipefail

# The release's tag: the one CI names as it runs on a tag. A run started by hand names none, and
# then it is the tag the repository has on the commit built: a commit without a tag vX.Y.Z, say on
# a branch, is never published. The tag must be the version in Cargo.toml either way.
tag="${1:-}"
tag="${tag#refs/tags/}"
if [ -z "$tag" ]; then
  head="${SOURCECRAFT_COMMIT_SHA:-$(git rev-parse HEAD)}"
  tag=$(git ls-remote --tags "${REPOSITORY:-https://git.sourcecraft.dev/ainit-tech/shot.git}" \
    | awk -v head="$head" '$1 == head && $2 ~ /^refs\/tags\/v[0-9]+\.[0-9]+\.[0-9]+(\^\{\})?$/ {
        sub(/^refs\/tags\//, "", $2); sub(/\^\{\}$/, "", $2); print $2; exit }')
fi
[[ "$tag" =~ ^v[0-9]+\.[0-9]+\.[0-9]+$ ]] || { echo "publish: \"$tag\" isn't a tag vX.Y.Z of the commit built" >&2; exit 1; }
version="${tag#v}"
cargo=$(sed -n 's/^version = "\(.*\)"$/\1/p' Cargo.toml | head -n 1)
[ "$version" = "$cargo" ] || { echo "publish: the tag $tag isn't the version $cargo in Cargo.toml" >&2; exit 1; }
dist="${DIST:-dist}"
installer="ainitShot-$version-windows-x64-setup.exe"
[ -f "$dist/latest.json" ] && [ -f "$dist/$installer" ] || { echo "publish: $dist/ lacks latest.json or $installer" >&2; exit 1; }

work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
umask 077
# minisign reads a key file of two lines, a comment and the key; a secret that holds the key alone
# gets the comment here.
case "$MINISIGN_KEY" in
  "untrusted comment:"*) printf '%s\n' "$MINISIGN_KEY" ;;
  *) printf 'untrusted comment: minisign secret key\n%s\n' "$MINISIGN_KEY" ;;
esac > "$work/minisign.key"
printf '%s\n' "$PUBLISH_SSH_KEY" > "$work/ssh.key"
# The server's key is known beforehand: CI trusts no host it meets.
echo "ainit.tech ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIEkyEeIHE58Yxz3nhUmy6FoKwrwHhz7hroTZzEoZTpd7" > "$work/known_hosts"

# The layout the server expects: latest.json and the version's folder with the installer.
mkdir -p "$work/out/$version"
cp "$dist/latest.json" "$work/out/latest.json"
cp "$dist/$installer" "$work/out/$version/$installer"
minisign -S -s "$work/minisign.key" -m "$work/out/$version/$installer" -t "ainitShot $version" > /dev/null
minisign -S -s "$work/minisign.key" -m "$work/out/latest.json" -t "ainitShot $version" > /dev/null

# Tried again only when the connection itself fails (code 255): publishing the same version again
# doesn't trouble the server.
for try in 1 2 3; do
  rc=0
  tar -C "$work/out" -cf - latest.json latest.json.minisig "$version" \
    | ssh -i "$work/ssh.key" -o IdentitiesOnly=yes -o BatchMode=yes \
        -o UserKnownHostsFile="$work/known_hosts" -o StrictHostKeyChecking=yes \
        -o ConnectTimeout=20 -o ServerAliveInterval=10 \
        shot-publish@ainit.tech "publish $version" || rc=$?
  [ "$rc" -ne 255 ] && exit "$rc"
  echo "publish: ssh couldn't reach the server (try $try of 3)" >&2
  sleep 5
done
exit 255
