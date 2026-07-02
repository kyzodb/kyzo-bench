# CLAUDE.md — kyzo-bench

This repo is the **public proving ground for [KyzoDB](https://github.com/kyzodb/kyzo)**: the
comparative benchmarks and the reproducible demos that put the engine up against the strongest
comparable systems, in the open. Every rig, every seed, every raw result, and every losing run is
published here. The engine's *self-referential* trials (determinism campaign, crash matrix, fuzzing
ledger, proof audit) live in the engine repo as tests; this repo is where KyzoDB meets opponents.

## The two repos and the one board

- **`kyzodb/kyzo`** is the engine. If it is checked out as a sibling (`../kyzo`), read its `README.md`,
  `CLAUDE.md`, and `REFACTOR.md` for engine context before doing anything here; otherwise fetch what
  you need with `gh` from `kyzodb/kyzo`. Never modify the engine from this repo. If a benchmark
  exposes an engine defect, file an issue on `kyzodb/kyzo`; the fix is engine work, not bench work.
- **The board is shared**: GitHub org project **KyzoDB Migration** (`gh project item-list 1 --owner
  kyzodb`). Benchmark and demo stories live as issues on `kyzodb/kyzo` labeled `trials`: benches
  #22–#28 under epic #39, demos #35–#38 under epic #41 (the trials epic #40 executes in the engine
  repo). Work only from the board. Each story is self-contained. Do not invent scope.

## What this repo must never do

The load-bearing invariant here is not key ordering; it is **fairness and the immutability of
published results**. The credibility of every number KyzoDB ever publishes rests on this repo being
unimpeachable.

- **Never cherry-pick.** Losing runs are committed and published alongside wins. Publishing only the
  favorable runs is this repo's version of narrowing scope to manufacture a clean answer: sabotage.
- **`results/` is append-only.** A committed raw-result file is never edited or deleted. Corrections
  are new files with a note pointing at what they supersede.
- **Opponents get their best game.** Every opponent is pinned to an exact released version, configured
  the way its own documentation recommends, and tuned in good faith. The test: would that project's
  maintainers sign off on the configuration? If unsure, open an issue asking them.
- **Everything reproducible.** Pinned versions, published seeds, recorded hardware, scripted dataset
  fetch. A number that cannot be reproduced from a clean clone does not get published.
- **No hype vocabulary.** The rigs and numbers speak; the audience draws the conclusion.

## How we work

Inherited from the engine repo, and load-bearing for the same reasons:

- **Verify, never assert.** Every claim about what a rig does, what a run produced, or what an
  opponent's configuration is must be backed by a real run or by reading the actual file. Quote the
  command and its output. Check exit codes, not pipe output. See
  `.claude/skills/verify-the-number/SKILL.md`.
- **Work from the board, one story at a time, hardest first.** See
  `.claude/skills/story-execution/SKILL.md`.
- **Nothing lands without a hostile review.** For this repo the decisive reviewer is
  `methodology-fairness-reviewer`, briefed to refute the fairness of the comparison itself.
- **A question is not a command.** Nothing public (pushes, published results, issues on opponents'
  repos) without an explicit go from the maintainer.

## Engine readiness

Harness building, dataset ingestion, and opponent baselines are startable now and parallelize across
stories. Final KyzoDB-side numbers mostly gate on the engine reaching product green
(`kyzodb/kyzo#4`); check the board before claiming a benchmark can produce its headline number.

## Layout

    benches/datalog/            kyzo#22  recursive Datalog vs Souffle / DDlog
    benches/graph-algorithms/   kyzo#23  LDBC Graphalytics vs Kuzu
    benches/snb/                kyzo#24  LDBC SNB Interactive, single-node
    benches/vector/             kyzo#25  ann-benchmarks + big-ann filtered track
    benches/oltp/               kyzo#26  embedded OLTP vs SQLite
    benches/fts/                kyzo#27  full-text vs Tantivy / FTS5
    benches/time-travel/        kyzo#28  as-of overhead vs history depth (we define it)
    demos/consistency-kill-shot/  kyzo#35
    demos/raspberry-pi-replay/    kyzo#36
    demos/browser-flex/           kyzo#37
    demos/ask-it-why/             kyzo#38
    results/                    append-only raw results (see .claude/rules/results-data.md)
    datasets/                   gitignored; fetch scripts only, never data
