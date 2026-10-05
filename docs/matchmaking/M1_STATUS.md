# M1 status: profiles and search (first slice)

## In `matchmaking-core` (tested: 11 tests pass)
- `FieldDef` gains `required`; `FieldKind::validate`, `FieldRegistry::validate_value`; unknown fields are rejected.
- `default_registry()`: 29 starter fields from the plan (personal info, family, religion, smoking, health, contact, free text);
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
- Children as sub-records (per-child gender, age, custody, living arrangement): needs a record-type field. Currently `has_children` and `children_count` only.
- Photos, partner-preference editor, questionnaires, import/export (CSV/JSON), saved searches, field editing UI (the `mm_save_field` command exists; no screen yet).
- Role-based visibility of sensitive fields (all fields are visible to the single local matchmaker for now).
- Unit tests for the Tauri commands (they need an `AppState` with a database).
