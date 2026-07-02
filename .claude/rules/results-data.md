---
paths:
  - "results/**"
---
# Rule: results are append-only (PUBLISHED DATA)

`results/` is the published record. Its integrity is binary: either no committed result has ever
been altered, or the repo's word is worthless.

- **Never edit or delete a committed file under `results/`.** Not to fix a typo, not to reformat,
  not to "clean up". Git history proving results were rewritten is indistinguishable from fraud.
- **Corrections supersede.** A flawed result gets a new file with a `supersedes:` header naming the
  old one and stating the flaw; the old file stays.
- **Every result file is self-describing**: engine commit, opponent name and exact version, dataset
  and its fetch hash, seed, hardware spec, date, and the command that produced it. A result missing
  any of these does not land.
- **Raw before summary.** The raw run output lands first; summaries, curves, and plots are derived
  artifacts that name the raw files they were computed from.
- **Losing runs land with the same ceremony as wins.** The append-only rule exists precisely so
  that publishing a loss costs nothing and hiding one costs everything.
