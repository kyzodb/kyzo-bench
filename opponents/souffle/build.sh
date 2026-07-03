#!/usr/bin/env bash
# Builds the pinned Souffle opponent from source, per its own build docs
# (https://souffle-lang.github.io/build). Compiled from the exact release tag;
# the resulting binary lands in opponents/souffle/dist/bin/souffle (gitignored).
#
# Pin: Souffle 2.5 (released 2025-03-24), the latest release at rig time.
set -euo pipefail

SOUFFLE_TAG="2.5"
HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
SRC="$HERE/src"
DIST="$HERE/dist"

if [[ -x "$DIST/bin/souffle" ]]; then
    echo "already built: $("$DIST/bin/souffle" --version | head -1)"
    exit 0
fi

if [[ ! -d "$SRC" ]]; then
    git clone --depth 1 --branch "$SOUFFLE_TAG" \
        https://github.com/souffle-lang/souffle.git "$SRC"
fi
cd "$SRC"
echo "souffle source at: $(git rev-parse HEAD) (tag $SOUFFLE_TAG)"

# Default domain width (32-bit), exactly as Souffle ships. The first build
# of this script set SOUFFLE_DOMAIN_64BIT=ON, which is non-default and
# slower; the fairness review flagged it and every baseline built with it
# was discarded and re-run. All workload fact values fit 32 bits.
cmake -S . -B build -G Ninja \
    -DCMAKE_BUILD_TYPE=Release \
    -DCMAKE_INSTALL_PREFIX="$DIST"
cmake --build build -j "$(nproc)"
cmake --install build

"$DIST/bin/souffle" --version
