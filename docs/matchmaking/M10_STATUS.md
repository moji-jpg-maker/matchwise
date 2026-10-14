# M10 status: AI provider and model selection

Diagnosis and repair of how the matchmaking AI picks a provider and a model, and how each matchmaking task is run.
MatchWise inherited Meetily's AI plumbing; this milestone reuses what is generic (the downloader, the model file, the
sidecar) and gives the matchmaking side its own registry, task profiles and checks. No new workflows: only the
existing Extraction and PairAnalysis tasks are covered.

## Principles
- **Advisory only.** A model output becomes a stored suggestion or analysis. It never changes a score or a status
  (`the_ai_cannot_change_scores_or_status` still passes, and pair analysis now also drops statements that *claim* to).
- **MatchWise owns its model list.** Meetily's summary registry is not the list of models offered for matchmaking.
- **One source of truth** for provider metadata (`ai/models.rs`), instead of per-file match statements.
- **No silent fallback.** A selection is saved as chosen. If its model is missing the page says so and offers the
  download; nothing is swapped behind the user's back.

## Root causes found (by reading the code)
1. **Built-in models were Meetily's summary models.** `validate_config` accepted any name known to
   `summary_engine::get_model_by_name`, and `BuiltinBackend` passed it straight on (`ai/settings.rs`, `ai/backend.rs`).
   A model added for meeting summaries would silently become selectable for matchmaking, including `gemma3:1b`.
2. **Provider metadata was scattered.** `default_base_url` and `kind_of` were separate matches; `default_urls` in the
   settings view listed only three providers, so `builtin` was missing; the page hardcoded its provider list and the
   `qwen3.5:4b` default.
3. **Two spellings, two stores.** Meetily writes `builtin-ai` (`onboarding.rs`, `database/commands.rs`,
   `database/repositories/setting.rs`) to its own settings repository; MatchWise stores `builtin` under the `ai_config`
   meta key. The stores are separate, so Meetily's summary provider cannot overwrite the MatchWise config, but a value
   with the other spelling was not recognised.
4. **Summary sampling was used for JSON tasks.** The built-in backend inherited the model's summary sampling
   (temperature 0.5 for Qwen, 1.0 for Gemma), 4096 tokens and a 900 s timeout. HTTP backends hardcoded temperature
   0.2 and Anthropic `max_tokens` 2048 for every task.
5. **Nothing told the user why AI was not usable.** The page showed a generic "Not ready" string.
6. **No real model has been exercised** (still true, see Not verified).

## Pieces
- `summary/summary_engine/client.rs`: `generate_with_builtin_overrides` and `GenerationOverrides` (sampling, max
  tokens, timeout). `None` keeps the old behaviour, so Meetily summaries are unchanged.
- `ai/models.rs` (new):
  - `MmModelDef` registry: `qwen3.5:4b` (recommended), `qwen3.5:2b`, `gemma3:4b`, each mapped to the summary model that
    supplies the file and download.
  - `TaskProfile { Extraction, PairAnalysis }`: temperature 0.1 / 0.2, max tokens 3072 / 1536, timeout 600 / 300 s,
    near-greedy sampling that keeps the model's stop tokens. `for_kind` maps the logged call kind to a profile.
  - `provider_catalog()`, `normalize_provider` (`builtin-ai` becomes `builtin` on read and save), and a typed
    `Readiness` (`ready`, `disabled`, `model_missing`, `needs_key`, `needs_consent`, `invalid`).
- `ai/backend.rs`: `LlmBackend::complete_for(task, ...)` (defaults to `complete`). `HttpBackend` applies the profile's
  temperature and token limit (Ollama `num_predict`, Anthropic `max_tokens`). `BuiltinBackend` checks the file exists
  and applies the sampling overrides.
- `ai/service.rs`: `ask` calls `complete_for` with the profile for its call kind.
- `ai/commands.rs`: the settings view gains `catalog`, `builtin_models` (with file state) and `readiness`.
- `app/ai/page.tsx`, `types/matchmaking.ts`: the page renders from the catalog and model list, with no hardcoded
  provider list or default model, and shows the readiness state.
- `matchmaking-core/src/ai.rs`: pair-analysis claims that try to decide a score or status are dropped and reported in
  `dropped`. Questions are exempt (they only ask).

## Verified here
- `cargo check` in `frontend/src-tauri` compiles.
- `cargo test --lib ai::`: 36 passed, 3 failed. The 3 are the stand-in HTTP tests that bind a loopback socket,
  blocked by this sandbox (`PermissionDenied`), and failed the same way before this work.
- New tests cover: registry maps to real summary models, unknown or Meetily-only models rejected, catalog covers every
  provider, alias normalisation (stored and legacy), save/reload with a model not downloaded, each readiness state,
  per-task profile selection, the service using the Extraction profile, and a full run
  settings, gate, session, analysis, stored result with the match's status and score snapshots unchanged.
- `cargo test -p matchmaking-core`: all pass, including the new authority-claim test.
- `npx tsc --noEmit` in `frontend/`: clean.
- The Windows release build carries this work: run `38589160522` (commit `8515db1`, unsigned) went green and produced
  `matchwise_0.4.1_x64-setup.exe` (NSIS, 43 MB, sha256 `7250f8bd...46c1`) and `matchwise_0.4.1_x64_en-US.msi`. That
  proves the AI layer compiles and bundles on Windows; it does not prove any model runs there.

## Not verified
- **No real model was run.** Neither the built-in sidecar with a downloaded GGUF, nor a real Ollama, nor a cloud
  provider. Output quality, whether the 3072/1536 token caps are enough, and the timeouts are unmeasured.
- The three loopback HTTP tests, which should be run on a normal machine.
- The page was type-checked, not opened. The download button and progress event were not exercised.
- The `AUTHORITY_PHRASES` list is a short English list; it will not catch every phrasing or other languages.

## Differences from the plan
- Readiness is computed in Rust from the config, gate and file presence. `unreachable` (a server that is down) is not a
  state: it is only discovered by Test connection, since probing on every page load would contact the provider.
- Per-task profiles apply temperature and token limits to every provider; sampling beyond temperature (top-k, top-p)
  applies only to the built-in sidecar.

## Checklist for a person with a real machine
1. AI page, Built-in, pick Qwen 3.5 4B, Download, Save. The badge should read Ready.
2. Test connection, then run Extract on a profile with notes and Analyse on a tracked match.
3. Repeat with Ollama and a model you have pulled.
4. Run `cargo test --lib ai::` outside the sandbox; expect no failures.

## Tests
`frontend/src-tauri/src/ai/tests.rs` (module `selection`), `matchmaking-core/tests/ai.rs`.
