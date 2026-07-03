#!/usr/bin/env bash
# Fetches the FTS bench corpus: 40 public-domain books from Project
# Gutenberg by pinned ID, hash-recorded. Real English prose, stable IDs,
# reproducible by a stranger. Documents for the bench are the books split
# into paragraphs (done deterministically by the rig, not here).
set -euo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
DEST="$HERE/../../datasets/gutenberg"
mkdir -p "$DEST"

# Pinned corpus: well-known long works, plain-text UTF-8. Changing this
# list is changing the dataset.
IDS=(
    11 76 84 98 158 161 174 345 768 1080
    1184 1232 1260 1342 1400 1661 1952 2542 2554 2600
    2701 3207 4300 5200 6130 7370 8800 10007 16389 20228
    24022 25344 26184 27827 28054 30254 35899 41445 42108 43453
)

for id in "${IDS[@]}"; do
    f="$DEST/pg$id.txt"
    if [[ ! -s "$f" ]]; then
        echo "fetching pg$id…"
        curl -fL --retry 3 -o "$f" "https://www.gutenberg.org/cache/epub/$id/pg$id.txt"
        sleep 1 # be polite to the mirror
    fi
done

sha256sum "$DEST"/pg*.txt > "$DEST/corpus.sha256"
echo "corpus: $(command ls "$DEST"/pg*.txt | wc -l) books, $(du -sh "$DEST" | cut -f1)"
