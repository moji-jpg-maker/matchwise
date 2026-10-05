# M1 status: profiles, search, children and partner preferences

## In `matchmaking-core` (tested: 11 tests pass)
- `FieldDef` gains `required`; `FieldKind::validate`, `FieldRegistry::validate_value`; unknown fields are rejected.
- `default_registry()`: 30 starter fields from the plan (personal info, family, religion, smoking, health, contact, free text);
  religion, health, finances, weight, name, date of birth, phone and Telegram are flagged `sensitive`.
- `Profile::validate`, `completeness` (share of required fields filled) and `missing_required`.
- `search`: a form-style `Condition` list (`eq ne lt le gt ge in between exists`) becomes the same `Expr` tree the rule engine uses.
  Strict mode returns definite matches; lenient mode also returns profiles with missing data, flagged `unknown`.
  Matching is case-insensitive for text; `in` works for single-choice fields and for multi-choice fields.

## In the app (written, **not compiled in the app by me**: run `cargo check`)
- Migration `20261005000000_add_matchmaking_profiles.sql`: `mm_field_definitions`, `mm_profiles` (JSON data, status, `org_id`), `mm_audit_log`.
  The audit log records which fields changed, never their values. The repository SQL was run against an in-memory SQLite database
  in a scratch crate: seeding, no re-seed on second load, insert/get/update, deactivate, audit, delete all behave.
- `src-tauri/src/mm/`: nine commands (`mm_list_fields`, `mm_save_field`, `mm_create_profile`, `mm_update_profile_fields`, `mm_get_profile`,
  `mm_list_profiles`, `mm_search_profiles`, `mm_set_profile_active`, `mm_delete_profile`), registered in `lib.rs`.
  Writes enforce provenance: an AI-inferred value cannot overwrite or clear human-entered data; refused keys are returned as `rejected`.
  List and search results only expose name, age, city and completeness.
- Frontend (type-checks with `tsc`): `/profiles` (search form + results, strict/lenient toggle, deactivated filter),
  `/profile?id=` (editor generated from the field registry, provenance labels, completeness, deactivate, delete with confirmation),
  sidebar links. `src/types/matchmaking.ts` mirrors the Rust types.
  A full `next build` could not be run in my sandbox (it cannot reach Google Fonts); run it on your machine.

## Not done yet in M1
- Photos, questionnaires, import/export (CSV/JSON), saved searches, field editing UI (the `mm_save_field` command exists; no screen yet).
- Role-based visibility of sensitive fields (all fields are visible to the single local matchmaker for now).
- Unit tests for the Tauri commands (they need an `AppState` with a database).

## M1b: children records and partner preferences
In `matchmaking-core` (21 tests pass in total):
- **Records fields.** `FieldKind::Records(sub-fields)` and `Value::Records`; validation checks every sub-value and rejects unknown sub-fields.
  `children` (gender, age, custody, living arrangement, other circumstances) is a default sensitive field. `has_children` / `children_count` stay as separate flat fields
  so rules and searches can use them; records fields themselves cannot be searched or used in preferences yet.
- **Order-preserving field registry.** The old registry sorted fields alphabetically; it now keeps insertion order (curated default order, matchmaker-added fields after).
- **Partner preferences.** `Preference` = a condition on the partner's profile + `Strength` (`required`, `deal_breaker`, `preferred`, `flexible`) + importance 1-5.
  Preferences compile to ordinary rules and run through the same three-valued engine: a failed must-have or a matched deal-breaker excludes the pair; a hard preference
  that cannot be decided because data is missing is reported in `needs_info`, not guessed; soft preferences produce a weighted score (importance, half for flexible),
  and unknown ones are left out of the score and listed. `evaluate_mutual` checks both directions.
- **Relative values.** A bound can be "own field + offset", which covers the plan's example (partner age between own age + 0 and own age + 8). Only valid in preferences.
- Validation (`validate_preference`): field must exist, operator must suit the field type, options must be allowed, importance 1-5, relative values only on numeric fields.

In the app (repository SQL verified against in-memory SQLite; Tauri commands not compiled by me):
- Migration `20261006000000_add_matchmaking_preferences.sql`: `mm_meta` and `mm_preferences` (`ON DELETE CASCADE`, so deleting a profile deletes its preferences).
- Default fields are now seeded by version (`fields_seed_version` = 2): existing databases gain `children` and the curated order without overwriting any field edited by a matchmaker.
  Verified: fresh install, and upgrade from a v1 database that had a custom field and an edited label.
- Commands: `mm_get_preferences`, `mm_save_preferences` (validates everything, saves nothing if any item is invalid; audit log records only the count),
  `mm_evaluate_preferences` (one-direction dry run for a chosen candidate).
- UI (`tsc` passes; helper logic checked with an ad-hoc node script, not committed because the frontend has no test runner): children cards in the profile editor,
  a "Partner preferences" panel (strength, importance, relative-to-own values, notes) and a "Check against a candidate" dry run showing each preference as ok/violated/met/not met/unknown.

Still open: photos, questionnaires, import/export, saved searches, field-editing screen, role-based visibility, per-child rules (for example "youngest child older than 5"), command-level tests.
