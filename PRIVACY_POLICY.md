# Matchwise Privacy Notice (DRAFT -- needs review before any real use)

This draft describes how the code currently behaves. It is not legal advice; have it reviewed for your jurisdiction
(for example GDPR if you handle EU residents' data) before collecting real profiles.

- **Storage.** Profile and application data is stored locally on the device running Matchwise.
  The database is encrypted at rest (SQLCipher); its key is kept in the operating system's credential store, or supplied through the `MATCHWISE_DB_KEY` environment variable. Backups and exports are not yet encrypted separately.
- **Telemetry.** Matchwise sends no analytics. The inherited analytics code has been disabled and is scheduled for removal.
- **AI providers.** If you configure an LLM provider, text you submit to it is sent to that provider under its own terms.
  A local provider (for example Ollama) keeps data on your machine.
- **Messaging.** If you enable the Telegram bot, anything sent to or from it passes through Telegram's infrastructure. Candidates must accept a short privacy notice before the bot does anything,
  can stop messages (/stop), unlink (/unlink) or ask for deletion (/forget), and the bot never shows stored sensitive details, scores or contact information. An introduction shows the other person's
  first name and only the facts you chose to share. The bot token is kept in the operating system's credential store.
- **Sensitive data.** Profiles can include religion, health, family, and financial information. Collect it only with consent and a clear purpose.
- **Deletion.** Data deletion, consent management, and audit logging are planned features.
