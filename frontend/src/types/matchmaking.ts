// Mirrors the Rust types in matchmaking-core and src-tauri/src/mm/commands.rs.

export type FieldKind =
  | 'bool'
  | 'number'
  | 'text'
  | { choice: string[] }
  | { multi_choice: string[] };

export interface FieldDef {
  key: string;
  label: string;
  kind: FieldKind;
  sensitive: boolean;
  required: boolean;
}

export type Value = boolean | number | string | string[];
export type Provenance = 'user' | 'matchmaker' | 'questionnaire' | 'ai_inferred';

export interface ProfileEntry {
  value: Value;
  source: Provenance;
}

export interface ProfileView {
  id: string;
  status: 'active' | 'deactivated';
  created_at: string;
  updated_at: string;
  fields: Record<string, ProfileEntry>;
  completeness: number | null;
  missing_required: string[];
  issues: { field: string; message: string }[];
}

export interface ProfileSummary {
  id: string;
  status: string;
  updated_at: string;
  name: string | null;
  age: number | null;
  city: string | null;
  completeness: number | null;
}

export interface SearchResultRow extends ProfileSummary {
  match_result: 'true' | 'unknown';
}

export type ConditionOp = 'eq' | 'ne' | 'lt' | 'le' | 'gt' | 'ge' | 'in' | 'between' | 'exists';

export interface Condition {
  field: string;
  op: ConditionOp;
  value?: Value | null;
  value2?: Value | null;
}

export interface UpdateResult {
  profile: ProfileView;
  rejected: string[];
}

export function kindName(k: FieldKind): 'bool' | 'number' | 'text' | 'choice' | 'multi_choice' {
  if (typeof k === 'string') return k;
  return 'choice' in k ? 'choice' : 'multi_choice';
}

export function kindOptions(k: FieldKind): string[] {
  if (typeof k === 'string') return [];
  return 'choice' in k ? k.choice : k.multi_choice;
}
