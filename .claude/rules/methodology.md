---
paths:
  - "benches/**"
  - "demos/**"
---
# Rule: methodology fairness (THE LOAD-BEARING INVARIANT)

Every number this repo publishes is a claim KyzoDB's reputation stands on. The invariant is
**fairness**: an unfair win is worse than an honest loss, because one discovered unfair
configuration retroactively poisons every result in the repo.

- **Opponents are pinned and tuned in good faith.** Exact released version recorded in the rig,
  configuration per the opponent's own documentation, tuning that project's maintainers would sign
  off on. When their docs offer a performance guide, follow it and cite it in the rig's README.
- **KyzoDB is pinned exactly like an opponent — never a live checkout.** A `Cargo.lock`-resolved
  dependency on `kyzodb/kyzo` (a `git`+`rev` pin pre-release, a `=X.Y.Z` crates.io pin once the
  engine tags releases), never a path dependency on a sibling working tree someone else is actively
  editing. Hand-picking which dev commit to benchmark while every opponent is frozen at a released
  version is exactly the asymmetry this rule exists to prevent. Headline/published numbers come
  only from tagged releases; runs against a pre-release git-rev pin are engine-team feedback,
  labeled as such in the result's `notes`.
- **Same terms for both sides.** Same hardware, same dataset, same measurement window, warm/cold
  state declared. KyzoDB never gets a preparation step the opponent is denied.
- **Scope is declared, not implied.** Single-node scoped honestly as single-node; embedded engines
  compared as embedded engines; server-class opponents labeled server-class. A comparison across
  classes states the class difference in the rig README.
- **Seeds and hardware are recorded in the rig**, not in a wiki, not in a commit message. A run whose
  seed is lost is a run that did not happen.
- **Expected losses are still run and still published** (raw ANN vs FAISS, standalone FTS vs
  Tantivy). Skipping a fight we lose is cherry-picking by omission.
- **Engine defects found by a rig are engine work.** File on `kyzodb/kyzo`; never work around a wrong
  answer in the rig to make a run complete.

Any change under `benches/**` or `demos/**` that touches opponent configuration, dataset selection,
measurement methodology, or what gets recorded needs the `methodology-fairness-reviewer` pass before
it lands.
