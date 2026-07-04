# results/ — the published record (APPEND-ONLY)

Raw benchmark and demo outputs land here and are never edited or deleted after commit. Corrections
are new files with a `supersedes:` header naming the flawed file and stating the flaw; the flawed
file stays.

Every result file is self-describing: engine commit, opponent name and exact version, dataset and
fetch hash, seed, hardware spec, date, and the exact command that produced it. Losing runs land with
the same ceremony as wins. See `.claude/rules/results-data.md`.

**Pin identity lives in the filename, for every subject alike.** `subject@version` names an
opponent's exact released version (`souffle@2.5`); `kyzo@<12-char-sha>` names the exact commit this
repo's own `Cargo.lock` resolved a `git`-pinned dependency on `kyzodb/kyzo` to — never a sibling
working tree. Until the engine cuts its first tagged release, every `kyzo@<sha>` result here is
pre-release: engine-team feedback, not a comparison against a published artifact (see
`.claude/rules/methodology.md`). Once a `v0.Y.Z` tag lands, headline comparisons re-run and land as
`kyzo@v0.Y.Z`, and that file is the first published-artifact comparison under this policy.
