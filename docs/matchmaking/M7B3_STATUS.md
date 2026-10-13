# M7b status: ranking v1, comparing an ordering against the deterministic baseline

Third slice of M7b. The first ([`M7B_STATUS.md`](./M7B_STATUS.md)) was the outcome *dataset*; the second
([`M7B2_STATUS.md`](./M7B2_STATUS.md)) was versioned scoring configs, exposure tracking, and one gate. This slice is
**ranking v1 as a question, not as a model**: it puts two orderings against the recorded outcomes and reports which
one agreed better with what actually happened.

Two orderings are on trial, and only these two:
- **baseline** -- the confidence-adjusted `ranking_score`, which is what orders a search today. The adjustment pulls a
  score resting on thin evidence towards a neutral prior, so a well-documented pair can outrank a better-scoring but
  poorly-documented one.
- **candidate** -- the raw coverage-weighted `overall`, before that adjustment.

The question is narrow and answerable: if the adjustment is worth its complexity, it must beat the raw number on
recorded outcomes. If it does not, it is costing ordering quality for nothing. Nothing here changes `mm_find_matches`,
fits a model, or re-orders any search -- see the limits at the end.

## What it does
- **One row per finished match, carrying both scores.** Every tracked pair with a recorded outcome is reduced to two
  numbers -- the newest snapshot's `overall` and its `ranking_score` -- and one label: did the pair progress?
- **The label is coarser than the nine outcomes, on purpose.** Progressed = `interested`, `first_conversation`,
  `first_meeting`, `continued`, `relationship_formed`, `married`; not progressed = `not_interested`, `stopped`. The
  comparison needs a direction, not a stage: a pair that met twice is not more progressed than one that met once for
  the question *which of these two candidates worked out*. A finer ranking of outcomes would be inventing a scale the
  plan does not define.
- **`unknown` is excluded, never counted as a failure.** An unrecorded move is not a move that went badly, and a
  match still in progress has no label at all yet. Both are dropped and **counted as dropped** so the shortfall is
  visible rather than silently deflating a rate.
- **The verdict is pairwise concordance.** For each pair whose outcomes differ, the ordering gets one point if it put
  the progressed match first and none if it did not; `concordance = concordant / (concordant + discordant)`. Ties are
  neither wins nor losses, so they leave the rate and are reported as their own count.
- **No rate is `None`, never `0`.** A set where every comparable pair tied, an empty history, and a history with no
  progressers all have *no concordance* -- reported as `null`. "Nothing to compare" and "compared and always wrong"
  are different findings, and the same blank-versus-zero convention the export already uses.
- **R-precision alongside it.** Of the top `k` rows by each score (`k` = the number of progressed rows), how many
  actually progressed. It answers the operational question a rate alone can miss: does the ordering put the pairs
  that worked out near the top?
- **The gate is quoted, not applied.** `mm_ranking_comparison` reads `RANKING_MINIMUM` through the existing readiness
  view, so the number stays one constant in one place. Below the gate the comparison is still shown, with a sentence
  saying it is there for inspection only; above it, a sentence saying it covers enough history to read as a finding.
- **Every sentence the interface shows comes from the backend.** The classification rule and the gate note are part
  of the payload, so the page cannot describe a rule the code does not apply.
- **The command writes nothing.** It is an aggregate read like `mm_ranking_readiness`, with no audit entry -- no rows
  leave the database and no content is exposed by it.

## Where it lives
- `matchmaking-core/src/ranking.rs` -- `Progress` (the classification), `ComparisonRow`, `OrderingResult`,
  `RankingComparison`, and `compare`. Pure: no DB, no Tauri, testable on its own the way `export.rs` is. 10 tests.
- `matchmaking-core/src/lib.rs` -- `pub mod ranking;` and the re-exports (`compare`, `ComparisonRow`,
  `OrderingResult`, `Progress`, `RankingComparison`, `classify` as `classify_outcome`).
- `frontend/src-tauri/src/mm/ranking_repo.rs` -- `RankingRepo::comparison_rows` reusing `ExportRepo::outcome_rows`
  (one row per match, newest snapshot joined), plus `ExclusionCounts` recording each drop reason.
- `frontend/src-tauri/src/mm/ranking_commands.rs` -- the `RankingComparison` payload, `CLASSIFICATION`, `gate_note`,
  `build_comparison`, and the `#[tauri::command] mm_ranking_comparison`, registered in `frontend/src-tauri/src/lib.rs`
  beside `mm_ranking_readiness`.
- `frontend/src-tauri/src/mm/ranking_tests.rs` -- 8 app-layer tests against a real pool.
- `frontend/src/types/matchmaking.ts` -- `OrderingResult`, `ExclusionCounts`, `RankingComparison`, field for field
  with the serde output (`null` where data may be absent).
- `frontend/src/app/scoring/page.tsx` -- the Ranking-gate card now fetches the comparison alongside the gate and shows
  both orderings: rate, counts, ties, and which one is in force.

- `cargo test -p matchmaking-core` -- 93 tests pass, 10 of them new in `ranking.rs` (the library target holds 20).
  The new ones: all nine outcomes classified (and unrecognised text excluded), agreement counted against disagreement,
  same-outcome pairs tied and kept out of the rate, a set with no progressers having no R-precision rather than zero, an
  empty set comparing to nothing, a row with no stored score skipped by one ordering while the other still places it,
  identical orderings producing identical results, and equal scores not counting as agreement.
- `cargo test --lib mm::` in `frontend/src-tauri` -- 44 tests pass. 8 are new: the drop rules leaving out a match with
  no outcome, an `unknown` outcome, or no snapshot while `mm_ranking_readiness` still counts them; the ordering that
  tracks the outcomes winning on crafted input and the same input with the two score columns swapped inverting the
  verdict (a golden-style direction test); an empty history compared rather than refused; the below-the-gate wording;
  a below-the-gate set still being compared and carrying the classification sentence it counted by; the payload
  matching the TypeScript shape field for field with `null` rather than `0`; and repeated reads writing **zero** audit
  rows.
- `pnpm exec tsc --noEmit` in `frontend/` is clean.

## Not verified / limits
- **It reports an ordering; it does not produce one.** `mm_find_matches` is untouched and searches are still ordered
  by `ranking_score`. This is the comparison the plan asks for before any learned ranking, and ranking stays disabled
  regardless of what it finds.
- **The gate stays 100 and still gates nothing here.** It is read and quoted, not enforced; below it the comparison is
  shown as inspection only. Ranking v1 is where it *would* start to matter, and it is still a placeholder chosen
  rather than derived.
- **Concordance is a small-sample figure and ties shrink it further.** With a few finished matches a rate moves for
  reasons other than which ordering is better; the counts are shown beside every rate for exactly that reason, and the
  gate sentence says whether it may be read as a finding. A proper significance treatment is not attempted here.
- **No model is fitted, so nothing is learned or persisted.** The comparison is recomputed on each read from the
  recorded history; there is no stored result and no migration.
- **Not audit-logged, deliberately.** It is an aggregate read and no row leaves the database; if read-access auditing
  parity with the export is wanted later, that is a separate decision.
- **No React tests and no click-through.** The scoring page is type-checked and the command is exercised in Rust
  tests, but not driven in a packaged build; a UI defect that type-checks (layout, a stale value) would not have been
  caught here.
