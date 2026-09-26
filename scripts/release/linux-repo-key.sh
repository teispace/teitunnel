#!/usr/bin/env bash
# Creates the key that signs Teitunnel's apt and dnf repositories. Run once, by a
# maintainer, from the repository root:
#
#   scripts/release/linux-repo-key.sh
#
# It stores the private key as the LINUX_REPO_GPG_KEY secret of the `release` environment,
# writes the public key to apps/web/public/linux/teitunnel.asc (commit it), and leaves one
# backup of the private key, teitunnel-linux-repo.key.asc: put it in your password manager
# and delete it. RSA 4096 without expiry, so every rpm and apt version can check it and
# installed machines never need a new key.
set -euo pipefail

public=apps/web/public/linux/teitunnel.asc
backup=teitunnel-linux-repo.key.asc
[ -f "$public" ] && { echo "$public already exists: the key was created before." >&2; exit 1; }
for tool in gh gpg; do
  command -v "$tool" >/dev/null || { echo "Needs $tool." >&2; exit 1; }
done

GNUPGHOME=$(mktemp -d)
export GNUPGHOME
trap 'rm -rf "$GNUPGHOME"' EXIT
gpg --batch --quiet --passphrase '' \
  --quick-gen-key "Teitunnel packages <info@teispace.com>" rsa4096 sign never
fingerprint=$(gpg --list-keys --with-colons | awk -F: '$1 == "fpr" { print $10; exit }')

mkdir -p "$(dirname "$public")"
gpg --armor --export "$fingerprint" >"$public"
(umask 077 && gpg --armor --export-secret-keys "$fingerprint" >"$backup")
gh secret set LINUX_REPO_GPG_KEY --env release <"$backup"

echo "Key $fingerprint"
echo "  public key:  $public (commit it)"
echo "  secret:      LINUX_REPO_GPG_KEY in the release environment"
echo "  backup:      $backup: move it to your password manager, then delete it"
