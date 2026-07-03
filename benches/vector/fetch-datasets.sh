#!/usr/bin/env bash
# Fetches the ann-benchmarks SIFT1M dataset (128-dim SIFT descriptors,
# Euclidean): 1,000,000 base vectors, 10,000 queries, and the exact
# 100-nearest-neighbor ground truth, in ann-benchmarks' standard HDF5
# layout (train / test / neighbors / distances).
#
# Source: http://ann-benchmarks.com/sift-128-euclidean.hdf5 — the dataset
# every published ann-benchmarks result uses for this workload.
set -euo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
DATA="$HERE/../../datasets/vector"
mkdir -p "$DATA"

FILE="sift-128-euclidean.hdf5"
URL="http://ann-benchmarks.com/$FILE"
# SHA-256 of the file as fetched at rig time (2026-07-03); refuses a
# changed upstream.
SHA="dd6f0a6ed6b7ebb8934680f861a33ed01ff33991eaee4fd60914d854a0ca5984"

if [[ ! -f "$DATA/$FILE" ]]; then
    curl -fL --retry 3 -o "$DATA/$FILE.part" "$URL"
    mv "$DATA/$FILE.part" "$DATA/$FILE"
fi

echo "$SHA  $DATA/$FILE" | sha256sum -c - || {
    echo "hash mismatch: refusing $FILE" >&2
    exit 1
}
sha256sum "$DATA/$FILE" > "$DATA/$FILE.sha256"
echo "ok: $DATA/$FILE"
