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
**Resolved.** The copy, `DatabaseManager::import_legacy_database`, the five legacy-database commands
(`select_legacy_database_path`, `detect_legacy_database`, `check_default_legacy_database`, `check_homebrew_database`,
`import_and_initialize_database`) and the unused `DatabaseImport/` UI (`LegacyDatabaseImport.tsx`,
`HomebrewDatabaseDetector.tsx`) are all deleted. Onboarding now always starts a fresh encrypted database on first launch;
importing an old plaintext Meetily file would have bypassed the encryption-at-rest guarantee, and the schema has diverged.

## M0 status (branch `remove-telemetry-updater`; verified locally -- see the checks below)

Checks run on this worktree:
- `cargo check --workspace --all-targets --offline` -> exit 0, warnings only. This prunes `posthog-rs` and the updater entries from `Cargo.lock`.
- `cargo test -p matchwise --offline database::encryption` -> 10 passed / 0 failed. Covers the real conversion path, not just the scratch crate:
  fresh install, plaintext conversion with a verified backup, wrong key, missing key, resumed interrupted conversion, conflicting backup,
  missing database next to a backup, row-count verification, key validation, and a compile-time `Send` check on the future.
- `tsc --noEmit` (frontend) -> exit 0.
- `bun test` on the two touched hook suites (meeting-details refresh, summary generation) -> 19 passed / 0 failed.
Done:
- **Telemetry removed.** Deleted the Rust `analytics/` module, the PostHog dependency, all `analytics::commands::*` registrations and the
  meeting-ended tracking block in `audio/recording_commands.rs`. Frontend: deleted the analytics provider/consent switch/data modal and
  replaced `lib/analytics.ts` with an inert no-op shim so the ~100 remaining call sites compile. **Shim cleanup done:** `lib/analytics.ts` and the shim's own
  test (`tests/lib/analytics.test.ts`) are deleted, and all 73 call sites across 24 files (pages, providers, hooks, components) are gone. Dead code that only
  existed to feed the shim went with them: `getDeviceMetadata` in `DeviceSelection.tsx`, the `trackPreferencesViewed` effect in `PreferenceSettings.tsx`,
  two empty `useEffect`s, the analytics `try/catch` in `useRecordingStop.ts` (including its `Store.load('analytics.json')` and the `totalMeetings === 1`
  first-run branch), an empty `if (fileInfo) {}` guard in `useImportAudio.ts`, and `trackedAttemptRef` / `finishGeneration` / the `outcome` parameter in
  `useSummaryGeneration.ts`. Two analytics-specific hook tests were renamed to describe the behaviour they actually assert. `tsc --noEmit` and the two hook
  suites pass.
- **Updater removed.** Deleted the `tauri-plugin-updater` dependency and registration, the `updater:default` permission, the `plugins.updater` config and
  `createUpdaterArtifacts`; frontend update dialog/notification/provider/service/hook removed and `@tauri-apps/plugin-updater` dropped from `package.json`.
  **Follow-up:** the dead `TAURI_SIGNING_PRIVATE_KEY` env plumbing was removed from all five tauri-action call sites, and the three release
  workflows now default `sign-build: false` so an unsigned build is the default. `release.yml` no longer looks for `latest.json`, and the orphaned
  `scripts/generate-update-manifest-github.js` / `scripts/test-update-locally.js` are deleted. `frontend/build.ps1` no longer demands the removed updater key:
  signing is now opt-in via `-Sign`, off by default, and checks `DIGICERT_KEYPAIR_ALIAS` (what `tauri.conf.json`'s signCommand actually reads). `.env.example`
  (repo root) was rewritten to list the real variables (`MATCHWISE_DB_KEY`, `MATCHWISE_TELEGRAM_TOKEN`, `MATCHWISE_AI_API_KEY`, the DigiCert set). No `TAURI_SIGNING_*` reference survives anywhere in the repo.

- **Local `.env` now actually takes effect.** Nothing loaded it before: the Rust side reads only `std::env::var` (no dotenv crate anywhere), and the only loader ran in
  `frontend/build.ps1`, pointed at `frontend/.env`, while the real secrets file sits at the repo root. So `pnpm run tauri:dev` on WSL silently ignored a root `.env` and
  the key/token had to be exported by hand every session. Added `frontend/scripts/load-env.js` (parser + optional command runner) and wired it into all
  `tauri:dev*` / `tauri:build*` scripts and `tauri-auto.js`; `build.ps1` now reads the root `.env` and passes `-NoOverride`. Precedence is uniform: an exported
  variable always beats the file, and a missing file is not an error (the credential store remains the default source). `.env.example` moved from `frontend/` to
  the repo root, which also keeps these values out of the Next.js directory. Verified: the JS loader forwards the vars to the real `tauri` CLI, propagates exit
  codes, and passes arguments verbatim.
- Deleted legacy `backend/`, dead `*_old` files, committed binaries and logs (earlier commit).
- Rebrand to Matchwise (earlier commit). `About.tsx` rewritten (no upstream marketing or links).
- Lockfiles: done. `cargo check` pruned `posthog-rs` and the updater entries from the workspace `Cargo.lock`, and `pnpm install` dropped `plugin-updater`
  from `frontend/pnpm-lock.yaml`. Both are already clean in the worktree.

- **License/PRO code removed** (branch `remove-license`; `cargo check` is green on it): deleted the `api_get_profile` / `api_save_profile` / `api_update_profile`
  commands, their request/response structs and the now-unused `make_api_request` helper (they only called the removed localhost:5167 backend).
  Existing migrations are left untouched (editing applied migrations breaks checksum validation on existing dev databases); the new migration
  `20261004000000_drop_licensing.sql` drops the `licensing` table. CI no longer passes `MEETILY_RSA_PUBLIC_KEY` / `SUPABASE_*`.

- **CSP tightened** (`tauri.conf.json`): `connect-src 'self'` only (the frontend makes no network requests; all HTTP is done in Rust), plus `object-src 'none'`, `base-uri 'self'`, `form-action 'self'`.
- **Database encrypted at rest** (SQLCipher, via `libsqlite3-sys` `bundled-sqlcipher-vendored-openssl` alongside sqlx's bundled `sqlite` feature).
  Revised design: instead of moving each API key into the OS keychain, one random 256-bit master key lives in the OS credential store
  (Windows Credential Manager / macOS Keychain / Linux Secret Service) and encrypts the whole database, so API keys, profiles and everything else are protected together.
  - `MATCHWISE_DB_KEY` (64 hex chars) overrides the credential store: needed on WSL/headless Linux without a Secret Service daemon.
  - A new key is created only when no encrypted database exists; if the key is lost the data is unrecoverable by design.
  - An existing plaintext database (old dev DB or imported legacy DB) is converted automatically; the plaintext original is kept as `<db>.plaintext.bak`
    and **must be deleted by you** once verified. Verified in a scratch crate: conversion, user_version, wrong-key rejection.
  - Build requirements: `perl` and `make` (vendored OpenSSL); Linux also needs the D-Bus dev package (`libdbus-1-dev`) for the keyring crate.
    Windows-native builds of vendored OpenSSL additionally need Strawberry Perl and NASM. `build.yml`, `build-windows.yml` and `build-devtest.yml`
    now install both (choco `strawberryperl` + `nasm`, added to `PATH`) on their Windows runners; still unverified until a Windows CI run goes green.
    `sqlx`'s `sqlite-unbundled` must not be enabled.

- **Encryption review fixes** (branch `fix-encryption-review`):
  - Conversion is now crash-safe. The plaintext original is never renamed away: the encrypted copy is built and verified first
    (key opens it, `integrity_check`, `user_version`, per-table row counts), the plaintext is *copied* to `.plaintext.bak` and compared byte-for-byte,
    and only then is the encrypted file atomically renamed over the database. A crash at any step leaves a complete database at the path;
    a stale `.enc.tmp` is discarded and an identical `.bak` is reused on the next start. A `.bak` that differs from the database blocks
    conversion with an error instead of being overwritten.
  - `database::encryption::open_database` is now the single entry point used by `DatabaseManager::new`. For an existing encrypted database it asks for the
    key with creation disabled, and proves the key opens the file (read-only) *before* connecting or migrating; on failure it returns an error and
    modifies nothing. It also refuses to start a new empty database when a `.plaintext.bak` exists but the database is missing.
  - Tests now cover that real path (10 tests): fresh install, conversion, wrong key (directory byte-for-byte unchanged), missing key, interrupted conversion,
    conflicting backup, missing database next to a backup, row-count verification, and a compile-time check that the future is `Send`.
    Run in the app with `cargo test -p matchwise database::encryption` (`database::encryption::tests`) -- confirmed 10/10 green on this worktree. What remains untested: the OS credential-store
    branch (needs a real Secret Service/Keychain/Credential Manager) and `DatabaseManager::new` itself (needs the migrations and a Tauri build).
  - CSP: `asset.localhost` appears only in `img-src`, unchanged from upstream; upstream's `connect-src` never allowed it either, and the frontend does not
    use `convertFileSrc` or asset URLs for audio (it uses `AudioContext`). So this change removed no asset access, but the running app still needs a
    manual check: open the WebView devtools console, exercise onboarding, settings, a recording playback and the summary editor, and confirm there are no
    "Refused to ..." CSP messages.

Not done: encrypted photo storage (no photo feature yet), legacy-import UI removal. The analytics shim call-site cleanup and the orphaned release-manifest
scripts are both finished. Still unverified from this machine: the Windows CI job (a real Windows run is the only proof the Strawberry Perl + NASM step works)
and the OS credential-store branch (needs a real Secret Service / Keychain / Credential Manager).
