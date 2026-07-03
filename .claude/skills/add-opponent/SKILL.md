---
name: add-opponent
description: The process for onboarding a new comparative subject (opponent database or engine) into one or more kyzo-bench benches. Use whenever a story adds a competitor that doesn't already have a rig, whether into one existing bench or across several. Enforces pin-first, fairness-before-wiring, and treats the output-format correctness bridge as the genuinely hard, per-workload step that no interface generalizes.
---

# Add opponent (kyzo-bench)

Onboarding a new competitor is not "implement an interface." Wiring code into `harness::Rig`
(kyzo#70) or the shared result envelope (`harness/envelope.py`) is one of seven gates, and it is
not the hard one. Jumping straight to it is how a rig ends up with an opponent that runs but was
never actually pinned, tuned, or checked for a fair fight — a number for a fight nobody agreed
the opponent showed up dressed for.

## The mantra

The bench exists to lose sometimes. An opponent onboarded in a way that makes that less likely —
a config quietly softened, a workload picked around its weak spot, a hard mode of theirs skipped
because it's inconvenient — is sabotage with better PR than editing `results/` directly.
Configure every opponent exactly as carefully as you would if you were rooting for it to win.
When an opponent's own marketing sounds like it's competing directly with KyzoDB's pitch, this
gets *more* rigor, never more suspicion of the opponent and never a thumb on the scale to protect
the narrative — configure it as if you had nothing riding on the outcome.

## The seven gates, in order

Work through these **per bench** the new opponent enters. An opponent that fits three of
kyzo-bench's benches is three passes through gates 3–7, sharing gates 1–2 once.

1. **Which bench(es), and what shape of comparison.** Read the opponent's own docs for what it
   actually does — never infer capability from marketing copy. For each candidate bench, decide
   one of three things and write down why: *fits* (cite the opponent's own docs for the
   capability that makes it comparable), *doesn't fit* (cite what's missing — don't force a
   workload the opponent's own maintainers wouldn't recognize as a fair test of their engine), or
   *unresolved* (name exactly what needs verifying, and verify it before gate 3 — "probably works
   like X" is not a scope decision). For each bench that fits, classify the comparison shape:
   subprocess-timed (`harness::Rig` once kyzo#70 lands), in-process library call (envelope only,
   `kuzu_baseline.py`-style), or multi-service concurrent (bespoke, `driver.py`-style). This
   decides gate 5's wiring shape before any code exists to plug into.
2. **Pin it, from its own release channel, in good faith.** One `opponents/<name>/` pin serves
   every bench the opponent appears in — don't re-pin per bench. Exact tagged/released version,
   hash-verified fetch (or, for a Rust crate available on crates.io, a version-pinned
   `Cargo.toml` dependency — not every opponent needs a `build.sh`; a source-available Rust crate
   embedded the way `opponents/tantivy-runner` embeds Tantivy is pinned by `Cargo.lock`, which is
   just as exact). State the pin where a version bump shows in the diff. Match the shape of
   `opponents/souffle/build.sh` and `opponents/sqlite/build.sh`: fetch or add at the tag, build or
   compile per the project's own documented instructions, verify the resulting binary or crate
   reports the version you pinned. Before writing a single line of rig code, confirm the
   opponent's license permits running and publishing comparative benchmark results — a
   benchmarking restriction is a hard blocker, not a fairness nuance to route around.
3. **Configure it the way its own documentation recommends — before touching our side.**
   `.claude/rules/methodology.md`'s test: would that project's maintainers sign off on this
   configuration? Read its performance-tuning docs if they exist and cite what you read in the
   bench's README. Open an issue on the opponent's own repo if a configuration choice is
   genuinely ambiguous in their docs, and wait for their answer rather than guessing in KyzoDB's
   favor.
4. **Bridge its output to the workload's canonical answer shape.** This is the gate with no
   interface, and it is the one that is genuinely hard per opponent and per workload. Every
   subject in a suite must produce output that hashes identically when it's right
   (`kyzo_bench_harness::canonical_answer` or `raw_answer`) — a new opponent's native output
   format (its own column order, row terminators, ranking ties) has to map into that exact shape
   without silently changing what's being compared. Prove it on the smallest workload in the
   suite first: run the new opponent and an existing subject side by side and confirm the hashes
   agree before scaling to the full workload set. Wiring gate 5 before this is proven is landing
   a number for an unverified answer.
5. **Wire the one bench-specific code arm.** For a `Rig`-shaped bench: one `match` arm in
   `prepare_subject` building its argv, plus adding its name to `SUBJECTS` — nothing in
   `harness/src/rig.rs` itself should need to change. For an in-process or multi-service bench:
   whatever that script's own call shape is, emitting the shared envelope from
   `harness/envelope.py`. If this step touches anything outside that one arm plus the opponent's
   own module, the shared runner's abstraction boundary is wrong for this case — fix the runner,
   don't special-case around it.
6. **Register and document it.** The bench's `README` gains the new subject in its subject list
   and its "Run it" section; the opponent's pin location states the version and how it was
   verified. A story is not done while its own docs don't mention the opponent that was just
   onboarded.
7. **Hostile review, then run and land — unconditionally.** `methodology-fairness-reviewer`
   reviews the pin, the configuration, and the correctness bridge (gates 2–4) before anything
   lands, briefed to refute the fairness of the comparison itself. Then run the full workload
   suite and commit every result under `results/`, wins and losses both, per
   `.claude/rules/results-data.md`. An onboarding story that only publishes the workloads the new
   opponent wins is the exact cherry-picking `CLAUDE.md` names as this repo's cardinal sin.

## What this process is not

- Not satisfied by gate 5 alone. A competitor that "runs" because someone wrote a
  `prepare_subject` arm without gates 2–4 first is not benchmarked, it's guessed at.
- Not a one-size-scope decision. Gate 1's honest answer may be "this opponent belongs in three of
  our seven benches, not seven" — say which three and why, in the story, rather than forcing it
  everywhere it could merely technically be made to run.
