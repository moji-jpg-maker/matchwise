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

// ---- match workflow (mirror matchmaking-core lifecycle + src-tauri mm/match_commands.rs)
export type MatchStatus =
  | 'identified'
  | 'recommended'
  | 'reviewed'
  | 'approved'
  | 'introduction_proposed'
  | 'both_interested'
  | 'contact_exchanged'
  | 'conversation'
  | 'meeting'
  | 'feedback'
  | 'closed'
  | 'rejected'
  | 'declined'
  | 'stopped';

export type Interest = 'unknown' | 'interested' | 'not_interested';

export type Outcome =
  | 'interested'
  | 'not_interested'
  | 'first_conversation'
  | 'first_meeting'
  | 'continued'
  | 'stopped'
  | 'relationship_formed'
  | 'married'
  | 'unknown';

export interface MatchNote {
  id: number;
  text: string;
  created_at: string;
}

export interface MatchEvent {
  at: string;
  actor: string;
  kind: string;
  from_status: string | null;
  to_status: string | null;
  detail: string | null;
}

export interface MatchDetail {
  id: string;
  profile_a: string;
  profile_b: string;
  name_a: string | null;
  name_b: string | null;
  source_profile: string | null;
  rule_set_id: string;
  rule_set_name: string | null;
  rule_set_version: number;
  status: MatchStatus;
  allowed_next: MatchStatus[];
  eligible: boolean;
  can_record_responses: boolean;
  can_record_outcome: boolean;
  reachable_a: boolean;
  reachable_b: boolean;
  hold_reason: string | null;
  a_response: Interest;
  b_response: Interest;
  outcome: Outcome | null;
  override_reason: string | null;
  weight_overrides: Record<string, number>;
  hidden: boolean;
  created_at: string;
  updated_at: string;
  scorecard: ScoreCard | null;
  scorecard_at: string | null;
  scorecard_trigger: string | null;
  snapshot_count: number;
  notes: MatchNote[];
  events: MatchEvent[];
}

export interface MatchSummary {
  id: string;
  name_a: string | null;
  name_b: string | null;
  status: MatchStatus;
  on_hold: boolean;
  hidden: boolean;
  eligible: boolean;
  score: number | null;
  outcome: Outcome | null;
  updated_at: string;
}

// ---- Telegram (mirror src-tauri/src/telegram/commands.rs)
export interface TelegramStatus {
  configured: boolean;
  token_source: string | null;
  running: boolean;
  bot_username: string | null;
  last_error: string | null;
  last_activity: string | null;
  processed_updates: number;
  sent_messages: number;
  consent_version: number;
  introduction_fields: string[];
  show_first_name: boolean;
  never_shared: string[];
  unread_messages: number;
  open_requests: number;
}

export interface TelegramLink {
  linked: boolean;
  consented: boolean;
  notifications_enabled: boolean;
  username: string | null;
  linked_at: string | null;
  open_invite: boolean;
  unread: number;
}

export interface TelegramInvite {
  code: string;
  link: string | null;
  expires_at: string;
}

export interface TelegramMessage {
  id: number;
  direction: 'in' | 'out';
  text: string;
  match_id: string | null;
  created_at: string;
  is_read: boolean;
}

export interface IntroPreviewSide {
  profile_id: string;
  name: string | null;
  reachable: boolean;
  message: string | null;
}

export interface IntroPreview {
  a: IntroPreviewSide;
  b: IntroPreviewSide;
}

export interface OutboxItem {
  kind: string;
  text: string;
  status: 'pending' | 'sent' | 'failed' | 'cancelled';
  attempts: number;
  created_at: string;
  sent_at: string | null;
  last_error: string | null;
}

export interface TelegramInbox {
  unread: { profile_id: string; name: string | null; unread: number }[];
  requests: { id: number; profile_id: string; name: string | null; kind: string; created_at: string }[];
}

// ---- AI (mirror src-tauri/src/ai)
export interface AiConfig {
  provider: 'none' | 'ollama' | 'openai_compatible' | 'anthropic';
  model: string;
  base_url: string;
  local_include_sensitive: boolean;
}

export interface AiSettings {
  config: AiConfig;
  is_cloud: boolean;
  has_key: boolean;
  key_source: string | null;
  consent: { version: number; at: string; include_sensitive: boolean } | null;
  consent_text: string;
  consent_version: number;
  default_urls: [string, string][];
  ready: boolean;
  not_ready_reason: string | null;
  mode: { is_cloud: boolean; include_sensitive: boolean } | null;
}

export interface AiSuggestion {
  id: number;
  profile_id: string;
  kind: 'field' | 'preference';
  field: string;
  payload: unknown;
  evidence: string;
  confidence: number;
  conflict: boolean;
  status: string;
  model: string;
  created_at: string;
}

export interface AiExtraction {
  run_id: number;
  model: string;
  is_cloud: boolean;
  created_at: string;
  suggestions: AiSuggestion[];
  contradictions: { description: string; evidence: string[] }[];
  missing: string[];
  questions: string[];
  dropped: string[];
}

export interface AiPromptPreview {
  system: string;
  user: string;
  is_cloud: boolean;
  include_sensitive: boolean;
}

export interface AiFact {
  id: string;
  source: string;
  text: string;
}

export interface AiClaim {
  text: string;
  refs: string[];
}

export interface AiPairView {
  run_id: number;
  model: string;
  is_cloud: boolean;
  created_at: string;
  stale: boolean;
  facts: AiFact[];
  analysis: {
    why_it_may_work: AiClaim[];
    potential_challenges: AiClaim[];
    important_differences: AiClaim[];
    questions_to_discuss: AiClaim[];
    missing_information: AiClaim[];
    overall_assessment: AiClaim | null;
    dropped: string[];
  };
}

export interface AiLogRow {
  at: string;
  kind: string;
  provider: string;
  model: string;
  is_cloud: boolean;
  include_sensitive: boolean;
  input_chars: number;
  output_chars: number;
  ok: boolean;
  error: string | null;
  prompt: string | null;
}
