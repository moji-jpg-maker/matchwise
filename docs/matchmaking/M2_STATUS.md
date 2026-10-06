# M2 status: configurable rule engine (first slice)

## In `matchmaking-core` (30 tests pass in total)
- **Rules are data.** A rule has: a condition tree (`expr`: AND/OR/NOT, comparisons, ranges, "field + offset", is-filled-in, IF/THEN), a kind
  (`hard` = excludes the pair when false, `soft` = weighted score), `weight`, optional `group`, `priority`, `enabled`, and two features added in M2:
  - **`when` (conditional rules).** The rule applies only if this holds. False = the rule is skipped (it neither passes nor fails and is not scored);
    unknown = reported as unknown, never assumed. This fixes a real problem with plain IF/THEN, where a soft rule whose condition was false
    would have counted as "met" and inflated the score.
  - **`scope`.** `pair` rules are symmetric and run once; `directional` rules read "A's partner is B" and run in both directions (the plan's
    "candidate age 25-30 -> partner 0-8 years older" example).
- **Rule sets** add `group_weights` (multiplier per group, so a matchmaker can triple "location" without editing rules) and `min_score` (threshold).
  Results are ordered by priority.
- **Validation** (`validate_ruleset`): unknown fields, type mismatches (text vs number, ordering on text, offsets on non-numbers), options that do not exist
  on a choice field, repeating-record fields, empty AND/OR, duplicate ids, bad weights/thresholds, and depth/size limits. A stored rule set always validates.
- **Evaluation.** `evaluate_ruleset_mutual` runs one rule set on a pair; `evaluate_match` combines it with both people's partner preferences into one
  `MatchEvaluation`: eligible, needs-info, provisional score (mean of the rule-set score and the two preference scores), `meets_threshold`, and **coverage**.
- **Coverage** is the share of applicable soft-rule weight that could actually be evaluated. A candidate with one known field can score 100 while coverage is
  29%; the app shows "based on N% of the checks" and uses coverage as the tie-break after score. It does not yet discount the score itself (M3).
- `default_ruleset()`: starter rules (children acceptance, age gap, same city, religion when both highly observant, and the plan's age-window example switched off).

## In the app (the real `mm` command code compiled and ran end to end against in-memory SQLite in a scratch crate with a Tauri stand-in; not compiled inside the Tauri build)
- Migration `20261007000000_add_matchmaking_rule_sets.sql`: `mm_rule_sets` + immutable `mm_rule_set_versions` (every save creates version N+1; older versions stay viewable).
  The starter rule set is created once (a marker prevents re-creating it after it is archived).
- Commands: `mm_list_rule_sets`, `mm_get_rule_set` (any version), `mm_validate_rule_set` (live), `mm_save_rule_set` (validates, refuses invalid, unique names, audit log
  records version number only), `mm_archive_rule_set`, `mm_evaluate_match` (two chosen people), `mm_find_matches` (ranks all active candidates for one person; can also
  return excluded candidates with plain-language reasons).
- UI (`tsc` passes; helper logic and the TS-to-Rust JSON contract tested): `/rules` (list, archive/restore), `/rule-set` (editor with a recursive condition-tree builder,
  "applies only when", scope, groups, priority, group weights, minimum score, live validation messages per rule, a "Test on two people" panel, version history with
  "view an older version"), and a "Potential matches" panel on each profile.

## Known limits
- No automated tests for the Tauri command wrappers themselves (the logic under them is covered by the core tests and the scratch end-to-end run).
- `mm_find_matches` evaluates every active profile in memory: fine for thousands, not for very large pools; caching arrives with M3/M4.
- The combined score is provisional. Multi-dimension scores (values, personality, lifestyle, ...) and score-confidence weighting are M3.
- Rules cannot yet reference repeating records (e.g. "youngest child"), and there is no import/export of rule sets or rule-set diff view.
- Role-based permissions on who may edit rule sets are not implemented.
