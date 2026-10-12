# M7b status: outcome data export (first slice)

Step two of the psychology and learning milestone (M7). This slice is the **outcome dataset** described in M7b: the
recorded decisions turned into a file that a ranking can later be checked against. The other two parts of M7b --
**versioned scoring configurations** and **ranking v1 compared with the deterministic baseline** -- come next and are
not in this slice.

## What it does
- **One row per tracked pair.** The export is a snapshot of *how decisions were actually made*: the pair, the lifecycle
  status, what each person said (`a_response`, `b_response`), the outcome if the match finished, and whether advancing
  it needed a written override (recorded as a bare `true`/`false`).
- **Each row carries the scorecard that was in force**, not one recomputed now: the rule-set id and version, the
  snapshot's trigger (`created`, `rescore`, `weights_adjusted`), when it was taken, `overall`, `confidence`,
  `ranking_score`, and the snapshot's own `eligible` / `meets_threshold` verdicts. The newest snapshot per match is
  joined in, so a rescore is reflected and the earlier ones stay in the database.
- **A score only means what it meant under the rules in force at the time**, which is why the rule-set id and version
  travel with every row and the page warns when the history spans more than one version.
- **Scored dimensions become their own columns** (`dim_<key>`), the union of every dimension scored anywhere. **A blank
  is not a zero**: a dimension that had no score for that pair stays empty, because a missing score is absence of data
  and writing `0` would invent a number. `unknown` dimensions are therefore never exported as anything.
- **No profile content leaves the database.** The row identifies the pair by id and never carries a name, a phone, an
  email, a Telegram handle, a note, or the override *reason* text. Everything the export needs lives in the match record
  itself; there is a test asserting each of those strings is absent from the produced CSV.
- **The export is audited as an action, not as content.** Reading the data is itself a use of it, so
  `matchmaking_export` lands in the audit log with row and outcome **counts only** -- never the rows.
- **Reproducible text.** CSV serialisation is deterministic (fixed column order, fixed 4-decimal number formatting with
  no exponent and no locale), so the same decisions always serialise to the same bytes. It lives in the pure
  `matchmaking-core` crate, so it is testable without a database.
- **Counts, for the gate.** The page shows tracked pairs, finished, how many carry a stored scorecard, how many were
  advanced by override, an outcome breakdown, what is still in progress, and the version span of the history.

## Where it lives
- `matchmaking-core/src/export.rs` -- `ExportRow`, the column list, RFC 4180 escaping, stable number formatting, and
  `to_csv`. 6 tests.
- `frontend/src-tauri/src/mm/export_repo.rs` -- read-only queries: every match with its newest snapshot, and the union
  of dimension keys.
- `frontend/src-tauri/src/mm/export_commands.rs` -- `mm_export_outcomes` and `mm_outcome_dataset_summary`, plus the
  audit entry.
- `frontend/src/app/export/page.tsx` -- the **Decision data** page, reached from the sidebar ("Decision data", in both
  the expanded list and the collapsed rail). Build CSV, then save or copy it; the columns and the first 20 lines are
  shown before anything is written.

## Verified here
6 core tests (header composition, RFC 4180 escaping including embedded quotes and CR/LF, stable number formatting,
one field per column when data is missing, header-plus-rows in the given order, and that the export carries no profile
content) and 7 app-layer tests (one row per match with the stored scorecard columns, no names/phones/notes, a finished
and an overridden match, an in-progress match counted by status and *not* as an outcome, an empty dataset as just the
header, a dimension scored in one match but not another leaving a blank rather than a zero, and the audit entry being
written without content). 76 core tests pass. The frontend type-checks (`tsc --noEmit`, clean) and the Rust JSON
matches the TypeScript types.

## Not verified / limits
- **The 100-match learning gate is my own placeholder.** The plan says ranking should wait until there is enough
  outcome data, but does not name a number. `LEARNING_MINIMUM = 100` in the export page is the value I chose for the
  copy; it gates nothing yet. Tell me the threshold you want before ranking v1 uses it.
- **Blank-versus-zero is a convention the reader must know.** The CSV says "no score" by being empty, which is correct
  but easy to misread in a spreadsheet that treats an empty cell as `0`. The page and the column list say so.
- **`profile_a`/`profile_b` ids are exported** so a row can be joined back to the profiles. They are identifiers, not
  content, but the file is not anonymous on its own if someone holds the profile table -- treat it as personal data.
- **Finished == has an outcome.** A match closed without a recorded outcome is not invented into either bucket; it is
  counted by its current status instead. If the lifecycle ever allows closing without an outcome, that hole will show up
  as an in-progress count rather than as a silent zero.
- Not clicked through as a finished UI: the page was type-checked and the commands exercised in tests, not driven in a
  packaged build. No test of the React page itself.
- No import path back: the CSV is a read of the decisions, not a round-trippable format.
