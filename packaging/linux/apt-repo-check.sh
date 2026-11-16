#!/usr/bin/env bash
# Installs bimcraft the way a user's machine does, from the site apt-repo.sh wrote, and runs both programs:
#
#   apt-repo-check.sh <site-dir> [version]
#
# With [version] (a release's, without the v) the installed bimcraft-cli must say it, so the repository's newest package is the
# release just made. Run by the release workflow before the site is published, so a repository apt can't read never goes out.
# It adds a source under /etc/apt and installs the package: run it on a disposable Debian or Ubuntu machine (a CI runner) with sudo.
set -euo pipefail

WANT="${2:-}"
# apt reads a file: source as its unprivileged user _apt, which can't enter a runner's home; a copy under /var/tmp it can read.
SITE="$(mktemp -d /var/tmp/apt-repo-check.XXXXXX)"
cp -a "$(cd "${1:?usage: apt-repo-check.sh <site-dir> [version]}" && pwd)/." "$SITE/"
chmod -R a+rX "$SITE"

sudo install -d -m 0755 /etc/apt/keyrings
sudo install -m 0644 "$SITE/bimcraft-archive-keyring.gpg" /etc/apt/keyrings/bimcraft-archive-keyring.gpg
sudo tee /etc/apt/sources.list.d/bimcraft-check.sources >/dev/null <<SRC
Types: deb
URIs: file://$SITE
Suites: stable
Components: main
Signed-By: /etc/apt/keyrings/bimcraft-archive-keyring.gpg
SRC
sudo apt-get update
apt-cache policy bimcraft
sudo apt-get install -y bimcraft
cli="$(bimcraft-cli --version)"
echo "$cli"
bimcraft --version
if [ -n "$WANT" ]; then
  case "$cli" in *" $WANT") ;; *) echo "error: the repository gave '$cli', not $WANT" >&2; exit 1 ;; esac
fi
