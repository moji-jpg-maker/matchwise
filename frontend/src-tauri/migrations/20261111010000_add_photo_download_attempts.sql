-- Matchwise M11.1 phase 11: how many times the Telegram downloader has tried a photo.
--
-- A photo sent to the bot is written `pending` with only its Telegram file id. The worker fills in the bytes.
-- A failure that looks transient (a timeout, a 429, a truncated body) is tried again on the next round; after
-- this many attempts the row is marked `failed` so the UI can say so and the worker stops retrying forever.

ALTER TABLE mm_profile_photos ADD COLUMN download_attempts INTEGER NOT NULL DEFAULT 0;
