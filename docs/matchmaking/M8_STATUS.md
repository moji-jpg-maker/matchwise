# M8 status: bulk profile intake

One screen and three commands turn a pasted (or loaded) target plus a corpus of raw-text entries into real profiles,
with the raw text kept, then reuse the existing AI extraction (M6) and `mm_find_matches` (M3). There is no second
parse path, no new scoring, no new crate. A bulk run still yields only *suggestions*; accepting a batch is one explicit,
audited action. Everything is per entry and failure-isolated: one bad entry cannot lose the others.

## What was built
- **Storage.** Migration `20261013000000_add_matchmaking_intake_notes.sql` adds `mm_intake_notes` (`profile_id` primary
  key, `org_id`, `batch_id`, `note_text`, `label`, `created_at`) and an index on `(org_id, batch_id)`. The raw text is
  never a profile field, so it is never scored, never exported as field data and carries no provenance.
  `mm/intake_repo.rs` (`IntakeRepo`) writes a profile and its note in **one transaction**, reads a note by profile or
  by batch, and deletes a note. `MmRepository::delete_profile` deletes the note too; `insert_profile` became generic
  over the executor so it can run inside that transaction.
- **Core helper.** `matchmaking-core/src/intake.rs`, the only parser on this path. `split_corpus` splits on a line of
  three or more dashes, falling back to blank lines when the text has no separator line; it normalizes CRLF, trims,
  and drops empty entries (reported, index preserved). `parse_entries` does the same for strings the caller already
  split. `parse_json_corpus` reads a `.json` array of strings. Entries are capped at `MAX_ENTRY_CHARS` (20 000
  *characters*, not bytes) and flagged `truncated`. A leading `#` heading is the label, otherwise `Entry N`.
- **Service hooks.** `ai/service.rs` gains a private `profile_extra`: the caller's extra text if there is any,
  otherwise the stored note. `preview_extraction` and `extract_profile` use it, so the batch and the profile page's
  "Read profile" button share one path. The note reaches the model only as `extra`, so the existing
  `include_sensitive` rule redacts it for cloud and keeps it verbatim locally. `core ai.rs` and the prompts are
  unchanged.
- **Commands** (`ai/intake_commands.rs`, registered in `lib.rs`, reusing `prepare()` which became `pub(crate)`):
  - `ai_intake_batch(target, target_text, entries, batch_id)`: reads the gate once, creates a profile and note for
    every entry, then extracts per entry through `service::extract_profile`, one at a time.
  - `ai_intake_report(batch_id)`: per-entry outcome rebuilt from storage.
  - `ai_intake_accept_all(batch_id)`: runs each pending suggestion through `service::decide_suggestion(Accept)`.
  - `mm_intake_note(id)`: the stored note for the profile page. Not in the original plan; added so the page can show
    it read-only.
  - Each body is a pool-taking function (`run_batch`, `run_batch_with`, `run_report`, `run_accept_all`) because a
    `tauri::State` cannot be built in a test. `run_batch_with` takes the session as a parameter so tests can supply a
    scripted model.
- **UI.** `/intake` (`app/intake/page.tsx`): target box, corpus box, `.txt`/`.json` load button, run button with a
  per-entry report, "Accept all suggestions", "Show ranked list". Sidebar entry "Bulk intake" in both the collapsed and
  expanded nav. The profile page shows the note read-only when one exists. `types/matchmaking.ts` has
  `IntakeEntryReport`, `IntakeBatchReport`, `IntakeAcceptReport`; none carries note text.

## Decisions made along the way
- **The target is its own profile, not a corpus entry.** The plan has a separate target box, so `ai_intake_batch`
  takes a `target` string, creates it through the same `create_one` path with its own note, and returns
  `target_profile_id`. It is not counted in `created`, and it is not extracted in the batch (the ranked run starts from
  its profile page, where "Read profile" is available).
- **A closed gate is a property of the whole run.** The gate is read once. If closed, every profile and note is still
  created, no model is contacted, nothing is marked `parsed`, and the reason is reported once in `note`.
- **Failure isolation per entry.** An empty entry is dropped and reported. An oversized entry is capped, not lost. A
  create failure or an unreadable model answer is recorded against that entry (the profile and its text are kept when
  only the parse failed) and the run continues.
- **Extraction is sequential.** `service::ask` already serializes requests; the milestone says one at a time, so there
  is no concurrency and no progress events.
- **A leading `Name: ...` line is not a label.** Only a `#` heading names an entry, so a sentence from the text is
  never promoted into a label the report would echo.
- **Audit holds names and counts only.** `intake_entry_created`, `intake_target_created` and `intake_batch` record
  labels or counts; accepted fields are audited by `decide_suggestion` as field names. Note text and the note lookup
  never appear in `mm_audit_log`.
- **Intake profiles start empty.** A profile created from an entry has no fields; the entry is the source, not the
  answer. Nothing is copied from the text into a field except through an accepted suggestion.
- **Foreign keys are off in this app**, so the note's `ON DELETE CASCADE` does not fire. `delete_profile` deletes the
  note explicitly, and a test covers it.
- **Plain strings cross the command boundary.** The frontend splits pastes and reads files; a `.json` array goes in as
  `entries`, a paste as `target_text`, so a JSON entry is never split a second time.

## Differences from the plan
- **"Show ranked list"** opens the target's profile page, where the existing `MatchesPanel` runs `mm_find_matches`
  (a deliberate button press, because it writes exposure rows). It is not a new ranked view and not an auto-run.
- **File load** uses the browser file input (`<input type="file">`), not `tauri-plugin-dialog`: the dialog's JS package
  is not installed and no dependency was to be added. The Rust plugin is initialised but unused here. Contents still
  cross the command as plain strings.
- **No TS contract test for the new types.** The repo has no existing test that checks the matchmaking TS types against
  Rust JSON, so none was added; `tsc --noEmit` is the only check on them.
- **Re-running batch accept is not a pure no-op.** Only accepted suggestions leave the pending list. A suggestion
  refused because a person owns a different value stays pending, so a second run reports it skipped again.
- **Report from storage is thinner than the live report.** `ai_intake_report` rebuilds from notes, so it lists entries
  as `created` with zero suggestions; dropped entries, reasons and parse counts exist only in the live result.
- **Plan field names.** The plan named `ai_intake_batch`'s inputs as target text, entries and `batch_id`. The command
takes `target` (the target person's own text, which becomes its own profile), `target_text` (the raw corpus, when the
  frontend has not split it), `entries` (the pre-split corpus, e.g. from a `.json` array) and `batch_id`. All but
  `batch_id` are optional.

## Tests
- **Core** (`matchmaking-core/src/intake.rs`, 14 tests): separator and fallback splitting, CRLF, blank entries, exact
  20 000-character boundary, JSON array and malformed JSON, labels, drop reasons, and a guard that raw intake text is
  not a profile field and reaches a model only through the redacted `extra` path.
- **Storage** (`mm/intake_tests.rs`, 7 tests): atomic insert, failed insert leaves no note, batch reads, delete removes
  the note.
- **App** (`ai/tests.rs`, section "bulk intake", 8 tests): gate-closed batch makes zero model calls; scripted batch
  gives the expected suggestions; a person-owned value is refused by both single and batch accept; an unusable entry
  does not stop the others; audit content; the report from storage; the per-profile path reading the stored note, with
  the cloud preview scrubbing a phone number.
- **Golden** (`ai/tests.rs` `mod golden`, 4 tests) over synthetic fixtures in `src-tauri/tests/golden/intake/`:
  gate closed, gate open, a JSON corpus mixing good and blank entries, and a separate target.
- Full `matchwise` lib suite: 358 passed. 17 tests that bind loopback sockets fail inside the sandbox and pass outside.

## Things to know when extending
- **Grounding drops suggestions whose evidence is not in the text read.** Test entries must contain the exact phrases
  the scripted answer cites, or suggestions vanish silently.
- **A name is only scrubbed when the profile has a known `full_name`.** An empty intake profile has none, so cloud
  redaction there is pattern-based (phone, e-mail, URL). Local mode keeps the text verbatim by design.
- **CSV is deferred.** The boundary is the helper in `intake.rs`; adding it should not touch the commands.
- **Not exercised:** the `/intake` page and the profile note are type-checked but not clicked through in a running
  build, and a real model has not been used; the scripted `Mock` stands in for it.
- **Not done on purpose:** no auto-run of the ranking, no background job, no learned ranking, no new crates.
