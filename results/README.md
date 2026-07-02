# results/ — the published record (APPEND-ONLY)

Raw benchmark and demo outputs land here and are never edited or deleted after commit. Corrections
are new files with a `supersedes:` header naming the flawed file and stating the flaw; the flawed
file stays.

Every result file is self-describing: engine commit, opponent name and exact version, dataset and
fetch hash, seed, hardware spec, date, and the exact command that produced it. Losing runs land with
the same ceremony as wins. See `.claude/rules/results-data.md`.
