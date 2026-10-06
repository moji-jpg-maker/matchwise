# M4 status: matchmaker workflow (first slice)

Turns scores into a working process: every pair a matchmaker acts on becomes a **match record** that moves from "recommended" to an outcome, with every
decision on file.

## Lifecycle (`matchmaking-core/src/lifecycle.rs`, 46 core tests pass in total)
- States: identified, recommended, reviewed, approved, introduction proposed, both interested, contact exchanged, conversation, meeting, feedback; finished states:
  closed, rejected, declined (a person said no), stopped. The state machine decides what is allowed from each state; the UI offers exactly those moves
  (it does not repeat the rules). Backwards jumps are refused; a rejection can be reconsidered; finished matches are final.
- **Overrides.** Recommending or approving a pair the rules exclude requires a written override reason (3+ characters); it is stored on the match and in its history.
- **Responses.** Recording whether each person is interested moves the match itself: two yes answers advance it, one no ends it (history marks these as automatic).
  "Both interested" cannot be set by hand.
- **Outcomes** (interested, not interested, first conversation, first meeting, continued, stopped, relationship formed, married, unknown) can be recorded once contact has begun
  or when a person declined, and closing a match requires one. They are the labels later learning will use (M7).

## Persistence and commands (migration `20261008000000_add_matchmaking_matches.sql`)
- One record per **unordered pair** (the pair is stored in a fixed order, so A-B and B-A are the same match). Deleting either profile deletes the match and everything under it
  (snapshots, notes, history): verified by the end-to-end run.
- **Score snapshots**: the pair is scored when tracking starts and that scorecard is stored as the record of *why it was recommended*, with the rule-set version that produced it.
  Re-scoring (after a profile changes, or with the newest rule-set version) adds a snapshot; none are overwritten.
- **Matchmaker controls** from the plan: approve, reject, override, per-match **dimension weights** (re-scored, rule set untouched), notes, request additional information (a visible
  "waiting for information" hold with the request text), hide from lists, change status.
- **History** per match with actor and from/to status. Free-text reasons live there; the global audit log records only the action and the status move, never the text
  (checked: a search of the audit log for note/override/request text finds nothing).
- Commands: `mm_create_match` (idempotent), `mm_find_match_record`, `mm_list_matches`, `mm_match_counts`, `mm_get_match`, `mm_transition_match`, `mm_set_response`,
  `mm_record_outcome`, `mm_set_match_hold`, `mm_add_match_note`, `mm_set_match_hidden`, `mm_rescore_match`. The real command code ran an end-to-end workflow against in-memory SQLite
  (create, override, pipeline, automatic steps, decline, notes, holds, hide, weights, filters, cascade delete).

## UI (`tsc` passes; JSON checked against the types; label/timeline helpers tested)
- **/matches**: pipeline tabs with counts, list with score, "waiting for info" and "rules exclude" flags.
- **/match** has two modes. Comparing two people: score card plus "Track this pair" (or "Open match record" if one exists). A tracked match: stored score card and the workflow panel
  (stepper, allowed actions with an inline form for reason / override / outcome, responses, outcome, information request, re-score, weights, notes, history).
- Sidebar link "Matches".

## Known limits
- No notifications or Telegram yet (M5), so "introduction proposed" and the responses are recorded by the matchmaker by hand.
- No role-based permissions: every action is attributed to "matchmaker". Multi-user attribution arrives with the permissions work.
- The match is scored in a fixed pair order (alphabetical by profile id); the A/B labels in directional rules follow that order, as the match view shows names.
- Outcome data is captured but nothing learns from it yet, and there is no export of the decision data set (planned for the learning milestone).
- No automated tests for the Tauri command wrappers or the React components; ESLint was not run.
