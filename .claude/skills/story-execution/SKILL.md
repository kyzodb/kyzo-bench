---
name: story-execution
description: The discipline for executing one bench or demo story from the shared KyzoDB board. Use when picking up any story. Enforces working from the board, one coherent end-state target, verify-the-number, hostile methodology review, and the anti-avoidance rules.
---

# Story execution (kyzo-bench)

The work is a set of stories on the shared board: org project **KyzoDB Migration**
(`gh project item-list 1 --owner kyzodb`), stories filed as issues on `kyzodb/kyzo` labeled
`trials` — benches #22–#28 (epic #39), demos #35–#38 (epic #41). Execute exactly one at a time.

## The mantra — chant it before every piece of new code
**Do the work. Prove the work. Tell the truth about the work.** The tells: relief means escaping;
narrating means lying; defending before re-examining means inverting; converging to the last thing
said means the world model is lost. Appearance is the enemy; reality is the only client.

## Steps
1. **Move the story to In Progress** on the board before any work starts. Sequence the work
   **hardest-first**: before ordering tasks, ask "is this dependency order or comfort order?" — the
   hardest item startable now comes first.
2. **Read the story, this repo's `CLAUDE.md`, and the engine context.** Engine context is
   `../kyzo` if checked out (its `README.md`, `CLAUDE.md`, `REFACTOR.md`), else fetch from
   `kyzodb/kyzo` with `gh`. Do the story's stated scope and nothing else.
3. **One coherent target.** A rig lands whole: opponent pinned and tuned, dataset fetch scripted,
   seeds recorded, correctness checks on both sides, invocation documented. Never land a rig "to
   finish later" — a half-rig that can emit a number is a fabrication machine.
4. **Fairness before speed.** Configure the opponent to its own documentation before optimizing
   anything on our side. The fairness rules in `.claude/rules/methodology.md` are the load-bearing
   invariant of this repo.
5. **Verify, never assert.** Every claim about a run, a config, or a result follows
   `.claude/skills/verify-the-number/SKILL.md`. Quote commands and output; check exit codes.
6. **Commits seal, they don't checkpoint.** Nothing commits until its hostile pass has cleared AND
   its findings are fixed — one commit per sealed unit.
7. **No landing without a hostile review.** Every rig, methodology, or results change is attacked by
   `methodology-fairness-reviewer` briefed to REFUTE, and its findings fixed; fixes-of-findings get
   their own hostile pass.
8. **Deferral is sabotage unless blocked.** Work leaves the current story only with a named hard
   technical blocker. "The engine isn't green yet" blocks final numbers (`kyzodb/kyzo#4`), not
   harness building, dataset ingestion, or opponent baselines — those are startable now.
9. **Do not narrow scope to look done.** An expected-loss run skipped, a standard workload quietly
   subsetted, or a result held back is this repo's canonical sabotage. Whole workload, or say it is
   partial.
10. **Honor the DoD.** A bench story is done when its rig reproduces from a clean clone, both sides
    pass correctness checks, and the raw results (wins and losses) are committed under `results/`.
11. **Nothing public without a go.** Pushes, published results, and issues on opponents' repos wait
    for an explicit go from the maintainer.

## Dependency notes
Harnesses parallelize freely; the seven benches and four demos are mutually independent. Final
KyzoDB numbers mostly gate on engine product green (`kyzodb/kyzo#4`). The Raspberry Pi replay (#36)
reuses the determinism-campaign harness (engine #30); the browser flex (#37) needs the WASM binding
(engine #10); ask-it-why (#38) needs provenance and time travel end-to-end.
