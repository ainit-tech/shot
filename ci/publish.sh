#!/usr/bin/env bash
# Signs a release of ainitShot and publishes it on the ainit server (decision 13). CI runs it after
# ci/build.sh:
#
#   bash ci/publish.sh vX.Y.Z
#
# By then dist/ holds latest.json and the installer it points to. Needs minisign, ssh and tar, and
# two secrets in the environment:
#   MINISIGN_KEY     the secret minisign key, without a password, that releases are signed with;
#   PUBLISH_SSH_KEY  the secret SSH key the server lets do nothing but publish.
#
# The server checks the signatures and the SHA-256 in latest.json, takes only a version newer than
# the one published, and lays the files out: the version's folder first, latest.json and its
# signature last (server/publish in the infra repository).
set -euo pipefail

version="${1#v}"
[[ "$version" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]] || { echo "publish: \"$1\" isn't vX.Y.Z" >&2; exit 1; }
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
