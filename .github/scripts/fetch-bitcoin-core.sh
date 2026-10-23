#!/usr/bin/env bash
# Downloads Bitcoin Core for Linux and fails unless it checks out:
#   - SHA256SUMS carries at least CORE_MIN_SIGNATURES good signatures from
#     the builder keys in bitcoin-core/guix.sigs, and
#   - the tarball matches both SHA256SUMS and the pinned CORE_TARBALL_SHA256.
# Then adds BITCOIN_BIN and BITCOIND_EXE to $GITHUB_ENV.
set -euo pipefail
: "${CORE_VERSION:?}" "${CORE_TARBALL_SHA256:?}" "${CORE_MIN_SIGNATURES:=3}"

mkdir -p "$RUNNER_TEMP/core-release" && cd "$RUNNER_TEMP/core-release"
tarball="bitcoin-${CORE_VERSION}-x86_64-linux-gnu.tar.gz"
base="https://bitcoincore.org/bin/bitcoin-core-${CORE_VERSION}"
for f in "$tarball" SHA256SUMS SHA256SUMS.asc; do
  curl -sSfL -o "$f" "$base/$f"
done

git clone -q --depth 1 https://github.com/bitcoin-core/guix.sigs
export GNUPGHOME="$RUNNER_TEMP/gnupg-core"
mkdir -p -m 700 "$GNUPGHOME"
gpg --batch --quiet --import guix.sigs/builder-keys/*.gpg 2>/dev/null || true
gpg --batch --status-fd 1 --verify SHA256SUMS.asc SHA256SUMS > gpg-status 2>/dev/null || true
good=$(grep -c '^\[GNUPG:\] GOODSIG' gpg-status || true)
if [ "$good" -lt "$CORE_MIN_SIGNATURES" ]; then
  echo "::error::Bitcoin Core's SHA256SUMS has $good good builder signatures; need $CORE_MIN_SIGNATURES"
  exit 1
fi
echo "Bitcoin Core's SHA256SUMS: $good good builder signatures"

grep " ${tarball}\$" SHA256SUMS | sha256sum -c -
echo "${CORE_TARBALL_SHA256}  ${tarball}" | sha256sum -c -
tar xzf "$tarball"
echo "BITCOIN_BIN=$PWD/bitcoin-${CORE_VERSION}/bin" >> "$GITHUB_ENV"
echo "BITCOIND_EXE=$PWD/bitcoin-${CORE_VERSION}/bin/bitcoind" >> "$GITHUB_ENV"
