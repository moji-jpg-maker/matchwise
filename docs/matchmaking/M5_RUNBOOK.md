# M5 runbook: first live Telegram round trip

The open item in `M5_STATUS.md`: no real Telegram traffic has happened. Everything below was written by reading the
code, not by running it against Telegram. It takes roughly 45-60 minutes with two phones in hand.

Fill the **Result** column as you go (`pass`, `fail`, `n/a` plus one line of what you saw). Anything that fails gets
matched back to `M5_STATUS.md` "Not verified / limits" or turned into a code fix.

## Run header

| Field | Value |
|---|---|
| Date | |
| Build (git rev) | |
| OS / version | |
| App mode (`tauri:dev` or installer) | |
| Bot username | `@` |
| Token source (credential store / env) | |
| Phone OS + Telegram version (A) | |
| Phone OS + Telegram version (B) | |
| App language (en / fa) | |

## Constants that matter (from the code)

| Thing | Value | Where |
|---|---|---|
| Token shape | `digits:<30-80 alnum/_/->`, id at most 20 digits | `telegram/commands.rs` `valid_token_format` |
| Token storage | OS credential store, service `com.matchwise.desktop`, account `telegram-bot-token` | `telegram/commands.rs` |
| Env override | `MATCHWISE_TELEGRAM_TOKEN` wins; `tg_save_token` then refuses to store | `telegram/commands.rs` |
| Invitation lifetime | 48 hours, single use, a new one revokes the old | `telegram/repo.rs` `INVITE_HOURS` |
| Invitation code | 10 characters, only the hash is kept | `telegram/repo.rs` |
| Failed-code limit | 5 per chat per hour | `telegram/repo.rs` `MAX_FAILED_CODES` |
| Consent version | 1 | `telegram/repo.rs` `CONSENT_VERSION` |
| Long-poll timeout | 25 s (client timeout 45 s) | `telegram/worker.rs`, `telegram/api.rs` |
| Outbox tick | ~2 s | `telegram/worker.rs` |
| Reminders | ~10 min tick, unanswered intro after 3 days (max 2), incomplete profile after 3 days (max 3) | `telegram/notify.rs` |

## Setup (do once)

Use a machine with real outbound network. This sandbox cannot resolve `api.telegram.org`; the app would report that
as "Telegram did not accept the token".

| # | Check | Result |
|---|---|---|
| S1 | `@BotFather` → `/newbot` created, username recorded above | |
| S2 | `curl -s "https://api.telegram.org/bot<TOKEN>/getMe"` returns `"ok":true` | |
| S3 | Groups disabled for the bot (`/setjoingroups` → Disable), optional | |
| S4 | App started (`pnpm run tauri:dev`); on Linux without gnome-keyring `MATCHWISE_DB_KEY` set first | |
| S5 | `MATCHWISE_TELEGRAM_TOKEN` **not** exported (unless testing the env path) | |
| S6 | Two Telegram accounts available on separate devices | |
| S7 | Two profiles created, both `active` | |

## A. Token and runtime (app)

| # | Check | Result |
|---|---|---|
| A1 | Telegram page shows setup steps and no configured token | |
| A2 | Pasting a malformed token is rejected with the "does not look like a bot token" message, no network call | |
| A3 | Valid token saves; UI reports `token in credential store` | |
| A4 | Bad-bot token (well-formed, wrong secret) is rejected with "Telegram did not accept the token" | |
| A5 | **Start** sets `running`, shows `@username`, `processed updates` and `messages sent` counters | |
| A6 | `last_error` stays empty while idle | |
| A7 | Status polls every 5 s without errors while the page is open | |
| A8 | **Stop** clears `running`; **Start** again resumes | |
| A9 | Restarting the app leaves the bot **stopped** — document this as expected | |
| A10 | **Remove token** clears the credential store and the stored bot username | |

## B. Linking and consent (phone A)

| # | Check | Result |
|---|---|---|
| B1 | Profile → Telegram panel → Create invitation shows a `t.me/<bot>?start=<code>` link and the 10-char code | |
| B2 | Link appears only after the bot username is known (save + start first); otherwise "no bot" with the code only | |
| B3 | Opening the link shows the consent card, first name filled, **I agree / No thanks** buttons | |
| B4 | Card language follows the phone's Telegram locale (`en` or `fa`); `/language` switches it before agreeing | |
| B5 | Any command other than `/start <code>`, `/help`, `/language`, `/stop`, `/unlink` before linking → "invitation" text | |
| B6 | Wrong/expired code → the same generic invalid-code text (does not reveal whether the code exists) | |
| B7 | 6th wrong code within an hour → "Too many attempts" | |
| B8 | **I agree** → welcome message with the first name | |
| B9 | Pressing the same consent button again → "Already answered." toast | |
| B10 | Second profile linked from a second chat while the first is linked → "already linked to a Telegram account" | |
| B11 | **No thanks** on a throwaway link → unlinked, panel returns to "not linked", new invitation possible | |
| B12 | Panel shows linked badge, `@username`, linked date, and the conversation | |

## C. Bot commands (phone A, English)

| # | Check | Result |
|---|---|---|
| C1 | `/help` renders bold heading, bullets, one command per line, no raw HTML | |
| C2 | `/profile` lists saved fields; **sensitive values are counted, never shown** | |
| C3 | `/edit` runs a guided flow (choice / yes-no / multi-choice buttons, Skip); `/cancel` mid-flow aborts | |
| C4 | `/edit` cannot overwrite a matchmaker-entered value | |
| C5 | `/preferences` lists, adds with must-have, removes one | |
| C6 | `/matches` lists only this person's proposed pairs | |
| C7 | `/status` shows a friendly state, no scores or internal reasons | |
| C8 | `/settings` shows notification state and the four buttons | |
| C9 | Notifications off then on via `/settings` | |
| C10 | `/stop` then `/resume` both confirm | |
| C11 | `/questionnaires` produces a questionnaire if one is assigned (else the "nothing" reply) | |
| C12 | Free text reaches the profile's conversation panel as an inbound message | |
| C13 | Unread badge on the profile clears after opening the conversation (`tg_mark_read`) | |
| C14 | Unknown command → "I did not understand that. Send /help" | |
| C15 | Message over 2000 characters → "too long" reply | |
| C16 | Non-text message (sticker/photo) → "I can only read text messages" | |

## D. Persian pass (phone with `fa` locale, or `/language`)

| # | Check | Result |
|---|---|---|
| D1 | Consent card reads right-to-left, no broken punctuation or reversed digits | |
| D2 | Buttons fit one row in Persian; no truncation | |
| D3 | `/help`, `/profile`, `/settings` render RTL correctly | |
| D4 | Persian digits used in lists and counts | |
| D5 | First name interpolated correctly, no stray punctuation after a Persian word | |

## E. Introduction round trip (both phones)

| # | Check | Result |
|---|---|---|
| E1 | Match created `recommended`; moved to `reviewed` → `approved` → `introduction_proposed` | |
| E2 | Before confirming, the workflow shows the exact message per side and who cannot be reached | |
| E3 | Preview matches the message actually delivered | |
| E4 | Card arrives within ~2 s while the app is running | |
| E5 | Card shows only the chosen fields plus first name; **no phone, username, full name, date of birth** | |
| E6 | **I'm interested** on A → "I will let you know when … has answered" | |
| E7 | **I'm interested** on B → both get the "both interested" message | |
| E8 | Fresh pair, **Not interested** on one side → the other gets the neutral "will not go ahead" text, never who declined | |
| E9 | Pressing an old intro button after the status moved on → "no longer open" | |
| E10 | Pressing a button belonging to someone else's match → generic "invalid" toast, no information leaks | |
| E11 | Repeated press of the same answer → "Already recorded." | |
| E12 | App shows both responses and the new status without a manual refresh | |

### Expected text, verbatim

Set the sample profile **B** to these values before running E4-E12, so the delivered card can be compared word for
word instead of judged by eye. Everything below is copied from `telegram/render.rs` and
`matchmaking-core/src/sharing.rs`; the wording does not change unless the matchmaker changes the settings.

| Field | Value to enter |
|---|---|
| `full_name` | `Sara Ahmadi` |
| `age` | `31` |
| `city` | `Tehran` |
| `education` | `master` |
| `occupation` | `Architect` |
| `marital_status` | `never_married` |
| `has_children` | `false` |
| `height_cm` | `165` |
| `smoking` | `never` |
| `children_count` | leave empty |

Sharing settings: default field list, first names **on**. Three notes before comparing:

- Cards are built only from the fields the matchmaker ticked, in that list's order, and a field with no value is
  skipped. `children_count` is empty above, so the card has eight lines, not nine.
- Option values are shown as their stored key with underscores turned into spaces (`never_married` → `never
  married`, `master` stays `master`). Digits are converted to Persian digits in Persian, nothing else is translated.
- A name is never translated: `first_name_opt` takes the first word of `full_name` verbatim. So in the Persian card
  and replies below, the name stays `Sara` in Latin letters, wrapped in invisible isolate marks so it sits correctly
  in the RTL sentence. Expect to *see* `Sara`, not `سارا`.

**Card delivered to phone A (English), exactly:**

```
Introduction
Your matchmaker would like to introduce you to Sara.

• Age: 31
• Current city: Tehran
• Education level: master
• Occupation: Architect
• Marriage history: never married
• Has children: no
• Height (cm): 165
• Smoking: never

Would you like to be introduced? Your answer is shared with your matchmaker only; the other person is told only if you are both interested.
```

Buttons underneath, one row: `I'm interested` · `Not interested`.

**The same card in Persian (phone with `fa`), exactly** — note Persian labels, Persian digits, and `Sara` /
`master` / `Tehran` / `never` left in Latin inside invisible isolate marks:

```
معرفی
مشاور شما می‌خواهد شما را به Sara معرفی کند.

• سن: ۳۱
• شهر محل سکونت: Tehran
• سطح تحصیلات: master
• شغل: Architect
• سابقه ازدواج: never married
• فرزند دارد: خیر
• قد (سانتی‌متر): ۱۶۵
• مصرف دخانیات: never

مایل به معرفی هستید؟ پاسخ شما فقط با مشاورتان در میان گذاشته می‌شود؛ فقط اگر هر دو علاقه‌مند باشید به طرف مقابل گفته می‌شود.
```

Buttons: `علاقه‌مندم` · `علاقه‌ای ندارم`. Heading renders bold; in a client that ignores the bold tag the first line is plain.

**Reply after pressing "I'm interested" (English):**

```
Thank you! I will let you know when Sara has answered.
```

Persian: `ممنون! وقتی Sara پاسخ داد به شما خبر می‌دهم.`

**When both sides are interested, each receives:**

```
Good news: you and Sara are both interested. Your matchmaker will be in touch to arrange the next step.
```

Persian: `خبر خوب: شما و Sara هر دو علاقه‌مند هستید. مشاورتان برای هماهنگی مرحله بعد با شما تماس می‌گیرد.`

**After a decline, the person who did not decline receives only:**

```
Thank you for your interest. Unfortunately this introduction will not go ahead. Your matchmaker will keep looking for someone who suits you.
```

Persian: `از علاقه شما سپاسگزاریم. متأسفانه این معرفی پیش نمی‌رود. مشاورتان به جست‌وجوی فرد مناسب برای شما ادامه می‌دهد.`
No name, no reason, nothing about who said no.

**The side that declined sees:**

```
Understood, thank you. I have noted that you are not interested. Nothing more will be shared.
```

Persian: `متوجه شدم، ممنون. ثبت کردم که علاقه‌ای ندارید. چیز دیگری به اشتراک گذاشته نمی‌شود.`

An untranslated name is correct behaviour, not a bug: stored values stay as entered. If you want a fully Persian
sample, set `full_name` to a Persian name instead and expect that exact string back.

**Toast / refusal wording to expect:**

| Situation | Text |
|---|---|
| Same answer pressed twice | `Already recorded.` / `قبلاً ثبت شده است.` |
| Introduction closed or withdrawn | `This introduction is no longer open.` / `این معرفی دیگر باز نیست.` |
| Button from a match that is not yours | generic `invalid` toast, no detail |
| First name sharing turned off | title becomes `Someone` / `یک نفر` instead of `Sara` |

**Preview detail:** the in-app preview (`tg_preview_introduction`) runs the same HTML through `render::plain`, so
it shows the identical wording *without* the bold tags. Compare the text, not the markup; the delivered message
is the one carrying `<b>`.

## F. Delivery, failures, cleanup

| # | Check | Result |
|---|---|---|
| F1 | Profile panel "what was sent" lists each outbox message with status, attempts and `last_error` | |
| F2 | `403` path: block the bot on B, queue a message → notifications turn off, no retry storm | |
| F3 | `409`/Conflict in `last_error` means a second poller on the same token (leftover instance or webhook) — confirm the cause | |
| F4 | Messages queued while the app was closed arrive after restart (within Telegram's 24 h window) | |
| F5 | Outbox retry/backoff visible for a transient failure | |
| F6 | `/unlink` unlinks; `/forget` raises a deletion request visible in the app inbox | |
| F7 | Deleting a profile removes its link, messages and outbox rows | |

## G. Sign-off

| Field | Value |
|---|---|
| Blocking defects found | |
| Cosmetic issues found | |
| Items to move from "Not verified / limits" in `M5_STATUS.md` | |
| New limits discovered | |
| Reviewer | |
| Date closed | |

## Known gotchas while testing

- The bot never auto-starts. After any app restart, press **Start** again or nothing answers.
- The token check mixes "bad token" and "no network" into one message; if the machine is offline, expect a misleading error.
- Set `MATCHWISE_TELEGRAM_TOKEN` only deliberately: it silently takes precedence over the credential store and blocks saving a new token.
- Creating a second invitation revokes the first, so a "dead" link during testing may simply have been replaced.
- A webhook set on the bot (by anything else using the same token) makes long polling fail; check `getWebhookInfo` if nothing arrives.
