// Mirrors the Rust types in matchmaking-core and src-tauri/src/mm/commands.rs.

export type FieldKind =
  | 'bool'
  | 'number'
  | 'text'
  | { choice: string[] }
  | { multi_choice: string[] }
  | { records: FieldDef[] };

export interface FieldDef {
  key: string;
  label: string;
  kind: FieldKind;
  sensitive: boolean;
  required: boolean;
}

export interface RecordValue {
  [key: string]: Value;
}
export type Value = boolean | number | string | string[] | RecordValue[];
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
  /** "<owner's field> + offset"; only valid in partner preferences. */
  value_rel?: RelativeValue | null;
  value2_rel?: RelativeValue | null;
}

export interface RelativeValue {
  field: string;
  offset: number;
}

export type Strength = 'required' | 'deal_breaker' | 'preferred' | 'flexible';

export interface Preference {
  id: string;
  condition: Condition;
  strength: Strength;
  importance: number;
  note?: string | null;
}

export type Direction = 'pair' | 'a_to_b' | 'b_to_a';
export type Tri = 'true' | 'false' | 'unknown';

export interface RuleResult {
  rule_id: string;
  description: string;
  kind: 'hard' | 'soft';
  group: string | null;
  priority: number;
  direction: Direction;
  /** false = the rule's "applies when" condition was false, so it was skipped */
  applicable: boolean;
  result: Tri;
  weight: number;
}

export interface MatchOutcome {
  rule_set: string;
  rule_set_version: number;
  eligible: boolean;
  needs_info: string[];
  soft_score: number | null;
  unknown_soft: string[];
  coverage: number | null;
  meets_threshold: boolean | null;
  results: RuleResult[];
}

export interface MatchEvaluation {
  rules: MatchOutcome;
  a_preferences: MatchOutcome;
  b_preferences: MatchOutcome;
  eligible: boolean;
  needs_info: boolean;
  score: number | null;
  coverage: number | null;
  meets_threshold: boolean | null;
}

export interface MatchCandidate extends ProfileSummary {
  eligible: boolean;
  needs_info: boolean;
  /** confidence-adjusted ranking score */
  score: number | null;
  overall: number | null;
  confidence: number | null;
  blocking: string[];
  strengths: string[];
  concerns: string[];
  unknown_count: number;
}

// ---- scorecard
export type DimStatus = 'strong' | 'mixed' | 'concern' | 'clear' | 'unknown' | 'not_applicable' | 'not_assessed';

export interface DimensionDef {
  key: string;
  label: string;
  description: string;
}

export interface DimensionScore extends DimensionDef {
  score: number | null;
  coverage: number | null;
  status: DimStatus;
  weight: number;
  checks: number;
}

export interface MissingField {
  who: 'a' | 'b';
  field: string;
}

export interface Finding {
  source: 'rule' | 'preference_a' | 'preference_b';
  id: string;
  description: string;
  dimension: string;
  direction: Direction;
  kind: 'hard' | 'soft';
  result: Tri;
  weight: number;
  priority: number;
  missing: MissingField[];
}

export interface ScoreCard {
  rule_set: string;
  rule_set_version: number;
  eligible: boolean;
  hard_constraints: {
    status: 'pass' | 'fail' | 'needs_info';
    passed: number;
    violations: Finding[];
    undecided: Finding[];
  };
  dimensions: DimensionScore[];
  overall: number | null;
  confidence: number | null;
  ranking_score: number | null;
  meets_threshold: boolean | null;
  strengths: Finding[];
  concerns: Finding[];
  unknowns: Finding[];
}

export interface MatchView {
  scorecard: ScoreCard;
  evaluation: MatchEvaluation;
}

// ---- rule sets (mirror matchmaking-core serde output) ----
export type Side = 'a' | 'b';
export type CmpOp = 'eq' | 'ne' | 'lt' | 'le' | 'gt' | 'ge' | 'in';

export type Operand =
  | { field: { of: Side; key: string } }
  | { lit: { value: Value } }
  | { offset: { base: Operand; by: number } };

export type Expr =
  | { op: 'and'; args: Expr[] }
  | { op: 'or'; args: Expr[] }
  | { op: 'not'; arg: Expr }
  | { op: 'cmp'; left: Operand; cmp: CmpOp; right: Operand }
  | { op: 'between'; value: Operand; lo: Operand; hi: Operand }
  | { op: 'exists'; value: Operand }
  | { op: 'if'; when: Expr; then: Expr };

export interface Rule {
  id: string;
  description: string;
  kind: 'hard' | 'soft';
  weight: number;
  expr: Expr;
  when?: Expr | null;
  group?: string | null;
  priority: number;
  scope: 'pair' | 'directional';
  enabled: boolean;
}

export interface RuleSet {
  name: string;
  version: number;
  rules: Rule[];
  group_weights: Record<string, number>;
  min_score: number | null;
  prior_score?: number | null;
}

export interface RuleIssue {
  rule_id: string | null;
  message: string;
}

export interface RuleSetSummary {
  id: string;
  name: string;
  description: string;
  current_version: number;
  archived: boolean;
  rule_count: number;
  updated_at: string;
}

export interface RuleSetView {
  id: string;
  name: string;
  description: string;
  archived: boolean;
  current_version: number;
  version: number;
  definition: RuleSet;
  versions: { version: number; created_at: string; note: string | null }[];
}

export interface UpdateResult {
  profile: ProfileView;
  rejected: string[];
}

export function kindName(k: FieldKind): 'bool' | 'number' | 'text' | 'choice' | 'multi_choice' | 'records' {
  if (typeof k === 'string') return k;
  if ('choice' in k) return 'choice';
  if ('multi_choice' in k) return 'multi_choice';
  return 'records';
}

export function kindOptions(k: FieldKind): string[] {
  if (typeof k === 'string') return [];
  if ('choice' in k) return k.choice;
  if ('multi_choice' in k) return k.multi_choice;
  return [];
}

export function recordFields(k: FieldKind): FieldDef[] {
  return typeof k !== 'string' && 'records' in k ? k.records : [];
}
