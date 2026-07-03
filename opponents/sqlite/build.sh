#!/usr/bin/env bash
# Builds the pinned SQLite opponent from the official autoconf amalgamation,
# hash-verified against the sqlite.org download page (SHA3-256, the hash
# SQLite itself publishes). Produces dist/bin/sqlite3.
#
# Pin: 3.53.3 (2026-06-26), the current release at the time the OLTP bench
# was built. Changing the pin is changing the opponent; it must show in the
# diff of this file.
set -euo pipefail

VERSION=3530300
HUMAN_VERSION=3.53.3
SHA3_256=98f2b3f3c11be6a03ea32346937b032c2472ebbd7a716bed36ca2f5693e7ce8b

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
SRC="$HERE/src"
DIST="$HERE/dist"
TARBALL="$HERE/sqlite-autoconf-$VERSION.tar.gz"

if [[ -x "$DIST/bin/sqlite3" ]]; then
    echo "already built: $("$DIST/bin/sqlite3" --version)"
    exit 0
fi

if [[ ! -f "$TARBALL" ]]; then
    curl -fL --retry 3 -o "$TARBALL" \
        "https://www.sqlite.org/2026/sqlite-autoconf-$VERSION.tar.gz"
fi

got="$(openssl dgst -sha3-256 -r "$TARBALL" | cut -d' ' -f1)"
if [[ "$got" != "$SHA3_256" ]]; then
    echo "SHA3-256 mismatch for $TARBALL: got $got want $SHA3_256" >&2
    exit 1
fi

rm -rf "$SRC"
mkdir -p "$SRC"
tar -xzf "$TARBALL" -C "$SRC" --strip-components=1

cd "$SRC"
# FTS5 is standard in essentially every shipped SQLite (distro packages,
# Python, most language drivers) and the FTS bench (kyzo#27) needs it.
./configure --prefix="$DIST" --enable-fts5
make -j"$(nproc)"
make install

echo "built: $("$DIST/bin/sqlite3" --version)"
