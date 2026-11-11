# M11 status: fast, deterministic profile extraction

"Read with AI" used to be one large model call: every field plus preferences, contradictions, missing
fields and questions in a single request, `max_tokens` 3072, a 600 s timeout, and a whole run that failed
if the JSON came back broken twice. M11 puts a local rule tier in front of it. The rules always run, always
finish, and always return something; the model is optional and is only asked about what the rules left empty.

## Principles
- **The rules never fail.** They are pure Rust in `matchmaking-core`: no I/O, no clock, no randomness. The
  same text gives the same suggestions, and a model that is missing, slow or wrong cannot remove them.
- **Every suggestion quotes the text.** Each rule suggestion's `evidence` is an exact substring of what the
  matchmaker typed. No rewriting, no paraphrasing, no invented span.
- **One reader per field.** A field the rules filled is never sent to the model, so the two tiers cannot
  disagree and nothing is paid for twice.
- **Advisory only.** Every result is still a pending suggestion that a person accepts or rejects. A rule
  entry lands as `Provenance::RuleExtracted`, whose `rank()` is 0, the same as `AiInferred`: it can never
  overwrite an entry a person made.
- **The gate is unchanged.** The rules run on the local text because nothing leaves the computer. The model
  tier keeps the redaction, the `SYSTEM_RULES` block and the sensitive-field gate exactly as they were;
  `check_gate`/`prepare_for` still guards every model call.

## Pieces
- `matchmaking-core/src/extract_rules.rs` (new):
  - Normalisation folds Persian and Arabic-Indic digits to ASCII, `ي`/`ك` to `ی`/`ک`, unifies ZWNJ variants,
    collapses whitespace and lower-cases, keeping one character per character so an index maps back to the
    original and evidence stays an exact substring.
  - Three matchers, each with a fixed confidence: labelled lines (`0.95`), patterns (`0.85`), keywords
    (`0.75`). Labelled lines work for *any* registry field, including custom ones, by field kind.
  - Default-field patterns in English and Persian: age 16-100, height 120-230 cm (also metres), weight
    35-250 kg, gender, marital status, children and children count (number words one-ten and یک-ده),
    education with longest-match-wins, employment, smoking, religiosity, dates of birth (Gregorian as typed,
    Jalali converted to ISO Gregorian), and city/province/birthplace.
  - Jalali dates of birth are converted by a built-in converter (`matchmaking-core/src/jalaali.rs`, no new
    dependency). `/`, `.` and `-` separate the parts, a two-digit year means `13YY`, the part over 31 is the
    year, and its position decides the order (`1368/1/1` is y/m/d, `1/1/1368` is d/m/y). A date whose parts
    are all 31 or less is skipped; month and day are checked against the month length, including a 30-day leap
    Esfand. A four-digit year of 1900 or later is Gregorian and keeps its `YYYY-MM-DD` value.
  - A partner-clause guard (looking for, seeking, wants a, دنبال, مایل به, همسری, می‌خواهد, ...) drops matches
    that describe the desired partner, so "looking for a woman" never sets the person's own gender. The
    clause starts after the nearest conjunction, so "41 years old and looking for a woman" keeps the age.
  - Two different values for one field make it ambiguous: no suggestion, field reported for the model tier.
  - Skips `NEVER_SHARED` and `Records` fields. It may read sensitive fields, because it runs locally.
  - `extract_with_rules(registry, text, targets, profile)` returns `RuleExtraction { suggestions, ambiguous }`;
    `FieldSuggestion` gained `source: SuggestionSource { Ai, Rule }`, defaulting to `Ai` on deserialisation.
- `matchmaking-core/src/ai.rs`:
  - `slim_extraction_prompt_for` lists only the target fields and asks for
    `{"fields":{KEY:{"value":V,"evidence":"exact words"}}}`; preferences, contradictions, missing fields and
    questions appear only with `with_preferences`.
  - `parse_slim_extraction` reuses the validation of `parse_extraction` and additionally drops any key that
    was not requested.
  - `output_constraint` builds a GBNF grammar and a JSON schema from one spec: choice fields limited to their
    option strings, numbers to a JSON number, booleans to `true`/`false`, keys limited to the targets, and
    evidence capped at 200 characters.
- `frontend/src-tauri/src/ai/backend.rs`: `complete_structured(task, system, user, constraint)`, defaulting to
  `complete_for`. The built-in sidecar gets the grammar, Ollama `format`, OpenAI-compatible
  `response_format: json_schema` with the existing fallback to `json_object` and then plain text, Anthropic
  the prompt only.
- `llama-helper/src/main.rs`: `Request::Generate` accepts `grammar: Option<String>` and, when set, starts the
  sampler chain with `LlamaSampler::grammar`. A grammar that fails to load returns an error response.
- `frontend/src-tauri/src/ai/models.rs`: `TaskProfile::SlimExtraction { fields, with_preferences }` uses
  `min(1024, 96 * fields + 128)` tokens (2048 with preferences) and a 180 s timeout.
- `frontend/src-tauri/src/ai/service.rs`: `extract_profile` resolves targets (explicit, else the default
  preset, else every eligible field), runs the rules, stores them with `source='rule'`, then — only if a
  session exists and fields remain — calls the model and stores those with `source='ai'`. It reports
  `llm_status` (`ok`, `skipped_nothing_left`, `unavailable`, `failed`), `llm_error`, `rule_count`, `ai_count`
  and `timings { rules_ms, llm_ms }`. A failed gate or a failed call keeps the rule results.
- `frontend/src-tauri/src/ai/commands.rs`, `intake_commands.rs`: `fields` and `with_preferences` on
  `ai_extract_profile` and `ai_preview_extraction` (the preview shows the slim prompt that would really be
  sent, or says nothing will be); `preset_id` on the intake run. Bulk intake now runs the rules for every
  entry even when the gate is closed.
- Migration `20261110000000_add_extraction_presets.sql`: `mm_ai_suggestions.source TEXT NOT NULL DEFAULT 'ai'`
  (existing rows keep reading as AI suggestions) and `mm_extraction_presets`. Saving a preset as default
  clears the flag on the others; unknown keys are filtered on read. Three commands, audited by name and field
  count only.
- UI: preset dropdown, per-run field checklist, the preferences checkbox seeded from the preset, a `rules` or
  `AI` badge per suggestion, a status line ("9 found by rules in 4 ms · AI read 3 fields in 8 s", or
  "AI unavailable: ..."). The button stays enabled with AI off and reads "Read"/"خواندن". A presets editor
  lives in the shared AI settings. English and Persian strings for all of it.

## Verified here
- `cargo test -p matchmaking-core`: 140 passed across all suites, including the 18 in
  `tests/extract_rules.rs`.
- `cargo test -p matchmaking-core --release --test extract_rules`: 18 passed. The 20k-character timing test
  is in this suite; measured on this machine it reads a 20 020-character entry in about 5 ms, and a dense
  Persian entry of the same length in about 10 ms, against the 20 ms budget.
- Golden corpus: 30 fixtures in `matchmaking-core/tests/fixtures/extraction`, precision 1.000, recall 0.864.
  The test fails below 0.98 precision.
- `cargo test -p llama-helper`: 4 passed, including the structural check of the embedded grammar (single
  `root` first, unique names, every referenced symbol defined, evidence capped at 200, an untargeted field
  absent).
- `cargo test --lib ai::` outside the sandbox: **51 passed**. In the sandbox 48 pass and the 3 loopback HTTP
  tests fail with `PermissionDenied`, as before this work.
- `cargo check --lib --tests` in `frontend/src-tauri`: clean.
- `pnpm tsc --noEmit` in `frontend/`: clean.
- Covered by tests: each matcher in English and Persian including Persian digits and number words; the
  preference guard; longest-match education; conflict to ambiguous; range rejection; labelled lines for a
  custom field of each kind; evidence is always an exact substring; determinism; the slim prompt listing only
  targets and never a field the rules filled; the parser dropping unrequested keys and fake evidence; a
  provider error leaving rule suggestions in place with `llm_status=failed`; a closed gate meaning rules only,
  no model call; a rule suggestion accepted as `RuleExtracted`; preset CRUD with a single default and unknown
  keys filtered; an intake batch with AI off still producing rule suggestions; a preview with the gate closed
  explaining itself instead of showing a prompt.

## Not verified
- **No real model was run.** Every model-side test uses a scripted stand-in. The manual comparison the plan
  asks for — built-in Qwen 3.5 4B on 10 real notes, old versus new wall time and prompt/output characters from
  `mm_ai_log` — has not been done. So the 96-tokens-per-field budget, the 180 s timeout and whether a real
  model obeys the grammar are all unmeasured.
- How much a real grammar slows generation in the sidecar.
- The AI panel and the presets editor were type-checked, not opened. The dropdown, the checklist, the badges
  and the status line have not been exercised in a running app.
- The 3 loopback HTTP tests, which need a normal machine (they pass outside this sandbox).
- The rule corpus is 34 anonymised samples. Recall is 0.925 and precision is 1.000: the rules still leave
  free-form occupation or lifestyle text to the model tier, but Jalali dates of birth are now read locally.

## Differences from the plan
- The rule tier was made faster than the first draft without changing behaviour: the searched string is
  joined once per run instead of per scan, the city/province and number-word sweeps are skipped unless their
  trigger literal is present, and the clause walk compares characters instead of allocating a string per step.
  The budget is not weakened; the test still asserts 20 ms in release.
- The grammar is produced in Rust by `output_constraint` and sent to the sidecar per request; nothing is
  hardcoded there. But llama.cpp only parses a grammar once a model is loaded, and `llama-cpp-2 =0.1.146`
  exports no parse entry point, so the sidecar test cannot run a real parse. It instead checks a copy of the
  generated grammar, byte for byte, against the structural invariants a parse would reject.

## Checklist for a person with a real machine
1. Settings, AI: download Qwen 3.5 4B, save, and confirm the badge reads Ready.
2. Open a profile with notes, press Read with AI off. Suggestions should appear in milliseconds with a
   `rules` badge and the status line should name only the rule time.
3. Turn AI on and press Read again. Only the fields the rules missed should reach the model; the status line
   should show both times and both counts.
4. Run the same notes through the old build and the new one and compare `mm_ai_log`: wall time, prompt
   characters, output characters.
5. Turn the preferences checkbox on and confirm the run is slower and preferences appear as suggestions.
6. Save a preset as default, reopen the panel, and confirm it is preselected and its field list is applied.
7. Stop the sidecar mid-run and confirm the rule suggestions are still returned and the status line says the
   model failed.
8. Run `cargo test --lib ai::` outside the sandbox; expect no failures.

## Tests
`matchmaking-core/tests/extract_rules.rs`, `matchmaking-core/tests/fixtures/extraction/` (30 samples),
`frontend/src-tauri/src/ai/tests.rs`, `llama-helper/src/main.rs` (grammar invariants).
