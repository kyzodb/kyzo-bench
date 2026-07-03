#!/usr/bin/env python3
"""One-time export of the SIFT1M HDF5 into flat little-endian binary files
so the Rust kyzo-runner needs no HDF5 dependency: train.f32 (n*dim f32),
test.f32 (q*dim f32), neighbors.i64 (q*100 i64), plus a shape line in
shape.txt (`n dim q k_truth`). Byte-for-byte deterministic given the
hash-pinned dataset."""

import sys
from pathlib import Path

import h5py
import numpy as np

here = Path(__file__).resolve().parent
data = here.parent.parent / "datasets" / "vector"
out = data / "flat"
out.mkdir(exist_ok=True)

with h5py.File(data / "sift-128-euclidean.hdf5", "r") as f:
    train = np.asarray(f["train"], dtype="<f4")
    test = np.asarray(f["test"], dtype="<f4")
    neighbors = np.asarray(f["neighbors"], dtype="<i8")

train.tofile(out / "train.f32")
test.tofile(out / "test.f32")
neighbors.tofile(out / "neighbors.i64")
(out / "shape.txt").write_text(
    f"{train.shape[0]} {train.shape[1]} {test.shape[0]} {neighbors.shape[1]}\n"
)
print(f"exported: n={train.shape[0]} dim={train.shape[1]} "
      f"q={test.shape[0]} k_truth={neighbors.shape[1]}")
sys.exit(0)
