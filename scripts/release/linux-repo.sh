#!/usr/bin/env bash
# Builds Teitunnel's apt and dnf repositories from release packages:
#
#   scripts/release/linux-repo.sh <packages-dir> <out-dir> <base-url>
#
# <packages-dir> holds the .deb and .rpm files of the releases to publish. <out-dir>
# receives:
#   teitunnel.asc               the public key (the one committed in apps/web/public/linux)
#   teitunnel.repo              dnf/zypper repository file
#   deb/dists/stable/…          Release, InRelease, Release.gpg, Packages per architecture
#   deb/pool/main/…             the .deb files
#   rpm/…                       the .rpm files (signed) and repodata/ (repomd.xml signed)
#
# Signs with the secret key in GPG's keyring whose fingerprint matches the committed
# public key (PUBLIC_KEY overrides its path, for tests), so a wrong key in the secret
# fails here instead of breaking every install.
# Needs: gpg, apt-ftparchive (apt-utils), createrepo_c, rpmsign (rpm).
set -euo pipefail

packages=$1 out=$2 base=${3%/}
here=$(cd "$(dirname "$0")/../.." && pwd)
public=${PUBLIC_KEY:-"$here/apps/web/public/linux/teitunnel.asc"}

fingerprint=$(gpg --show-keys --with-colons "$public" | awk -F: '$1 == "fpr" { print $10; exit }')
[ -n "$fingerprint" ] || { echo "No key in $public" >&2; exit 1; }
gpg --list-secret-keys "$fingerprint" >/dev/null 2>&1 ||
  { echo "The signing key ($fingerprint) isn't in GPG's keyring" >&2; exit 1; }
sign=(gpg --batch --yes --pinentry-mode loopback --local-user "$fingerprint")

shopt -s nullglob
debs=("$packages"/*.deb)
rpms=("$packages"/*.rpm)
[ ${#debs[@]} -gt 0 ] && [ ${#rpms[@]} -gt 0 ] || { echo "No .deb or .rpm in $packages" >&2; exit 1; }

rm -rf "$out/deb" "$out/rpm"
mkdir -p "$out/deb/pool/main" "$out/rpm"
cp "$public" "$out/teitunnel.asc"

# apt: one suite, "stable", with a component "main" for amd64 and arm64.
cp "${debs[@]}" "$out/deb/pool/main/"
(
  cd "$out/deb"
  for arch in amd64 arm64; do
    dir="dists/stable/main/binary-$arch"
    mkdir -p "$dir"
    apt-ftparchive --arch "$arch" packages pool >"$dir/Packages"
    gzip -9kn "$dir/Packages"
  done
  apt-ftparchive \
    -o APT::FTPArchive::Release::Origin=Teitunnel \
    -o APT::FTPArchive::Release::Label=Teitunnel \
    -o APT::FTPArchive::Release::Suite=stable \
    -o APT::FTPArchive::Release::Codename=stable \
    -o APT::FTPArchive::Release::Architectures="amd64 arm64" \
    -o APT::FTPArchive::Release::Components=main \
    -o APT::FTPArchive::Release::Description="Teitunnel, the Cloudflare Tunnel app" \
    release dists/stable >dists/stable/Release.tmp
  mv dists/stable/Release.tmp dists/stable/Release
  "${sign[@]}" --clearsign -o dists/stable/InRelease dists/stable/Release
  "${sign[@]}" --armor --detach-sign -o dists/stable/Release.gpg dists/stable/Release
)

# dnf: every package signed, and the metadata signed too (gpgcheck and repo_gpgcheck).
cp "${rpms[@]}" "$out/rpm/"
rpmsign --addsign \
  --define "_gpg_name $fingerprint" \
  --define "__gpg /usr/bin/gpg" \
  "$out"/rpm/*.rpm >/dev/null
createrepo_c --quiet "$out/rpm"
"${sign[@]}" --armor --detach-sign -o "$out/rpm/repodata/repomd.xml.asc" "$out/rpm/repodata/repomd.xml"

cat >"$out/teitunnel.repo" <<EOF
[teitunnel]
name=Teitunnel
baseurl=$base/rpm
enabled=1
gpgcheck=1
repo_gpgcheck=1
gpgkey=$base/teitunnel.asc
EOF

echo "Linux repositories: ${#debs[@]} .deb, ${#rpms[@]} .rpm, signed by $fingerprint"
