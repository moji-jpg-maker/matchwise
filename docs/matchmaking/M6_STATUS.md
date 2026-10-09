# M6 status: AI profile understanding and pair analysis (first slice)

Decision taken with the project owner: **local first.** AI is off by default; the recommended setup is a model on this computer (Ollama), and a provider outside this computer is opt-in
behind a recorded consent, an API key in the credential store, HTTPS and redaction.

## Principles (enforced in code, covered by tests)
- **A model suggests; a person decides.** Nothing a model returns changes a profile, a preference, a score, an eligibility or a match status. Suggestions wait in a review list. Accepting stores
  the value with **AI-inferred provenance** (any person's entry outranks it, and the UI keeps flagging it); "Accept as verified" is a deliberate human override recorded as matchmaker-entered.
  A suggestion that differs from a value a person entered cannot be accepted as a plain accept.
- **Evidence or it is dropped.** Every suggested field or preference must quote words that really occur in the text the model was given; the value must also pass the same field and preference
  validation as typed input. Invented, mis-typed or unquoted items are discarded and the matchmaker is shown that they were, and why.
- **Pair analysis must cite the facts.** The model receives numbered facts (A1.., B1.., stated preferences P1.., rule results S1..) and every statement except questions must cite listed fact
  ids; statements citing nothing, or ids that do not exist, are removed. The analysis is stored, marked **out of date** when profiles, preferences, rules or weights change, and explicitly cannot
  change a score or status.
- **Minimal data.** Names become "Person A/B"; phone, Telegram username, full name and date of birth are never sent; free text is scrubbed of phone numbers, e-mail addresses, @handles, links,
  long numbers and the people's own name tokens; sensitive fields are left out unless allowed (local model: a setting, default on, because the data stays on the machine; cloud: only if the consent
  says so). Text inside the `<data>` tags is treated as untrusted (prompt-injection guard), and a model that obeys an injected instruction still cannot do more than produce a suggestion.
- **The gate runs before every call.** Provider off, no cloud consent (or an outdated notice), a missing key, or plain http to another machine means the model is never contacted. Only loopback
  addresses count as local; a LAN machine counts as outside this computer.
- **Transparency.** "Show exactly what will be sent" previews the real prompt. Every call is logged (size, provider, model, success); for cloud calls the exact redacted prompt is kept for 30 days.
  The key never appears in the UI, errors or logs, and the global audit log records the field name of an accepted suggestion, never the value or the quote. Deleting a profile removes its
  suggestions, analyses and logged prompts.
- One model request at a time (a local model should not be asked three things at once); an unreadable answer gets one retry, then an honest error.

## Pieces
- `matchmaking-core/src/ai.rs` (pure, 62 core tests in total): redaction, the text and facts a model may see, prompts, JSON extraction from chatty replies, and the validation described above.
- `telegram`-style layering: `ai/backend.rs` (Ollama, OpenAI-compatible with a fallback when `response_format` is unsupported, Anthropic; all against one trait), `ai/settings.rs` (config, consent,
  key storage, the gate), `ai/repo.rs` + migration `20261010000000_add_matchmaking_ai.sql`, `ai/service.rs`, `ai/commands.rs` (14 commands).
- UI: **/ai** (provider, model, address, local-or-cloud explanation, cloud notice and consent, API key, test connection, "what was sent" log), an **AI profile reading** panel on each profile
  (optional intake notes, preview of what will be sent, suggestions with the quoted evidence and confidence, contradictions, missing fields, questions to ask, discarded items) and an
  **AI assessment** panel on each tracked match (sections with fact tags you can hover, out-of-date notice, preview).

## Verified here
16 in-crate tests with a scripted model (gate and address rules, no identifiers or sensitive text reaching the model, suggestions-only, provenance and override rules, retry, supersede,
grounding, staleness, serialization of requests, deletion, audit contents) plus tests of the three HTTP backends against a local stand-in server (request shapes and headers, fallback, error
mapping, key never in an error). The commands ran end to end against a stand-in local model. The Rust JSON matches the TypeScript types.

## Not verified / limits
- **No real language model has been used.** Prompt quality, and how well a particular model follows the JSON format and copies evidence exactly, are untested; the strict checks mean a model that
  paraphrases its quotes will have good suggestions discarded (visible in the "discarded" list), which is the safe direction. Try a model and tell me what it gets wrong.
- Redaction is pattern based. It does not know the names of relatives, employers or places mentioned in free text. That is why cloud use is opt-in and previewable.
- Text in languages other than English works only as well as the chosen model does; the prompts are English.
- Pair analysis works on tracked matches only. It does not influence ranking; learned ranking from outcomes is the next milestone, evaluated against the deterministic baseline.
- No streaming, no embeddings, no automatic runs: every model call is a button press.
- The first live test (install Ollama, pull a model, point the app at it) is yours; the AI page has a Test connection button.
