# M12 status: configurable autonomy

M12 adds an autonomy level, L0 Manual to L4 Autonomous, in eight vertical phases. **Phases 1–2 and step 3.1 are built:
policy-checked extraction acceptance, an on-demand read/analysis agent, and eligible-pair recommendation/review.** Nothing has been run with a real model.

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

## Phase 2: what shipped
- `src-tauri/src/agent/`: one validated JSON action per turn, with GBNF and JSON Schema generated from the same
  tool registry and passed through `complete_structured`. `TaskProfile::Agent` uses 512 output tokens and a
  120-second timeout. Runs stop at `finish`, 20 turns, a closed gate, a configuration change or the kill switch.
- Tools: `list_work`, `get_profile_redacted`, `run_extraction`, `accept_suggestion`, `find_matches`, `create_match`,
  `analyse_pair`, `finish`. Profile observations use the existing redactor and sensitive-field gate; observations
  are explicitly untrusted data. Tool errors returned to the model contain no raw profile text.
- Extraction, suggestion acceptance, scoring, match creation and pair analysis reuse existing validated services.
  Agent extraction leaves suggestions pending until the explicit acceptance tool. Accepted fields retain rank-zero
  provenance and undo history. Created matches stay Identified; no recommendation or approval transitions ship here.
- Storage: `mm_agent_runs` and `mm_agent_steps`, including per-turn actions, observations, status and actor.
  Real run IDs identify agent writes and accepted-field undo records. Suggest-mode actions are persisted proposals;
  a matchmaker can reject or edit arguments and approve, with current policy and invariant checks repeated.
- Commands: `agent_run_now`, `agent_runs`, `agent_run_detail`, `agent_proposals`, `agent_decide_proposal`.
- UI: Run now on saved active profiles and imported intake batches; run history, expandable traces and editable
  approvals inbox on `app/autonomy`. English and Persian. Batches execute sequentially, one subject per run.

## Step 3.1: what shipped
- `transition_match` accepts only `recommended` or `reviewed`; `add_note` adds a private match note. Both
  appear in the generated action grammar/schema, existing run traces and approvals inbox. Recommend and AddNote
  are enabled on the autonomy page: L2 executes them, L1 proposes them.
- Core `validate_transition_by` restricts agent transitions to eligible Identified → Recommended and
  Recommended → Reviewed. Agents cannot supply overrides, attach outcomes or reconsider rejections.
- The tools reuse the human lifecycle, repository, audit and notification services. Each request must involve the
  run's target. Recommendation/review checks the saved scorecard and re-scores current active profiles with the
  match's recorded rules/config, including current preferences. Proposal approval repeats these checks.
- Match timeline events carry AI badges and an Undo button for the latest agent recommendation/review.
  `agent_undo_transition` atomically restores the prior status and records a matchmaker undo event/audit row.
  Undo is refused after any later lifecycle event, including moving away and back; notes do not block it.
  A consumed undo cannot expose earlier transitions for cascading undo. No notification is sent for these internal moves.
- Migration `20261112020000_add_agent_transition_undo.sql` stores one-use transition undo receipts.
  Free-text notes stay in match notes and agent traces, outside `mm_audit_log`. English and Persian timeline strings ship.
- Scripted tests cover L2 recommendation/review/notes, L1 proposals and edited approval, excluded/unrelated pairs,
  stale profiles at approval, actor attribution, no outbox writes and undo after later human changes.

## Decisions worth knowing
- The phase-1 deterministic step still records actor `agent:0`; its `run_id` remains NULL in `mm_agent_actions`.
  On-demand runtime actions instead record their real `agent:<run_id>` actor.
- L4 is unreleased until phase 8: `save_policy` rejects it and the UI disables it. A step that is not built always
  counts as Off.
- The rules-only tier is gated by the AI gate too, because a closed gate stops all agent actions.
- `run_accept_all` in `ai/intake_commands.rs` is the manual, unthresholded path and is untouched.
- Seven steps are built: RunExtraction, AcceptExtraction, FindMatches, CreateMatch, AnalysePair, AddNote and Recommend.
  Read-only inventory/profile tools and finish still require an open AI gate and an unpaused policy.
- Runtime/model access is serialized. There is no background scheduler or review-window executor: a ReviewWindow
  action is explicitly denied as unavailable, never executed early. Hourly/message budgets belong to later phases.

## Verified here
- `cargo test -p matchmaking-core`: 188 passed, including the built-step policy matrix, invariant denials
  and actor-aware lifecycle checks.
- `cargo test --lib autonomy_tests` in `src-tauri`: 14 passed.
- Frontend: `tsc --noEmit`, `check:i18n`, `check:invoke` and `check:autonomy` pass.
- Scripted backend agent tests cover stored matches plus analyses at L2, proposals/edited approval/rejection at L1,
  extraction and explicit acceptance, redacted observations, rank-zero undo, invariant denials, gate/config changes,
  batch subjects, unavailable review windows, invalid actions and the 20-turn stop. A prompt regression test lives in
  `ai/tests.rs`. All 14 agent tests pass, including step 3.1 recommendation/review, notes and transition undo.
- `cargo test -p matchwise --lib`: 448 passed, 2 ignored. The suite was rerun outside the filesystem/network sandbox
  so its local HTTP fixture servers could bind sockets; no real model was used.

## Still open
- Steps 3.2–8: approval/rejection, overrides, scheduler and agent messages to candidates remain unavailable.
- No run with a real model, and the UI was type-checked but not clicked through in a running app.
