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

## M0 status (branch `m0-safety-cleanup`, written without compiling the Tauri app -- run `cargo check` first)
Done:
- Telemetry hard-disabled at its single choke point (`analytics/commands.rs`: empty key, no host, `enabled: false`).
  All `track_*` calls become no-ops. **Still to do:** delete the `analytics/` module, `posthog-rs`, and the ~30 frontend call sites/UI.
- Updater neutralized in `tauri.conf.json` (`endpoints: []`, `createUpdaterArtifacts: false`) so the app can never be replaced by upstream Meetily builds.
  **Still to do:** remove the plugin registration (`lib.rs`), the `updater:default` capability, the Cargo dependency, and `Update*` UI; the update check will now error until then.
- Deleted `backend/` (+ `.gitmodules`), `lib_old_complex.rs`, `audio/core-old.rs`, `audio/recording_saver_old.rs`, `recording_commands.rs.backup`,
  `frontend/build_backup.bat`, `frontend/vs_buildtools.exe`, `src-tauri/logs/`. None were referenced by `mod` declarations or CI; two test paths in
  `audio/import.rs` pointed at `backend/whisper.cpp/samples` (check those tests still behave as you expect).
Not done: rebrand, license/PRO removal, keychain for API keys, encrypted DB, CSP tightening.
