#!/usr/bin/env bash
# Fetches the LDBC Graphalytics datasets this bench runs on, into datasets/,
# and records the SHA-256 of exactly what was fetched. Datasets are never
# committed; results name these hashes.
#
# Source: https://datasets.ldbcouncil.org/graphalytics/ (LDBC's official
# dataset host). Each archive carries <name>.v/.e (vertex/edge lists) plus
# the Graphalytics properties file and reference outputs (*-BFS, *-PR, ...)
# used for correctness checking.
set -euo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
DEST="$HERE/../../datasets/graphalytics"
mkdir -p "$DEST"

DATASETS=(wiki-Talk kgs cit-Patents)

for name in "${DATASETS[@]}"; do
    archive="$DEST/$name.tar.zst"
    if [[ ! -f "$archive" ]]; then
        echo "fetching $name…"
        curl -fL --retry 3 -o "$archive" \
            "https://datasets.ldbcouncil.org/graphalytics/$name.tar.zst"
    fi
    sha256sum "$archive" | tee "$DEST/$name.tar.zst.sha256"
    if [[ ! -d "$DEST/$name" ]]; then
        mkdir -p "$DEST/$name"
        tar --zstd -xf "$archive" -C "$DEST/$name" --strip-components=0
    fi
done

echo "fetched into $DEST:"
command ls "$DEST"
