# Matchwise

Privacy-first, AI-assisted matchmaking for human matchmakers. Matchwise is a desktop application (Tauri + Next.js + Rust)
that keeps profile data on your machine, applies configurable rules and compatibility scoring, uses LLMs only as an
assistive layer, and keeps the human matchmaker in control of every introduction.

> **Status: early development.** The matching engine (`matchmaking-core/`) has its first pieces in place; the desktop app
> is still being converted from its upstream base. See [`docs/matchmaking/PLAN.md`](docs/matchmaking/PLAN.md) for the
> roadmap and [`docs/matchmaking/M0_AUDIT.md`](docs/matchmaking/M0_AUDIT.md) for the cleanup status.

## Principles
- **Local-first and private.** No telemetry. Profile data is sensitive and stays on the device unless you explicitly send it somewhere (for example a cloud LLM or Telegram).
- **Deterministic core, AI on top.** Hard constraints and rules are data-driven and explainable; AI suggestions never overwrite human-entered data.
- **Missing data is visible.** Unknown information is reported, never guessed.
- **Human decides.** Every recommendation can be reviewed, overridden, and annotated by the matchmaker.

## Repository layout
```
matchmaking-core/     pure-Rust engine: field registry, profiles + provenance, rule engine
frontend/             Next.js UI (src/) and the Tauri Rust app (src-tauri/)
llama-helper/         local LLM helper sidecar (inherited)
docs/                 plan, audit, and inherited build documentation
```

## Development
Build instructions inherited from the upstream base are in [`docs/BUILDING.md`](docs/BUILDING.md) and
[`docs/building_in_linux.md`](docs/building_in_linux.md) (some content still refers to the old product name).

The app database is encrypted. On Linux/WSL, install `libdbus-1-dev`, `perl` and `make` to build, and either run a Secret Service
(gnome-keyring) or provide a key yourself before starting the app:
```bash
export MATCHWISE_DB_KEY=$(openssl rand -hex 32)   # keep a copy; without it the database cannot be read
```

```bash
cargo test -p matchmaking-core          # engine tests
cd frontend && pnpm install && pnpm run tauri:dev
```

## Telegram
Matchwise can talk to candidates through a bot you create yourself (BotFather). See the Telegram page in the app and `docs/matchmaking/M5_STATUS.md`. On WSL or a machine without a credential store,
provide the token as `MATCHWISE_TELEGRAM_TOKEN`. The bot answers only while the app is running.

## AI assistant
Optional and off by default. Recommended: a model on this computer (Ollama); see the AI page in the app and `docs/matchmaking/M6_STATUS.md`. For a cloud provider on WSL or a machine without a
credential store, provide the key as `MATCHWISE_AI_API_KEY`.

## Origin and license
Matchwise started as a fork of [Meetily](https://github.com/Zackriya-Solutions/meetily) by Zackriya Solutions, released under the
MIT License. The original copyright notice is preserved in [`LICENSE.md`](LICENSE.md). Matchwise is not affiliated with or endorsed by Zackriya Solutions.
