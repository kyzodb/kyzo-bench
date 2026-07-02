---
name: verify-the-number
description: The verify-with-run discipline for a benchmarks repo. Use whenever making any claim about what a rig does, what a run produced, what an opponent's configuration is, or what a result file contains. Every such claim must be backed by a real run whose output is quoted, or by reading the actual file.
---

# Verify the number

Every claim in this repo is backed by evidence produced in this session, or it is not made. In a
benchmarks repo this discipline is not hygiene; it is the product. A published number that was
asserted rather than produced is fabrication.

## The rule

- **A claim about what a run produced** requires the actual run, with the command and its output (or
  the failing tail) quoted. Never report a number from memory or from a previous session.
- **A claim about an opponent's configuration or version** requires reading the rig's pinned config
  in this session, or querying the running opponent (`--version`, config dump) and quoting it.
- **A claim about a result file's contents** requires reading that file in this session.
- **A claim about reproducibility** requires an actual reproduction from a clean state, not an
  inspection of the script that ought to reproduce it.
- If a claim cannot be verified (opponent not installed, dataset not fetched, engine not yet green),
  **say so plainly** and state what was checked instead. An unverifiable claim stated as fact is
  worse than no claim.

## Exit codes, not pipe output

A command piped into grep/tail reports the LAST pipe stage's exit code: `run-bench | tee log` looks
green while the bench failed. Check the command's own exit code explicitly (e.g. `${pipestatus[1]}`)
for every gating claim. A green that is not exit-code-verified is not a green — this exact failure
produced a false commit on the engine project.

## A completed run proves nothing about itself

- **Sanity-check both sides before trusting a comparison.** A rig that misconfigures the opponent
  produces a clean-looking run and a worthless number. Verify the opponent actually did the work
  (row counts, recall checks, answer hashes) before reading the timing.
- **Wrong answers void the run.** Timing is only meaningful over verified-correct output. Every rig
  needs a correctness check on both sides before its stopwatch counts.

## Reporting

- Quote the actual command and its result. Never summarize a failure as a pass or a partial run as a
  whole one.
- If a run was skipped, the report says it was skipped and why.
- Scope every claim explicitly: which dataset, which scale factor, which hardware, warm or cold.
