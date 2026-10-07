# M5 status: Telegram adapter and notifications (first slice)

Decisions taken with the project owner: each matchmaker creates their **own bot** with BotFather and pastes its token into the app (kept in the OS credential store,
or in `MATCHWISE_TELEGRAM_TOKEN` on WSL/headless machines, never in the database); delivery is **long polling from the desktop app**, so the bot answers only while
Matchwise is running. A relay server is a later phase.

## The privacy boundary (also shown on the Telegram page and in the bot's notice)
Profiles, scores and rules stay in the local encrypted database. Everything sent to or received from Telegram, including introduction cards and anything a candidate types,
passes through Telegram's servers. The bot therefore:
- serves **private chats only**, and an unlinked chat can do nothing except redeem an invitation code;
- needs an **explicit "I agree" to a versioned privacy notice** before anything else works (declining unlinks the chat);
- never shows stored **sensitive** values back to a person (it only says how many are saved), never shows scores, rules, holds or internal states, and never shares
  **contact details** (phone, Telegram username, full name and date of birth cannot appear on an introduction card whatever the settings say; the matchmaker exchanges contacts);
- shows a person **only pairs the matchmaker has actually proposed to them**, and only the facts ticked in the Telegram settings plus the other person's first name;
- **re-checks every button press** against the database (forged or stale buttons, other people's matches, closed introductions);
- tells the other side of a decline **neutrally**, never who said no.

## Pieces
- `matchmaking-core`: `sharing.rs` (introduction card from the shared fields, never-shared list, first-name rule) and `describe_preference` (50 core tests in total).
- `telegram/api.rs`: `TelegramApi` trait (the bot logic never touches the network) and an HTTP implementation that maps Telegram errors (401, 403, 429 with retry-after, 400, network) and
  **redacts the token from every error text**.
- `telegram/repo.rs`, migration `20261009000000_add_matchmaking_telegram.sql`: links (one chat per profile), **hashed single-use invitation codes** (10 characters, 48 hours, a new invitation
  revokes the old one), failed-attempt limiting (5 per chat per hour), conversation state, the **outbox** (rendered text kept so you can see what was sent), the candidate conversation log, and data
  requests. Deleting a profile deletes all of it (tested).
- `telegram/engine.rs`: `/start <code>`, consent, `/help`, `/profile`, `/edit` (guided questions for missing required fields, choice/yes-no/multi-choice buttons, skip; answers are saved as
  "entered by the candidate" and **cannot overwrite what the matchmaker entered**), `/preferences` (view, add with must-have / deal-breaker / preferred / nice-to-have, remove), `/matches`, `/match N`,
  `/status`, `/settings`, `/stop` / `/resume`, `/unlink`, `/forget` (asks the matchmaker to delete their data, shown in the app), `/cancel`, and free text (goes to the matchmaker).
- `telegram/notify.rs`: introduction, both-interested, neutral "not going ahead", withdrawal, follow-up reminders (an unanswered introduction after 3 days, at most 2; an incomplete profile 3 days
  after linking, at most 3; none while a match is on hold) and information requests. People who are not linked, have not agreed, or turned notifications off are skipped.
- `telegram/worker.rs`: long-polling loop with the update offset persisted, outbox delivery with retry/backoff (blocked bot gives up and turns notifications off; rate limit waits; a person who
  withdrew since queueing is never messaged), reminders every ~10 minutes, tidy-up of delivered messages after 30 days.
- The app and the bot share one code path for answering an introduction (`set_response_inner`), so both trigger the same status changes and notifications.

## Matchmaker screens (`tsc` passes; Rust JSON checked against the TypeScript types)
- **/telegram**: bot token (never displayed), start/stop and status, which facts an introduction may show (with a never-shared notice), unread messages and deletion requests.
- Profile page: **Telegram panel** (create a one-time invitation and link, link status, the conversation, write to the person, remind them to complete the profile, "show what was sent", unlink).
- Match workflow: proposing an introduction **shows the exact message each person would receive and who cannot be reached**, before you confirm; "Request more information" can ask a chosen person
  on Telegram (their next reply is filed against the match and puts a badge on the profile).

## Verified here
21 conversation-level tests run real updates through the engine against an in-memory database (linking, consent, guessing limits, expiry and revocation, no leakage of sensitive values or contact
details, editing, preferences, introductions, forged buttons, declines, reminders, delivery retries, unlink/forget, cascade deletion) and 2 more cover the HTTP layer against a local stand-in server
(request shape, error mapping, malformed updates, token never in an error). The matchmaker commands were exercised end to end through the real engine. All of this passes in my sandbox; none of it
touched the real Telegram service, which my sandbox cannot reach.

## Not verified / limits
- **No real Telegram traffic has happened.** The first live run (token, `/start` link, an introduction round trip) is yours; expect small surprises (message formatting, button behavior on phones).
- The bot works only while the app runs; messages sent while it is off are delivered by Telegram later only for updates (Telegram keeps them 24 hours), and queued outbox messages wait for the next start.
- No quiet hours or time zones; reminders can arrive at any hour.
- Editing repeating records (children) and free-text preferences is not offered in the bot; the matchmaker does those.
- A person can switch Telegram off with `/stop`, but there is no per-kind notification setting yet.
- Several bots/matchmakers, roles and permissions, and attribution of who in the team did what, come with the multi-user milestone.
- The matchmaker is not notified outside the app when a candidate writes: the Telegram page and profile show unread messages; there is no desktop notification yet.
