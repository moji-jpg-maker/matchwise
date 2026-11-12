# M12 status: configurable autonomy

M12 adds an autonomy level, L0 Manual to L4 Autonomous, in eight vertical phases. **Phases 1–2 and steps 3.1–3.3 and 4.1–4.3 / 5.1–5.2 / 6.1–6.2 are built:
policy-checked extraction acceptance, an on-demand read/analysis agent, eligible-pair recommendation/review/approval,
reasoned rejection, policy-controlled rule overrides, background scheduling, review windows, veto, configurable budgets, template introductions, template follow-ups, screened AI-written follow-ups and missing-info requests.** Nothing has been run with a real model.

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
  120-second timeout. Runs stop at `finish`, the configured step limit (default 20), a closed gate, a configuration change or the kill switch.
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

## Step 3.2: what shipped
- `transition_match` also accepts `approved` and `rejected`, selecting the independent Approve or Reject policy
  step from the requested destination. Editing a proposal repeats this selection and all lifecycle checks.
  The existing stored `reject` policy key is preserved.
- L3 approves eligible Recommended/Reviewed pairs automatically; L1 and L2 propose approvals. Reject is an
  internal step: L2/L3 execute reasoned rejections and L1 proposes them. Both steps can be overridden individually
  on the autonomy page, using the existing English and Persian labels.
- Agent rejections require a nonempty reason, trimmed and limited to 2,000 characters. Rejected → Reviewed
  remains human only. Ineligible pairs cannot be approved, and rule overrides and outcomes remain unavailable
  to agent transitions. Rejecting an ineligible pair needs no override.
- Approval reuses current-profile eligibility checks and the existing lifecycle, repository, audit and notify path.
  Timeline events retain the real agent run actor and AI badge. Rejection reasons stay in events/traces;
  audit rows contain only transition metadata. Recommendation/review undo keeps its existing scope.
- The action grammar and JSON Schema include both destinations and an optional reason field; lifecycle validation
  requires it for rejections. Scripted tests cover L3 execution, L1 proposals, L2 independent step modes,
  edited destinations/reasons, eligibility rechecks, missing reasons, stale proposals and human-only reconsideration.

## Step 3.3: what shipped
- RuleOverride is available as an individual step on the autonomy page. L1/L2/L3 default to Suggest;
  custom Auto permits overrides. The L4 preset remains unreleased until phase 8, but its Auto modes for
  built steps can be selected individually now.
- `transition_match` accepts an optional `override_reason`, trimmed to at least three characters and bounded
  to 2,000 characters, including Persian. Excluded pairs can be recommended or approved only with both the
  destination step's permission and RuleOverride permission. Off denies either operation; Suggest proposes;
  review windows schedule the action and cannot be bypassed through proposal approval.
- Current-profile rescoring, active-profile checks, target ownership, the AI gate and kill switch still apply.
  Overrides cannot reconsider a rejection, review an excluded pair, record outcomes or change candidate Interest.
  Edited proposal approval repeats the same policy and lifecycle validation.
- Migration `20261112030000_add_agent_rule_override.sql` adds `mm_matches.override_by`, preserving historical
  overrides as `matchmaker`. New overrides record `agent:<run_id>` or the human actor through the existing
  lifecycle/repository/audit path. Reasons stay in match records, events and traces, outside `mm_audit_log`.
- The match list has All / Any override / AI override filters and an AI override badge in English and Persian.
  Undoing an agent recommendation restores its prior override reason and actor along with its status; undoing
  a later eligible review preserves an existing human override. Historical undo receipts are migrated too.
- Scripted tests cover custom Auto recommendation/approval of excluded pairs, L3 proposals, edited arguments,
  each required permission, current rule exclusions, paused/inactive targets, attribution and undo. Migration
  fixtures cover existing human overrides and undo receipts. No real model was used.

## Step 4.1: what shipped
- A desktop-managed agent worker starts with the initialized database and stops before database cleanup.
  It scans active profiles at startup and every 15 minutes, and polls queued events once per second.
  Each run handles one profile; background runs, manual runs and proposal decisions share the runtime lock.
- Migration `20261112040000_add_agent_scheduler.sql` adds a durable per-profile work queue. Completed intake
  batches enqueue their profiles with batch attribution. A transactional status-change trigger enqueues both
  match participants across human, agent, Telegram and undo paths; no-op updates and rollbacks enqueue nothing.
- Repeated events coalesce. Generation checks preserve events received during a run; inactive profiles are
  discarded, and profiles with pending proposals wait for a decision. Failed runs retry at the next periodic
  scan. Restarting the app preserves queued work.
- L0 and pause leave the worker idle. The AI/consent/redaction gate is checked before model construction;
  closed gates preserve queued work. Waiting model calls and model-lock waits stop on pause within the
  one-second policy tick or on shutdown cancellation. An already executing validated tool finishes its audit.
  Stopped runs are finalized as stopped and their queued work remains available after resume.
- Existing run history, traces, agent actors, approvals inbox and kill switch serve background runs too.
  The autonomy page explains the background behavior in English and Persian.
- Scripted tests cover unattended L2 execution and L1 proposals, stable ordering, intake completion, coalesced
  transactional status events, events during runs, proposal deferral, inactive profiles, gate/pause/L0 checks,
  shared-lock contention, waiting-model cancellation, repeated periodic scans and idempotent worker start/stop/restart.

## Step 4.2: what shipped
- ReviewWindow actions persist their original run, arguments, review start and deadline in `mm_agent_steps`
  (migration `20261112050000_add_agent_review_windows.sql`). Scheduled receipts appear in the approvals inbox
  with live countdowns, deadline times and a Veto button in English and Persian.
- The existing one-second worker processes one due receipt before queued model work, even without queued subjects.
  Expiry repeats current policy, AI/consent gates, active-profile, suggestion ownership/confidence and lifecycle/eligibility
  checks, then uses the existing validated tool with the original agent actor. It does not ask a model to regenerate the action.
- Pausing, closing the AI gate or shutting down preserves pending receipts. Restarting resumes from stored deadlines.
  A longer current window extends the wait from the original review start; shortening a window or selecting Auto never
  executes before the persisted deadline. Off denies; Suggest converts a due receipt to a human approval proposal.
  Combined destination/RuleOverride permissions remain independent, using the longest required window.
- `agent_veto` needs no AI session and remains available while paused or another run is busy. An atomic conditional
  status update races safely with the worker claim: a successful veto cannot execute. Veto remains possible after the
  deadline until the worker claims the action. Claimed actions are not automatically retried after interruption.
- Scheduled/vetoed/executed receipts remain in the run log. Schedule and veto audit entries carry their actors without
  free text. Scheduled subjects wait before another background run; manual proposal approval cannot skip the window.
- Scripted tests cover expiry, no early execution, once-only execution, actor attribution, veto before/after expiry,
  paused veto, closed gates and resume, changed policies/windows, stale eligibility, independent override permissions,
  and suggestion ownership and undo receipts. No real model was used.

## Step 4.3: what shipped
- Policy budgets default to 120 agent model calls per rolling hour, 20 turns per run, one open introduction per
  profile and three messages per candidate per day. Older policy JSON gets these defaults without changing its
  level, step modes or pause state. Each setting accepts nonnegative integers; zero disables the corresponding work.
- The autonomy page edits all four limits through the existing versioned policy command, with English and Persian
  labels. Changing a preset preserves the chosen budgets. Run history and tool traces identify hourly budget stops;
  step-budget stops use the configured limit rather than a fixed 20-turn message.
- Migration `20261112060000_add_agent_budgets.sql` adds durable, text-free model-request reservations linked to runs.
  An atomic conditional insert admits each request against organization-wide usage in the preceding hour. Agent
  turns, extraction, pair analysis and parsing retries share the same quota. Failed or cancelled requests consume
  their reserved slots; restarting the app or purging stored prompts cannot reset usage. Human requests are excluded.
- Current limits are checked before each turn and each nested model request, including proposal approval and due
  review-window execution. Reaching the hourly quota stops the run and records `model_call_limit`; delayed actions
  that exhaust it are finalized once with the same trace status. Step limits are rechecked after waiting for the model lock.
- The introduction admission guard checks both participants and counts existing human and agent introductions in
  IntroductionProposed, BothInterested, ContactExchanged, Conversation, Meeting and Feedback. Terminal matches
  release their slots. Step 5.1 now integrates these limits into atomic introduction admission and message reservations;
  existing human Telegram flows are unchanged.
- Tests cover upgrade/default and invalid policy values, quota boundaries, configured/zero run limits, shared hourly
  usage and expiry, atomic reservations, nested analysis/extraction retries, proposal approvals, due actions and
  introduction slot release. No real model was used.

## Step 5.1: what shipped
- `propose_introduction(match_id)` and `resend_interest_buttons(match_id, profile_id)` are registered structured tools
  under SendIntroduction. They accept template arguments only. L1/L2 propose; L3 schedules a 60-minute review
  window; a custom per-step Auto policy executes immediately. L4 remains disabled.
- Introductions require an approved eligible target match, active participants, current Telegram consent and enabled
  notifications on both sides. Existing lifecycle validation rechecks saved and current eligibility. Hidden, held,
  excluded and unrelated matches are denied. Resends require an unanswered participant of an existing introduction.
- Existing safe cards, localized templates, escaped text, interest buttons and the outbox carry delivery. The model
  supplies no outbound text. The work list exposes participant IDs and response states for selecting a resend.
- Delivery rechecks the kill switch and AI gate, leaving agent messages pending while either blocks execution.
  Stale buttons for answered, held, hidden or ended introductions are cancelled before sending.
- Migration `20261112070000_add_agent_introductions.sql` attributes outbox rows and adds a durable message ledger.
  Initial introductions reserve both participants' open-introduction capacity and daily message slots atomically
  with the lifecycle event, outbox and metadata-only audit. Resends consume a message slot. Pending and failed
  agent deliveries count for a rolling 24 hours; outbox cleanup cannot reset usage. Budget stops appear in the
  bilingual run log as `introduction_limit` or `message_limit`.
- Interest still comes only from candidates' Telegram buttons. After both candidates press yes on an agent
  introduction, the existing response flow records BothInterested, then a validated System transition records
  ContactExchanged. Only the existing mutual-interest notices are sent; contact values remain private. A decline
  produces the existing neutral notice, and replayed buttons do not duplicate messages. Human introductions keep
  their existing behavior.
- English/Persian conversation fixtures cover escaped introductions, privacy, free-text yes, both buttons, decline
  and replay. Scripted agent tests cover proposals, approval rechecks, Auto, resends, scope/eligibility denials,
  atomic budget rollback/expiry and review-window execution, veto and consent/budget changes. No real model was used.

## Step 5.2: what shipped
- `send_follow_up(profile_id, match_id?)` is a structured, template-only SendFollowUp tool. With a match ID it
  reminds an unanswered introduction participant; without one it reminds the target about missing required fields.
  `list_work` returns due reminder IDs. No model-written outbound text is accepted, and Interest remains button-only.
- L1/L2 propose follow-ups, L3 uses a 60-minute review window, and a custom Auto mode sends immediately. The
  autonomy page enables Follow-up and explains reminder caps in English and Persian. Existing run traces,
  editable approvals, veto and due execution apply, with eligibility checked again at approval and admission.
- Existing escaped English/Persian templates, buttons and outbox deliver the messages. Recipients need an active
  profile, current Telegram consent and enabled notifications. Introduction reminders require active participants,
  an existing introduction receipt and an unanswered, visible, unheld IntroductionProposed match involving the target.
- The existing three-day delay and spacing, two reminders per participant per introduction and three per incomplete
  profile are enforced inside the enqueue transaction. Migration `20261112080000_add_agent_follow_ups.sql` retains
  text-free admission history, backfills retained noncancelled outbox records and permits profile-only message receipts.
  Caps survive outbox cleanup; historical deliveries purged before upgrading cannot be recovered. New failed or
  cancelled attempts conservatively consume their cap. Profile deletion cascades the metadata receipts.
- Follow-ups share the rolling 24-hour candidate message ledger with introductions and resends. A write lock,
  current profile/link snapshots, cap checks, budget reservation, outbox insert and agent audit commit together.
  Budget stops appear as `message_limit`; audit rows contain no free text. Delivery repeats the AI gate, pause,
  consent, notifications, active-profile and current-need checks, cancelling obsolete reminders.
- Default L0 installations retain existing system reminders. Opted-in levels (or a custom enabled Follow-up step)
  suppress the legacy reminder loop so it cannot bypass the step policy or approvals inbox. The existing agent
  scheduler discovers reminders on its periodic subject scans; no additional scheduler or runtime is introduced.
- Scripted tests cover proposals, edited scope rejection, review-window expiry, Auto, spacing, lifetime caps after
  outbox cleanup, shared introduction/follow-up budgets and expiry, rollback and migration preservation. Telegram
  fixtures cover both languages, delivered templates/buttons, candidate yes/decline, closed gates, pause and stale need.
  No real model was used.

## Step 6.1: what shipped
- The pure core outbound screen rejects contacts (including Persian/Arabic digits), URLs, handles, score language,
  stored private values and rule names, and messages over 600 Unicode characters. Matching normalizes case,
  Persian letter/digit variants and separators; private-value matching is intentionally conservative.
- `send_message(match_id, profile_id, text)` is policy-checked SendMessage, restricted to linked, consenting active
  participants of the target's existing nonterminal introduction. Hidden/held matches cannot receive follow-ups.
  L1/L2 propose, L3 schedules a review window, and a custom Auto step queues immediately. Later steps remain Off.
- Admission rechecks current registry sensitivity (including nested fields), NEVER_SHARED values, identifiers,
  stored score values and versioned rule names. Escaped `render::agent_message_text` uses the existing outbox;
  delivery screens again against current data and cancels stale/unsafe content. No Interest or lifecycle change occurs.
- The shared rolling daily message budget, outbox insertion and text-free agent audit commit atomically. Existing
  run traces, edited approvals, review windows, veto, pause and AI gate apply. English/Persian settings help and
  run-log labels ship. No new inbound interpretation, missing-info tool or real-model run is included.

## Step 6.2: what shipped
- `request_info {field,text}` asks the run's linked target for one missing, registered nonsensitive field. Redacted profile observations list requestable fields. Existing values, contacts, identifiers, NEVER_SHARED fields and record fields cannot be requested.
- Questions use the step-6.1 screen, escaped message renderer, shared daily budget and outbox. The existing policy, approvals, edited arguments, review windows, veto, pause and AI gate apply. English/Persian settings help and tool labels are included.
- One question is reserved in the existing Telegram flow state atomically with delivery and budget reservation. Delivery rechecks the link, field and screen; cancellation or a stale request prevents delivery. Only a delivered question consumes an answer.
- Answers use the existing typed bot parser and field validation, producing extraction suggestions transactionally with the incoming message and a text-free system audit. AcceptExtraction decides acceptance; accepted answers retain rank-zero provenance and existing undo. No model interprets replies and no Interest is set.
- Scripted agent tests cover policy proposals, review windows, edited approval and blocked requests. English/Persian conversations cover escaping and pending versus automatic acceptance; cancellation and shared-budget tests cover blocked delivery.

## Decisions worth knowing
- The phase-1 deterministic step still records actor `agent:0`; its `run_id` remains NULL in `mm_agent_actions`.
  On-demand runtime actions instead record their real `agent:<run_id>` actor.
- L4 is unreleased until phase 8: `save_policy` rejects it and the UI disables it. A step that is not built always
  counts as Off.
- The rules-only tier is gated by the AI gate too, because a closed gate stops all agent actions.
- `run_accept_all` in `ai/intake_commands.rs` is the manual, unthresholded path and is untouched.
- Fourteen steps are built: RunExtraction, AcceptExtraction, FindMatches, CreateMatch, AnalysePair, AddNote, Recommend,
  Approve, Reject, RuleOverride, SendIntroduction, SendFollowUp, SendMessage and RequestInfo.
  Read-only inventory/profile tools and finish still require an open AI gate and an unpaused policy.
- Runtime/model access and due execution are serialized. Veto uses an atomic status check and does not wait for
  model work. Budget reservations are separate from retained AI prompts.

## Verified here
- `cargo test -p matchmaking-core`: 198 passed, including the built-step policy matrix, invariant denials
  and actor-aware lifecycle checks.
- `cargo test --lib autonomy_tests` in `src-tauri`: 14 passed.
- Frontend: `tsc --noEmit`, `check:i18n`, `check:invoke` and `check:autonomy` pass.
- Scripted backend agent tests cover stored matches plus analyses at L2, proposals/edited approval/rejection at L1,
  extraction and explicit acceptance, redacted observations, rank-zero undo, invariant denials, gate/config changes,
  batch subjects, durable review windows and veto, invalid actions and configurable budget stops. A prompt regression test lives in
  `ai/tests.rs`. All 59 agent tests pass, including recommendation/review, notes, transition undo, and step 3.2
  approval/rejection, rule override, migration, proposal, template follow-up and screened-message checks.
- `cargo test --manifest-path frontend/src-tauri/Cargo.toml --lib`: 500 passed, 2 ignored; no real model was used.

## Still open
- Steps 6.3–8: inbound reply interpretation,
  outcomes and the full autonomy release remain unavailable.
- No run with a real model, and the UI was type-checked but not clicked through in a running app.
