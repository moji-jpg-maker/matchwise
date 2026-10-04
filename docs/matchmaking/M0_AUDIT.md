# M0 Audit of the fork (Meetily v0.4.1 base)

Audited from a shallow clone of `moji-jpg-maker/deep-matcher` (`main`, commit a2cb62e). The Tauri app was not
built here (no WebKit/audio toolchain in the audit sandbox); findings come from reading source and config.

## Layout (confirmed)
- Cargo workspace: `frontend/src-tauri` (crate `meetily`, lib `app_lib`) and `llama-helper`. `matchmaking-core` added in this change.
- Rust modules in `frontend/src-tauri/src`: `audio/`, `audio_v2/`, `whisper_engine/`, `parakeet_engine/`, `summary/`,
  `database/` (sqlx 0.8 + SQLite, `migrations/`), `ollama/`, `anthropic/`, `groq/`, `openai/`, `openrouter/`, `notifications/`, `analytics/`, `api/`.
- The plan's `src-tauri/` path is really `frontend/src-tauri/`. Next.js UI is `frontend/src/`.

## Must-fix before any real profile data touches this app
1. **Hard-coded PostHog key sends analytics to Meetily's project.** `src/analytics/commands.rs` (`init_analytics`) builds a
   config with a baked-in `phc_...` key, host `https://us.i.posthog.com`, `enabled: true`. Check how the frontend gates this
   (`AnalyticsProvider.tsx`, `AnalyticsConsentSwitch.tsx`) and then remove the module, the `posthog-rs` dependency, and the UI.
2. **Auto-updater points at Meetily's GitHub releases** and carries Meetily's public signing key
   (`tauri.conf.json` -> `plugins.updater`). Remove the updater plugin (or point it to your own signed releases and key).
3. **Plain-text API keys in SQLite.** `settings` / `transcript_settings` hold `groqApiKey`, `openaiApiKey`,
   `anthropicApiKey`, ... as text columns (initial schema + later migrations). Move secrets to the OS keychain; do not keep in the DB.
4. **Database is unencrypted** (`sqlx` + plain SQLite via `SqlitePool::connect`). Profiles contain religion, health, children and
   finances. Needs SQLCipher (or equivalent) and encrypted photo storage.
5. **Meetily "PRO"/license code** (`api/api.rs`: `license_key`, `is_licensed`; migrations `add_pro_license_custom_openai`, `add_grace_period_to_licensing`) -- remove.
6. **CSP `connect-src`** lists localhost ports 5167/8178 (legacy backend) and `api.ollama.ai`; tighten to what the new app uses.

## Cleanup (low risk, no behaviour change for matchmaking)
- Remove `backend/` (archived FastAPI + whisper.cpp submodule in `.gitmodules`).
- Dead code: `src/lib_old_complex.rs` (2437 lines, not referenced from `lib.rs`/`main.rs`), `audio/core-old.rs`,
  `audio/recording_saver_old.rs`, `audio/recording_commands.rs.backup`, `frontend/build_backup.bat`.
- `frontend/vs_buildtools.exe` (4.4 MB binary committed), `frontend/src-tauri/logs/`.
- Rebrand: `productName`, `identifier` (`com.meetily.ai`), window title, Cargo `name/authors/repository`, README, `PRIVACY_POLICY.md`, `CLAUDE.md`.
  Keep the MIT `LICENSE.md` and the acknowledgments. Changing `identifier` changes the app-data directory.

## Keep / reuse
- LLM provider layer (`summary/llm_client.rs`, `ollama/`, `anthropic/`, `groq/`, `openai/`, `openrouter/`) for profile extraction and pair analysis.
- sqlx migration setup (`database/manager.rs`, `migrations/`), repository pattern, Tauri command/event pattern, notifications module.
- Optional: local Whisper/Parakeet for matchmaker intake interviews. If dropped, remove `whisper-rs`, `ort`, `cpal`,
  `silero`, `ffmpeg-sidecar`, `cidre` (large build-time and platform risk).

## Open question
`database/manager.rs` auto-copies a legacy `meeting_minutes.db` into the app dir; remove with the rebrand.

## M0 status (branch `remove-telemetry-updater`; Rust parts are unverified until you run `cargo check`, frontend type-checks with `tsc`)
Done:
- **Telemetry removed.** Deleted the Rust `analytics/` module, the PostHog dependency, all `analytics::commands::*` registrations and the
  meeting-ended tracking block in `audio/recording_commands.rs`. Frontend: deleted the analytics provider/consent switch/data modal and
  replaced `lib/analytics.ts` with an inert no-op shim so the ~100 remaining call sites compile. **Follow-up:** delete those call sites, then the shim.
- **Updater removed.** Deleted the `tauri-plugin-updater` dependency and registration, the `updater:default` permission, the `plugins.updater` config and
  `createUpdaterArtifacts`; frontend update dialog/notification/provider/service/hook removed and `@tauri-apps/plugin-updater` dropped from `package.json`.
  **Follow-up:** CI release workflows and `scripts/*update*` still reference updater manifests.
- Deleted legacy `backend/`, dead `*_old` files, committed binaries and logs (earlier commit).
- Rebrand to Matchwise (earlier commit). `About.tsx` rewritten (no upstream marketing or links).
- Lockfiles: run `cargo check` (prunes `posthog-rs`/updater entries from `Cargo.lock`) and `pnpm install` (prunes `plugin-updater` from `pnpm-lock.yaml`), then commit the lock changes.

- **License/PRO code removed** (branch `remove-license`, unverified until `cargo check`): deleted the `api_get_profile` / `api_save_profile` / `api_update_profile`
  commands, their request/response structs and the now-unused `make_api_request` helper (they only called the removed localhost:5167 backend).
  Existing migrations are left untouched (editing applied migrations breaks checksum validation on existing dev databases); the new migration
  `20261004000000_drop_licensing.sql` drops the `licensing` table. CI no longer passes `MEETILY_RSA_PUBLIC_KEY` / `SUPABASE_*`.

Not done: keychain for API keys, encrypted DB, CSP tightening (remove `localhost:5167/8178`), release workflow/updater manifest scripts, shim call-site cleanup.
