# M3 status: multi-dimension scoring (first slice)

Replaces the single provisional percentage with a scorecard: **how compatible, in which respects, and how sure are we.**

## How it works (`matchmaking-core/src/scoring.rs`, 40 core tests pass in total)
- **Dimensions** follow the product plan: age / life stage, values, personality, family, children, religion, lifestyle, communication, relationship expectations,
  geographic compatibility, financial / practical, personal preferences. A rule joins a dimension through its `group` (a dimension key or a friendly alias such as
  `location`, `age`, `education`; anything else lands in "Other"). The partner preferences of both people form the *personal preferences* dimension.
- **Statuses** distinguish things a single number hides: `strong` (75+), `mixed` (50-74), `concern` (below 50 or a violated must-have), `clear` (only must-haves, all hold),
  `unknown` (rules exist but data is missing), `not_applicable` (rules exist but none applies to this pair), `not_assessed` (nothing measures this dimension).
  Personality, values, communication and relationship expectations show **not assessed** until questionnaire data and rules exist (M7); they are never faked.
- **Hard constraints** are a separate verdict: `pass`, `fail` (with the possible deal-breakers listed) or `needs_info` (with exactly which must-haves are undecided).
- **Scores.** `overall` = coverage-weighted mean of dimension scores (dimension weight = the rule set's `group_weights`, default 1). `confidence` = how much of the picture is
  backed by data. `ranking_score` = `confidence * overall + (1 - confidence) * baseline` (baseline defaults to 50, configurable per rule set). The minimum score applies to the
  ranking score. **Why:** in M2 a candidate with one known field could score 100 and tie with a fully documented 95. Now it ranks at about 62 (25% confidence) against 95 (100%).
  The trade-off is deliberate: thin profiles are pulled towards neutral, so completing a profile can only raise its confidence, never silently hide it.
- **Findings.** Strengths (met soft rules, heaviest first, top 5), concerns (unmet soft rules, top 5), possible deal-breakers, and unknowns. Every unknown names the **missing profile
  fields and whose they are** ("B's smoking", "A's age"), correctly attributed even for directional rules evaluated from the other side, so a matchmaker can ask for exactly that.

## In the app
- Commands: `mm_dimension_catalog`, `mm_score_pair` (scorecard + rule-by-rule evaluation), and `mm_find_matches` now ranks by the confidence-adjusted score (list rows show
  score, confidence, top strengths and concerns, and why excluded candidates were excluded). The real command code ran end to end against in-memory SQLite.
- UI: a **match view** (`/match`): score, confidence, hard-constraint verdict, dimension bars with status chips and "% known", strengths / concerns / possible deal-breakers /
  unknown information, and a collapsible rule-by-rule evaluation. Clicking a candidate in "Potential matches" opens it. The rule editor gained dimension pickers (rule dimension,
  dimension weights) and the neutral baseline; the starter rule set now uses dimension keys and gained religiosity, smoking and education rules.
- Rust JSON verified against the TypeScript types (every field the UI reads is present; aliases do not leak).

## Known limits
- Dimension score statuses use fixed thresholds (75 / 50). They are not yet configurable per rule set.
- Many rule sets from M2 use groups such as `age` or `location`; these resolve through aliases. Custom group names map to "Other" (the editor says so).
- The baseline pull assumes missing information is roughly neutral. It does not model *why* data is missing (for example a deliberately hidden field).
- No "AI assessment" yet (M6) and no psychological dimensions (M7): those panels are intentionally absent rather than placeholder text.
- No automated tests for the Tauri command wrappers; ESLint was not run (its config dependencies were unavailable in my sandbox).
