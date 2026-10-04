# Matchwise Privacy Notice (DRAFT -- needs review before any real use)

This draft describes how the code currently behaves. It is not legal advice; have it reviewed for your jurisdiction
(for example GDPR if you handle EU residents' data) before collecting real profiles.

- **Storage.** Profile and application data is stored locally on the device running Matchwise.
  (Database encryption at rest is planned but not yet implemented.)
- **Telemetry.** Matchwise sends no analytics. The inherited analytics code has been disabled and is scheduled for removal.
- **AI providers.** If you configure an LLM provider, text you submit to it is sent to that provider under its own terms.
  A local provider (for example Ollama) keeps data on your machine.
- **Messaging.** Telegram integration is planned. Anything sent through Telegram passes through Telegram's infrastructure.
- **Sensitive data.** Profiles can include religion, health, family, and financial information. Collect it only with consent and a clear purpose.
- **Deletion.** Data deletion, consent management, and audit logging are planned features.
