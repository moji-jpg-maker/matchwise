# M7b status addendum: versioned scoring configs, exposure tracking, and one gate

Second slice of M7b. The first slice ([`M7B_STATUS.md`](./M7B_STATUS.md)) was the outcome *dataset*. A backend slice
that added **versioned scoring configurations** and **exposure tracking** had already landed, but it was invisible:
the commands were registered, yet nothing in the interface reached them, the TypeScript types did not describe them,
and the ranking gate existed as two separate hard-coded `100`s. This slice closes that gap. It adds **no ranking
logic** -- ranking v1, learning from outcomes, and comparing a learned ordering against the deterministic baseline
remain out of scope.

## What it does
- **Scoring configs are editable.** A rule set says *what to measure* (dimensions, rules, weights); a scoring config
  says *how to read the result* -- the strong cutoff, the mixed cutoff, and the neutral baseline that a score resting
  on thin evidence is pulled towards. `/scoring` lists them, archived ones behind a toggle, each row showing the
  numbers in force and how many rule sets use it.
- **Each config is versioned, and versions are append-only.** Saving a config creates a new version; an old one is
  never rewritten, because a stored snapshot names the exact version that judged it. The editor shows which version
  it is displaying, keeps a per-save note in the history, and warns that saving an old version appends rather than
  edits it.
- **Validation is live and the backend is the authority.** `mm_validate_scoring_config` runs debounced as the three
  numbers change and its sentences are shown as they would be on save. A half-typed number cannot be sent at all, so
  the editor reports that locally and holds the save until the field parses.
- **A rule set can be pointed at a config.** The rule-set editor gained a selector; the chosen id now travels with
  `mm_save_rule_set`, which previously never sent it, so a rule set saved from the interface could not have a config
  attached at all. The config is a property of the rule set rather than of a version: switching it re-reads later
  results under the new numbers, while every stored scorecard keeps the version that actually judged it.
- **The ranking gate is one number, reported and not enforced.** `RANKING_MINIMUM` now lives once, in
  `matchmaking-core`, and is quoted by both `mm/exposure_repo.rs` (the readiness view) and `mm/export_commands.rs`
  (the dataset summary, via a new `minimum_finished` field). The export page dropped its local `LEARNING_MINIMUM` and
  reads the value from the command result, so the page that counts finished matches and the page that states the gate
  cannot drift apart.
- **The gate is surfaced, read-only, and says so.** `/scoring` shows the gate sentence, finished against needed,
  how many matches carry a stored scorecard, candidate pairs shown, and the outcome breakdown -- with an explicit
  statement that nothing there produces an ordering, fits a model, or re-orders a search.
- **Exposure tracking is described where it matters.** The card explains that what was *shown* is recorded, not only
  what was decided, because learning from outcomes alone would teach the matchmaker's browsing order as if it were
  preference.
- **Archiving is a label, not a delete.** An archived config disappears from the default list while rule sets already
  using it keep resolving it.

## Where it lives
- `matchmaking-core/src/export.rs` -- `RANKING_MINIMUM`, the one gate constant, exported from the crate root.
- `frontend/src-tauri/src/mm/scoring_config_repo.rs`, `scoring_config_commands.rs`, `exposure_repo.rs` -- the storage,
  the 7 `mm_*` commands, and the readiness/summary views (already present from the backend slice).
- `frontend/src-tauri/src/mm/rules_commands.rs` -- `mm_save_rule_set` takes `scoring_config_id`.
- `frontend/src-tauri/src/mm/export_commands.rs` -- `DatasetSummary.minimum_finished`.
- `frontend/src/types/matchmaking.ts` -- `ScoringConfig`, `ScoringConfigSummary`, `ScoringConfigView`, `VersionInfo`,
  `ExposureSummary`, `RankingReadiness`, `scoring_config_id` on the rule-set types, and the scoring-config fields on
  `MatchDetail`, all mirroring the serde output field for field.
- `frontend/src/app/scoring/page.tsx` -- the list: configs, archive toggle, the read-only ranking gate.
- `frontend/src/app/scoring-config/page.tsx` -- the editor: live validation, save as new version, version history.
- `frontend/src/app/rule-set/page.tsx` -- the scoring-config selector, and `scoring_config_id` on save.
- `frontend/src/app/export/page.tsx` -- reads the gate from the command instead of hard-coding it.
- `frontend/src/components/Sidebar/index.tsx` -- a **Scoring** entry in both the expanded list and the collapsed rail.

## Verified here
- `cargo test -p matchmaking-core` -- 84 tests pass, including the export formatting and round-trip work unchanged by
  this slice.
- `cargo test --lib mm::` in `frontend/src-tauri` -- 36 tests pass, including the ranking-gate test
  (`readiness_gate_counts_recorded_outcomes_at_zero_99_100_and_101`) untouched.
- Two new tests. `the_dataset_summary_quotes_the_one_ranking_gate` asserts the summary reports 100, that it equals
  `RankingReadiness.minimum_finished`, and that it equals the community constant -- so the two views are tied to one
  number rather than agreeing by coincidence -- and that an empty dataset is summarised rather than refused, since the
  gate is reported and not applied. `a_scoring_config_creates_at_v1_and_versions_append_while_old_ones_stay_readable`
  drives the create/version path no other test covered: v1 on create, v2 appended on save, v1 still readable and
  identical after the save, a missing version yielding nothing, a rule set pointed at the config and reported in
  `used_by`, and archiving hiding the config from the list while `effective_for_rule_set` still resolves it.
- `pnpm exec tsc --noEmit` in `frontend/` is clean.

## Not verified / limits
- **The gate stays 100 and still gates nothing.** It is consolidated into one constant and reported in two places; no
  ranking reads it to make a decision. It is a placeholder threshold, chosen rather than derived -- ranking v1 is where
  it would start to matter, and it should be revisited before then.
- **No React tests, and no click-through.** These pages have no tests, consistent with the export and rule-set pages;
  they were type-checked and their commands exercised in Rust tests, but not driven in a packaged build. UI defects
  that type-check (layout, a stale value after a save) would not have been caught here.
- **Attaching a config does not retroactively re-read anything.** Switching a rule set's config changes how later
  results are read; already-stored scorecards keep the version that judged them, which is the point, but it means two
  matches in the same list can be read by different numbers -- the reason the version is recorded per snapshot.
- **The exposure log is described, not used.** Nothing consumes it yet; the readiness view counts it.
- **Ranking v1 is untouched**: no learned ordering, no comparison against the deterministic baseline, no change to how
  searches are ordered.
