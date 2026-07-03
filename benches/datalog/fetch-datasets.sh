#!/usr/bin/env bash
# Fetches the real-graph datasets for the recursive-Datalog bench from SNAP
# (https://snap.stanford.edu/data/), recording the SHA-256 of exactly what
# was fetched. Datasets are never committed; results name these hashes.
#
# The story (kyzo#22) also names Doop Java facts and Graspan dataflow
# graphs. Doop facts require running the Doop toolchain (JVM + a corpus);
# Graspan's datasets are hosted on a Google Drive folder, which cannot be
# fetched reproducibly by hash from a script. Both are tracked on the issue
# as follow-ups; SNAP graphs are the stable, hash-verifiable real inputs.
set -euo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
DEST="$HERE/../../datasets/snap"
mkdir -p "$DEST"

# p2p-Gnutella31 was tried and refused: its full transitive closure blows
# the 12 GiB cap in under a minute (billions of pairs). Gnutella08 is the
# same real network family at a closure size that fits the caps.
DATASETS=(wiki-Vote p2p-Gnutella08)

for name in "${DATASETS[@]}"; do
    gz="$DEST/$name.txt.gz"
    if [[ ! -f "$gz" ]]; then
        echo "fetching $name…"
        curl -fL --retry 3 -o "$gz" "https://snap.stanford.edu/data/$name.txt.gz"
    fi
    sha256sum "$gz" | tee "$DEST/$name.txt.gz.sha256"
    if [[ ! -f "$DEST/$name.txt" ]]; then
        gunzip -k "$gz"
    fi
done

echo "fetched into $DEST:"
command ls "$DEST"
