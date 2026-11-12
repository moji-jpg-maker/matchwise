# M12 status: configurable autonomy

M12 adds an autonomy level, L0 Manual to L4 Autonomous, in eight vertical phases. **Only phase 1 is built:
the policy engine and autonomous acceptance of extraction suggestions.** There is no LLM loop yet, and nothing
has been run with a real model.

## Phase 1: what shipped
- `matchmaking-core/src/autonomy.rs`: `Actor`, `Step` (all 18, so a stored policy never changes shape), `StepMode`
  (Off, Suggest, ReviewWindow, Auto), `Level` presets, `AutonomyPolicy`, and `decide(action, ctx, policy)` returning
  Execute, Propose, Schedule or Deny. The invariants are checked before the step mode: kill switch and closed AI
  gate, the human-only list, never setting Interest, NEVER_SHARED and contact data. `acceptance_check` decides
  whether one suggestion may be accepted (threshold, ambiguity, person-owned values).
- Storage: `mm_agent_policy` (versioned JSON plus `paused`) and `mm_agent_actions` (migration
  `20261112000000_add_agent_autonomy.sql`). `MatchRepo` writers have `*_as(pool, actor, …)` variants,
  `MmRepository::audit_as` and `decide_suggestion_as` take an actor; the old signatures pass `Actor::Matchmaker`.
- `mm/autonomy.rs`: `settle_extraction` runs inside `extract_profile` after the run is stored. At L2 or above
  (`AcceptExtraction` Auto) it accepts suggestions at or above `min_confidence` (default 0.85). Ambiguous fields
  and fields a person owns are never accepted. In Suggest mode nothing changes from today. `undo_action` restores
  the prior value, and refuses if a person edited the field since.
- Commands: `agent_get_policy`, `agent_save_policy`, `agent_set_paused`, `agent_undo`.
- UI: `app/autonomy` (level picker, per-step table with unbuilt steps disabled, threshold, pause), a kill-switch
  bar in `MainContent`, an "AI" badge with undo on accepted fields in `app/profile`. English and Persian.
- Contract test: `npm run check:autonomy` compares the TypeScript step, mode and level names with
  `matchmaking-core/tests/fixtures/autonomy/policy.json`.

## Decisions worth knowing
- The deterministic step records actor `agent:0`; `run_id` is NULL in `mm_agent_actions`. Phase 2 will store real run
  ids in the same column, so no migration is needed.
- L4 is unreleased until phase 8: `save_policy` rejects it and the UI disables it. A step that is not built always
  counts as Off.
- The rules-only tier is gated by the AI gate too, because a closed gate stops all agent actions.
- `run_accept_all` in `ai/intake_commands.rs` is the manual, unthresholded path and is untouched.
- Actor threading exists as `*_as` variants everywhere, but only the suggestion path uses the agent actor so far.

## Verified here
- `cargo test -p matchmaking-core`: passes, including the 18 autonomy tests.
- `cargo test --lib autonomy_tests` in `src-tauri`: 14 passed.
- Frontend: `tsc --noEmit`, `check:i18n`, `check:invoke` and `check:autonomy` pass.
- `cargo test --manifest-path frontend/src-tauri/Cargo.toml --lib`: 415 passed, 18 failed. The 18 are the known
  loopback/network tests that cannot bind a socket in this sandbox (`PermissionDenied`); none touch autonomy.

## Still open
- Phases 2-8. No agent runtime, no scheduler, no agent messages to candidates.
- No run with a real model, and the UI was type-checked but not clicked through in a running app.
