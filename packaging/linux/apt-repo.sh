#!/usr/bin/env bash
# Builds the site tree of BIMCraft's signed APT repository from the release's .deb files.
#
#   apt-repo.sh <debs-dir> <site-dir> <signing-key>
#
# <debs-dir> holds the bimcraft .deb files of the releases to keep (the newest three, say: the pool is what fills GitHub
# Pages, whose sites may be 1 GB). Name, version and architecture are read from each .deb's control file, never from its
# file name. <signing-key> is a key id or fingerprint in the current GnuPG home.
# Needs apt-utils (apt-ftparchive), gnupg, dpkg-dev, gzip and xz. It keeps no state: the site is rebuilt whole every time.
#
# Writes <site-dir>/{pool,dists/stable/...}, the public key (bimcraft-archive-keyring.gpg binary, .asc armored) and
# bimcraft.sources, the file a user drops in /etc/apt/sources.list.d, and an index.html with the install lines. BASE_URL is where
# the site is served: the domain's root, bimcraft.ipedrax.com (GitHub Pages with a custom domain).
set -euo pipefail

DEBS="${1:?usage: apt-repo.sh <debs-dir> <site-dir> <signing-key>}"
SITE="${2:?usage: apt-repo.sh <debs-dir> <site-dir> <signing-key>}"
KEY="${3:?usage: apt-repo.sh <debs-dir> <site-dir> <signing-key>}"
BASE_URL="${BASE_URL:-https://bimcraft.ipedrax.com}"
SUITE=stable
COMPONENT=main
KEYRING=bimcraft-archive-keyring

rm -rf "$SITE"
mkdir -p "$SITE/pool/$COMPONENT/b/bimcraft"
archs=()
shopt -s nullglob
for deb in "$DEBS"/*.deb; do
  pkg="$(dpkg-deb -f "$deb" Package)"
  ver="$(dpkg-deb -f "$deb" Version)"
  arch="$(dpkg-deb -f "$deb" Architecture)"
  [ "$pkg" = bimcraft ] || { echo "error: $deb is package '$pkg', not bimcraft" >&2; exit 1; }
  case "$ver" in *~*) echo "error: $deb is a pre-release ($ver); the stable suite takes releases only" >&2; exit 1 ;; esac
  cp "$deb" "$SITE/pool/$COMPONENT/b/bimcraft/${pkg}_${ver}_${arch}.deb"
  case " ${archs[*]-} " in *" $arch "*) ;; *) archs+=("$arch") ;; esac
done
[ "${#archs[@]}" -gt 0 ] || { echo "error: no .deb in $DEBS" >&2; exit 1; }

cd "$SITE"
for arch in "${archs[@]}"; do
  dir="dists/$SUITE/$COMPONENT/binary-$arch"
  mkdir -p "$dir"
  apt-ftparchive --arch "$arch" packages pool >"$dir/Packages"
  gzip -9nk "$dir/Packages"
  xz -9k "$dir/Packages"
done
# No Valid-Until: a repository nobody has published to for a while must still work.
apt-ftparchive \
  -o APT::FTPArchive::Release::Origin=BIMCraft \
  -o APT::FTPArchive::Release::Label=BIMCraft \
  -o APT::FTPArchive::Release::Suite="$SUITE" \
  -o APT::FTPArchive::Release::Codename="$SUITE" \
  -o APT::FTPArchive::Release::Architectures="${archs[*]}" \
  -o APT::FTPArchive::Release::Components="$COMPONENT" \
  -o APT::FTPArchive::Release::Description="BIMCraft packages" \
  release "dists/$SUITE" >"dists/$SUITE/Release"
gpg --batch --yes --local-user "$KEY" --clearsign -o "dists/$SUITE/InRelease" "dists/$SUITE/Release"
gpg --batch --yes --local-user "$KEY" --armor --detach-sign -o "dists/$SUITE/Release.gpg" "dists/$SUITE/Release"

gpg --batch --export "$KEY" >"$KEYRING.gpg"
gpg --batch --armor --export "$KEY" >"$KEYRING.asc"
cat >bimcraft.sources <<SRC
Types: deb
URIs: $BASE_URL
Suites: $SUITE
Components: $COMPONENT
Signed-By: /etc/apt/keyrings/$KEYRING.gpg
SRC
cat >index.html <<HTML
<!doctype html>
<meta charset="utf-8">
<title>BIMCraft APT repository</title>
<h1>BIMCraft APT repository</h1>
<p>For Debian, Ubuntu and derivatives (amd64, arm64, riscv64):</p>
<pre>sudo install -d -m 0755 /etc/apt/keyrings
sudo curl -fsSLo /etc/apt/keyrings/$KEYRING.gpg $BASE_URL/$KEYRING.gpg
sudo curl -fsSLo /etc/apt/sources.list.d/bimcraft.sources $BASE_URL/bimcraft.sources
sudo apt update &amp;&amp; sudo apt install bimcraft</pre>
<p><a href="https://github.com/IPedrax/BIMCraft">BIMCraft on GitHub</a></p>
HTML
echo "wrote $SITE: ${archs[*]}, $(find pool -name '*.deb' | wc -l) packages, $(du -sh pool | cut -f1) of pool"
