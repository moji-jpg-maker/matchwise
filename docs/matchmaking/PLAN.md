# Matchwise -- MVP & Phased Roadmap (revised)

Revision of the original plan after auditing the fork. Original feature scope is unchanged; **changes are marked
`[CHANGED]` or `[NEW]`**. See `M0_AUDIT.md` for evidence.

## 0. Foundation
Fork of Meetily (Tauri 2 + Next.js 14 + Rust core, sqlx/SQLite). Paths: Rust in `frontend/src-tauri/`, UI in `frontend/src/`.

**[NEW] Engine lives in its own crate, `matchmaking-core/`** (pure Rust, no Tauri/SQLite/Telegram/LLM dependency).
Tauri commands, the Telegram adapter, and any future server call into it. This keeps the engine testable on any machine and
enforces "the matching engine must not depend on Telegram".

Target module layout:
```
matchmaking-core/        field registry, profiles+provenance, expression/rule engine, scoring, ranking traits
frontend/src-tauri/src/
  mm/                    Tauri commands + sqlx repositories wrapping matchmaking-core
  telegram/              adapter (long polling), behind a trait
  ai/                    reuse of existing llm_client + extraction/pair-analysis prompts
frontend/src/            matchmaker dashboard (Next.js)
```

## Milestones (Phase 1 / MVP, built as vertical slices) [NEW]
Same feature set as before, ordered so something works end to end early.
- **M0 Cleanup & safety** -- remove telemetry, updater, license code, legacy backend, dead code, rebrand, keychain for secrets, encrypted DB. (`M0_AUDIT.md`)
- **M1 Profiles & search** -- field registry (custom fields at runtime), profiles with provenance, deterministic filters, import/export. *(first slice written: see `M1_STATUS.md`)*
- **M2 Rules** -- data-driven rule engine, hard constraints, versioned rule sets, admin UI, evaluation traces. *(first slice written: see `M2_STATUS.md`)*
- **M3 Scoring & match view** -- multi-dimension scores, strengths/concerns/unknowns, explanations. *(first slice written: see `M3_STATUS.md`)*
- **M4 Workflow** -- match lifecycle, matchmaker approve/reject/override, notes, audit log. *(first slice written: see `M4_STATUS.md`)*
- **M5 Telegram** -- adapter, account linking, notifications. *(first slice written: see `M5_STATUS.md`)*
- **M6 AI** -- LLM profile extraction (never overwrites human data), pair analysis grounded in rule results. *(first slice written: see `M6_STATUS.md`)*
- **M7 Psychology, outcomes, ranking v1** -- evidence registry + questionnaires, outcome capture, versioned scoring configs. *(M7a, questionnaires and the evidence registry, written: see `M7A_STATUS.md`; M7b step 1, the outcome export, written: see `M7B_STATUS.md`; M7b step 2, versioned scoring configurations, exposure tracking and the one gate, written: see `M7B2_STATUS.md`; ranking v1, the comparison against the deterministic baseline, written: see `M7B3_STATUS.md`. The comparison reports an ordering and still changes nothing; a learned ranking that would consume it is not started.)*
- **M8 Bulk profile intake** `[NEW]` -- take one target profile and X corpus profiles as raw text in a single
  operation, create the profile records, extract fields through the M6 path, and hand the corpus to the M3/M7 ranking
  path. *(written: see `M8_STATUS.md`. Spec below.)*
- **M9 Windows distribution** `[NEW]` -- a production Windows installer: sign and bundle the sidecar binaries per
  target, complete the Windows signing step that the inherited config already points at, and ship NSIS plus MSI
  verified on a clean machine. *(config and workflows written: see `M9_STATUS.md`. Nothing has run on a Windows
  runner yet, so the installer itself is still unbuilt. Spec below.)*

## Gaps against the end-to-end flow

The tool is meant to take one target profile plus a corpus of raw-text profiles and return a ranked list of matches.
Against that flow:

| Step | State | Where |
|---|---|---|
| Ingest 1 target + X corpus profiles as raw text | **missing** | M8 |
| Parse and extract fields with AI, persist them | partial: one profile at a time, each suggestion accepted by hand | M6, `M6_STATUS.md` |
| Compare the target against the corpus | written | `mm_find_matches`, `mm/rules_commands.rs`; M3 |
| Ranked list sorted by score | written | `find_matches` ordering; M7b step 3, `M7B3_STATUS.md` |

The engine half is done. What is missing is the way in: no command or screen takes a block of raw text and turns it
into a profile, and nothing fans a corpus out across many profiles. The extraction that would do the parsing exists
and is reachable only from an already-created profile's own notes field.

## M8 in detail (written)

Scope: one operation that accepts a target profile and a corpus, so a matchmaker can paste or load what they already
have instead of typing each person in.

- **Input.** One target as raw text, plus X corpus entries as raw text (paste, or a text/CSV/JSON file per entry).
  Each entry becomes a profile with the raw text kept as an intake note, so the source is never lost.
- **Extraction.** Reuse `ai::service::extract_profile`'s pipeline per entry rather than a second parse path. Batch
  runs still produce *suggestions*; the batch does not bypass the review the M6 rules require, and a batch accept is
  an explicit choice the matchmaker makes, recorded in the audit log the same way a single accept is.
- **Gating.** The AI gate, redaction and consent rules from M6 apply per entry. A batch with AI off still creates the
  profiles and keeps the raw text; nothing is parsed.
- **Comparing and ranking.** After intake, the target runs through the existing `mm_find_matches` against the corpus.
  No new scoring or ranking code.
- **Failure handling.** One bad entry (empty text, an unreadable model answer) must not lose the other entries; the
  batch reports per-entry what was created, what was parsed and what was dropped, and why.
- **Tests.** Core: any parsing or chunking helper added to `matchmaking-core` gets unit tests. App: a batch command
  test against a real pool, plus golden-file tests on synthetic profiles only.

Open questions for this milestone: whether a corpus entry is one raw-text block or a file, and whether a batch accept
is offered at all or every suggestion stays individual.

## M9 in detail (config written, unbuilt)

Scope: an installer a stranger can run on a clean Windows machine.

- **Toolchain.** Keep the inherited Tauri 2 bundler rather than adding a second packager; it already names the NSIS
  and MSI targets (`frontend/src-tauri/tauri.conf.json`), and `tauri.windows.conf.json` carries the Windows resource
  overrides. Per-user versus per-machine install, and the optional auto-launch, are Tauri/NSIS settings.
- **Dependency bundling.** Ship the Windows builds of every `externalBin` sidecar. A Windows checkout has nothing in
  `frontend/src-tauri/binaries/` (gitignored; only Linux artifacts sit there on a Linux box), but the Windows job is
  already fed: `build.yml`/`build-windows.yml`/`build-devtest.yml` build `llama-helper` and copy it to
  `binaries/llama-helper-x86_64-pc-windows-msvc.exe`, `build/ffmpeg.rs` downloads the Windows FFmpeg at build time,
  and `build/onnxruntime.rs` stages `binaries/onnxruntime` (which `tauri.windows.conf.json` maps into the bundle).
  The open decision is what ships inside the installer (local model runtime, ONNX runtime, ffmpeg) against what is
  downloaded on first run -- model weights are downloaded today, not bundled -- and how the encrypted database and its
  key in the credential store survive install, upgrade and uninstall.
- **Signing.** `bundle.windows.signCommand` calls `scripts/sign-windows.ps1`, which **does exist**, at
  `frontend/src-tauri/scripts/sign-windows.ps1` (the path in `signCommand` is relative to `frontend/src-tauri/`, not
  `frontend/` -- the earlier "does not exist" reading was wrong). It skips with exit 0 when `DIGICERT_KEYPAIR_ALIAS`
  is unset, otherwise runs `smctl sign` and re-verifies with `Get-AuthenticodeSignature`. What is still missing is the
  certificate itself, so the installer ships unsigned and that is what a SmartScreen or Defender warning comes from.
  Decide the certificate (OV versus EV) and whether to submit the build for reputation.
- **QA.** A manual checklist run on a clean Windows VM: install, shortcut and Start-menu entry, first-run database
  creation, the AI gate with no model present, intake, a ranked run, upgrade over an installed copy, uninstall
  leaving no key behind. Records what a fresh machine asks the user for.

## Feature areas (unchanged unless marked)
1. **Profiles**: schema-driven fields, children as sub-records, questionnaires, partner preferences, contact, photos.
   **[CHANGED]** profile = JSON column + versioned field registry (+ indexed columns for hot filters), not EAV.
   Each field has a `sensitive` flag driving visibility, encryption, and redaction before any cloud-LLM call.
2. **Search/filter**: same expression tree as rules; reusable by UI and Telegram.
3. **Rule engine**: stored as data. **[CHANGED]** typed JSON expression tree evaluated in Rust (AND/OR/NOT, comparisons, ranges,
   offsets such as "partner age <= my age + 8", conditionals), with **three-valued logic**: missing data is `Unknown`, never a silent pass/fail.
   Hard rules exclude; Unknown hard rules are reported as "needs info". Weighted soft rules produce the score. Rule sets are versioned and each recommendation records the version.
4. **Scoring**: multi-dimension output (values, personality, lifestyle, family, children, religion, location, preferences...) plus strengths / concerns / unknowns / possible deal-breakers.
5. **Psychology + evidence registry**: as before. **[CHANGED]** present results as indicators with evidence level, not a headline percentage:
   research on relationship outcomes finds pre-relationship self-reports predict quality only weakly, and pair-specific compatibility less so.
   Prioritise stated constraints and matchmaker judgment. No clinical claims.
6. **AI profile understanding**: extraction -> validation -> stored with `source = ai_inferred`. Provenance ranking (AI < questionnaire < user < matchmaker) is enforced in `matchmaking-core`.
7. **AI pair analysis**: constrained to structured data + rule results.
8. **Ranking**: stages as before. **[CHANGED]** learned ranking stays disabled until minimum outcome counts are met; log *exposure* (who was shown), not only choices, to limit selection bias; always compare to the deterministic baseline.
9. **Photos**: quality/duplicate/format checks only. No inference of traits from faces. Photo store encrypted.
10. **Dashboard, 11. Lifecycle, 15. Feedback loop**: as before.
12. **Telegram** **[CHANGED]**: a desktop app cannot receive webhooks. Phase 1 = long polling from the Rust core (works while the app runs).
    A relay service is a Phase 5 item. Document that anything sent via Telegram passes through Telegram.
13. **Notifications**: Telegram first.
14. **Multi-user** **[CHANGED]**: desktop = administrators/matchmakers/reviewers. Candidates are Telegram-linked identities only. Keep `org_id` on every table; real multi-tenant sync is deferred to a server phase.
16. **Privacy & security** **[CHANGED]**: encrypted DB (SQLCipher or equivalent) from M0; secrets in the OS keychain; default to local LLMs;
    cloud-LLM use requires per-organization consent with identifier redaction; every external send is audit-logged; no telemetry.
17. **[NEW] Internationalization**: RTL layout, locale-aware dates (including Jalali if needed), and non-English LLM prompts from the start, not Phase 5.
18. **[NEW] Testing**: synthetic profiles and golden-file tests only; never commit real profiles.

## Phases 2-5 (deferred, out of scope)
The original phases 2-5 -- better psychology/AI reasoning, outcome-based learning, an agentic assistant with an
authorization policy, and the web/mobile/API ecosystem -- are **out of scope for now** and are not planned for the
near future. Phase 1 plus M8 (bulk intake) and M9 (Windows distribution) is the whole of the current commitment.
The text below is kept for record only and describes nothing that is being built.

Original intent: better psychology/AI reasoning -> outcome-based learning -> agentic assistant with authorization policy -> web/mobile/API ecosystem.
