# Persian (fa) support: handoff

Repo: /home/mojtaba/deep-matcher (Matchwise: Tauri 2 + Next.js `frontend/`, Rust core `matchmaking-core/`, Tauri `frontend/src-tauri/`).

## Working constraints
- Sandbox: no network, `.git` read-only, cannot bind local ports. No commits or branches.
- Tools: use the exec tool (`Bash`/`Read`/`Write` unsupported). Edit via python3 heredocs with plain string replace plus assert. Keep calls under ~10KB.
- Style: no inline comments, no one-letter variable names. Rust `///` docs are fine.
- Checks:
  - `matchmaking-core`: `cargo test --offline`
  - `frontend/src-tauri`: `cargo check --offline --lib`, `cargo test --offline --lib telegram` (FFmpeg stub at `binaries/ffmpeg-x86_64-unknown-linux-gnu`, gitignored)
  - `frontend`: `npm run check:i18n`, `npx tsc --noEmit`, `npx next build` (needs escalation)
- User replies "yes/ok/continue"; pick reasonable defaults.

## Done and verified
- AI (step 2): consent text, provider labels, gate and readiness messages, model notes, and service errors take a `locale` (optional `locale` argument on the `ai_*` commands; frontend passes it). `extraction_prompt_for` and `pair_prompt_for` ask for Persian free text and keep keys, enums and evidence unchanged. `Session` carries `locale`. Bulk intake (`intake_commands.rs`) and `backend.rs` `AiError` text stay English (logged, never matched). The stale-analysis hash always uses the English prompt so changing language does not mark analyses stale.
- Frontend: fonts (Vazirmatn via next/font/local), locale switcher, RTL, `useI18n()` returning `{locale, dir, setLocale, t, fmt}`. Intl tag for fa is `fa-IR-u-ca-persian-nu-arabext`. Catalog has 1682 keys in sync.
- Telegram (`engine.rs`, `notify.rs`, `commands.rs`) fully localized, with 4 new Persian tests.
- Core validation: `FieldKind::validate_for`, `FieldRegistry::validate_value_for`, `validate_preference_for`, and now `validate_ruleset_for(set, reg, locale)` (rule_validation.rs). English functions are wrappers.
- Tauri `mm_validate_rule_set` / `mm_save_rule_set` take an optional `locale`. `frontend/src/app/rule-set/page.tsx` passes it.
- Test `validation_messages_follow_the_locale` in `matchmaking-core/tests/ruleset.rs`.
- Last run: core tests pass, `cargo check` ok, `tsc` clean, `check:i18n` ok. One telegram test (`http_api_builds_requests_and_maps_errors`) fails only because the sandbox blocks binding a port.
- Step 3 (smaller Rust):
  - `q_save_instrument` duplicate keys were already Persian in `validate_instrument_for`.
  - Summary template names and descriptions are translated on the frontend (`templateInfo.<id>` catalog keys, applied in `useTemplates.ts`). Custom ids fall back to server text. Template prompts stay English.
  - `ui_locale.rs` holds the UI locale; the `set_ui_locale` command is called by `I18nProvider` on locale change and rebuilds the tray menu. `tray.rs` `build_menu` uses it.
  - Notification constructors have `_for(locale)` variants (`notifications/types.rs`); the manager and test notification use `ui_locale::get()`. Tests added.
- Step 4: Persian option list in `FieldKind::validate_for` joins with «،» (the rule validator already did).
- Step 5: `scripts/check-i18n.js` flags values split over several lines and lists likely hardcoded JSX text, `placeholder`, `title`, `aria-label` and `alt` strings (warning; `--strict` fails). Brand and provider names are allowed, and commented-out code and `*Test.tsx` are skipped. Found and fixed `lib/recordingNotification.tsx` (`recordNotice.*`, 4 keys; the toast reads `currentLocale()`). Catalog: 1700 keys.
- Step 1 (core labels), complete: (partly done: evidence labels, dimension catalog/scorecard, starter-rule and note-less preference finding descriptions, `locale` on match/questionnaire commands). Also done: transition refusals and text-length errors (`validate_transition_for`, `validate_text_for`, `MatchStatus::label_for`), all English `Err(..)` strings in `match_commands.rs` (`set_response_inner` now takes a locale; the Telegram engine passes the recipient's), `q_assign` errors and its Telegram text (`render::questionnaire_assigned_text`, sent in the link's language; test `refusals_follow_the_locale`). Also done: `mm/commands.rs` (all commands take optional `locale`; frontend callers pass it), `scorecard_trigger` (translated in `MatchWorkflowPanel` via `matchPanel.scoreRescore/scoreWeights`), questionnaire errors (`validate_instrument_for`, `validate_answer_for`, `service::record_answer_for`, `instrument_for_locale`; Telegram passes the recipient's locale; test `answer_and_instrument_errors_follow_the_locale`). Also done: built-in instrument text via `Instrument::localized(locale)` (Mini-IPIP name, description, items, scale labels, subscales, evidence limitations/interpretation; applied in `q_list_instruments`, `instrument_for_locale`, response views and the Telegram assignment text; stored definition stays English; test `the_shipped_instrument_reads_in_persian_without_changing_its_structure`). Outcome labels already exist in the frontend catalog (`outcome.*`), no Rust change needed. Core labels are complete; the evidence citations stay as published.
- Step 6: `frontend/tests/lib/i18n-core.test.ts` (13 tests: `interpolate`, `normalizeSearch`, formatters, `lookup`/`translate`/`directionOf`, template catalog). Component rendering tests were not added because the existing hook tests need heavy setup.
- Step 7: README (Languages), CONTRIBUTING, CLAUDE.md (Internationalization section), `docs/i18n/glossary-fa.md` (draft), and a pending-translation note in `PRIVACY_POLICY.md`.

## Remaining plan
- Launch the app once in a real desktop session and click through both languages (not possible in the sandbox).
- Native-speaker and legal review of all Persian copy (see the flags below).

## Decisions to preserve
- Stored values stay English: default meeting title, `New Meeting` fallback, `'+ New Call'`.
- Rust error text matched by the frontend stays English: `No recording in progress`, `Recording start timed out`, `Download already in progress`, `isOllamaNotInstalledError`.
- Leave as is: commented-out UI in `AISummary/index.tsx` and `DeviceSelection`, dead `getModelPerformanceBadge`, console-only strings.
- Frontend conventions: `<bdi>` around names/user text; `dir="ltr"` on numbers, ids, tokens, links; `dir="auto"` on free-text inputs and Rust-sourced text; separate singular/plural keys; state holding a message stores a `MessageKey`; `tRef` in empty-dependency effects; catalog values on one line, unique group names.
- Telegram: recipient language from `mm_telegram_links.language` (init from `from.language_code`); outbox stores text already rendered in the recipient's language; matchmaker UI shows `render::plain`; callback data ASCII; user text via `render::user_text(loc, s)` (escape + bidi isolates for fa).
- Matchmaker-written rule descriptions and preference notes stay as typed; only starter rules (`default_rule_description_fa`) and note-less preferences are re-described per locale.
- Core pattern: `_for(.., locale)` is the real implementation, the original is an English wrapper. Persian inline via `match locale` or `locale.pick(en, fa)`. Digits via `localize_digits`. Persian messages use «guillemets» and `label_for(locale)`.

## Flag to user at the end
- App never launched. `next build` passed (escalated). Last checks: core tests pass, `cargo check` ok, `tsc` clean, `check:i18n` 1700 keys, new `bun` tests pass. Port-binding tests fail only in the sandbox (3 `ai::tests`, `parakeet_engine` download tests, telegram http test).
- All Persian copy is my draft and needs native and legal review: AI consent/privacy, Telegram consent and privacy (`CONSENT_VERSION` stays 1), questionnaire caution banner, `conditions.*`/`scoring.*`/`expr.*`, model-manager units, `about.*`, `btWarn.*`, default field labels, `describe_preference_for`, Telegram engine strings, validation messages.
- `pnpm add` reported `@resvg/resvg-js` and `opentype.js` as removed devDependencies; the diff shows only additions. Verify before committing.
- 16 `bun test` failures already exist at HEAD (35 pass).
- Copied transcript/summary markdown is localized (easy to revert).
- Inline pre-hydration script in `layout.tsx` may be blocked by the Tauri CSP (costs only a flash).
- `export.rs` CSV is deliberately not localized; Rust has no HTML/printable export (not yet told the user, now told).
- 134+ files changed in `git diff --stat`.
