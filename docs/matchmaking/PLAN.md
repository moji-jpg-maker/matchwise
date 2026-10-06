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
- **M3 Scoring & match view** -- multi-dimension scores, strengths/concerns/unknowns, explanations.
- **M4 Workflow** -- match lifecycle, matchmaker approve/reject/override, notes, audit log.
- **M5 Telegram** -- adapter, account linking, notifications.
- **M6 AI** -- LLM profile extraction (never overwrites human data), pair analysis grounded in rule results.
- **M7 Psychology, outcomes, ranking v1** -- evidence registry + questionnaires, outcome capture, versioned scoring configs.

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

## Phases 2-5
Unchanged in intent: better psychology/AI reasoning -> outcome-based learning -> agentic assistant with authorization policy -> web/mobile/API ecosystem.
