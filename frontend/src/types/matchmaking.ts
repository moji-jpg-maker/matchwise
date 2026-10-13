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
  /** The scoring config this rule set reads its scorecards with, if it has been given one. */
  scoring_config_id: string | null;
  rule_count: number;
  updated_at: string;
}

export interface RuleSetView {
  id: string;
  name: string;
  description: string;
  archived: boolean;
  current_version: number;
  scoring_config_id: string | null;
  version: number;
  definition: RuleSet;
  versions: { version: number; created_at: string; note: string | null }[];
}

// ---- versioned scoring configs (mirror matchmaking-core/src/scoring_config.rs + src-tauri mm/scoring_config_commands.rs)

/** The three numbers that turn dimension scores into a verdict. */
export interface ScoringConfig {
  /** A dimension at or above this is `strong` (0-100). */
  strong_threshold: number;
  /** A dimension at or above this (but below strong) is `mixed` (0-100). */
  mixed_threshold: number;
  /** Neutral score (0-100) that thin evidence is pulled towards in the ranking score. */
  prior_score: number;
}

/** A config in a list, carrying the numbers in force so it can be read without opening it. */
export interface ScoringConfigSummary {
  id: string;
  name: string;
  description: string;
  current_version: number;
  archived: boolean;
  definition: ScoringConfig;
  /** How many rule sets currently score with this config. */
  used_by: number;
  updated_at: string;
}

export interface VersionInfo {
  version: number;
  created_at: string;
  note: string | null;
}

/** One config, and which version of it is being shown (which may be older than current). */
export interface ScoringConfigView {
  id: string;
  name: string;
  description: string;
  archived: boolean;
  current_version: number;
  version: number;
  definition: ScoringConfig;
  versions: VersionInfo[];
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
  /** Which scoring config judged the latest snapshot, and which version of it. Null for pre-config snapshots. */
  scoring_config_id: string | null;
  scoring_config_name: string | null;
  scoring_config_version: number | null;
  /** The numbers the latest snapshot was judged by. Null when no config was recorded. */
  scoring_config: ScoringConfig | null;
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

// ---- questionnaires and the evidence registry (mirror matchmaking-core/src/questionnaire.rs)
export type EvidenceLevel = 'validated_instrument' | 'adapted_instrument' | 'matchmaker_defined';

export interface Instrument {
  key: string;
  name: string;
  version: number;
  kind: 'standard' | 'custom';
  dimension: string;
  description: string;
  evidence: {
    level: EvidenceLevel;
    sources: { citation: string; note: string }[];
    limitations: string;
    interpretation: string;
  };
  scale: { min: number; max: number; labels: string[] };
  items: { id: string; text: string; subscale: string; reverse: boolean }[];
  subscales: { key: string; label: string; field: string; method: 'sum' | 'mean' }[];
}

export interface InstrumentView {
  instrument: Instrument;
  archived: boolean;
  created_at: string;
  evidence_label: string;
  is_latest: boolean;
}

export interface QuestionnaireResponse {
  id: number;
  instrument_key: string;
  instrument_name: string;
  instrument_version: number;
  dimension: string;
  evidence_label: string;
  limitations: string;
  interpretation: string;
  status: 'assigned' | 'in_progress' | 'complete';
  answered: number;
  total: number;
  assigned_at: string;
  completed_at: string | null;
  scores: { subscale: string; label: string; field: string; value: number }[];
}

export interface QuestionnaireDetail {
  response: QuestionnaireResponse;
  instrument: Instrument;
  answers: Record<string, number>;
}

/** The decision dataset export (M7b). Mirrors `mm::export_commands::ExportDoc`. */
export interface ExportDoc {
  generated_at: string;
  columns: string[];
  row_count: number;
  with_outcome: number;
  csv: string;
}

/** What the decision data contains so far. Mirrors `mm::export_commands::DatasetSummary`. */
export interface DatasetSummary {
  matches: number;
  finished: number;
  /** The ranking gate: finished matches needed before a learned ranking could be compared. */
  minimum_finished: number;
  with_outcome: number;
  outcome_counts: Record<string, number>;
  open_by_status: Record<string, number>;
  with_snapshot: number;
  overridden: number;
  rule_set_versions: string[];
  earliest_created_at: string | null;
  latest_updated_at: string | null;
}

/** What the exposure log holds. Mirrors `mm::exposure_repo::ExposureSummary`. */
export interface ExposureSummary {
  exposures: number;
  /** Distinct (profile, candidate) pairs that have appeared in a result list. */
  pairs_shown: number;
  /** How many of those shown pairs later produced a recorded outcome. */
  pairs_with_outcome: number;
  searches: number;
  first_at: string | null;
  last_at: string | null;
}

/**
 * Whether there is enough recorded history to compare a learned ranking, and the counts that decide it.
 * Read-only: it reports the gate and proposes no ordering. Mirrors `mm::exposure_repo::RankingReadiness`.
 */
export interface RankingReadiness {
  /** Finished matches needed before a learned ranking could be compared (the gate). */
  minimum_finished: number;
  finished: number;
  ready: boolean;
  /** One plain sentence describing the current position, shown as-is. */
  state: string;
  with_outcome: number;
  /** Matches carrying a stored scorecard: without one there is nothing to compare a result against. */
  with_snapshot: number;
  outcome_counts: Record<string, number>;
  open_by_status: Record<string, number>;
  /** Candidate rows in the exposure log, and the distinct pairs behind them. */
  exposures: number;
  pairs_shown: number;
  /** How many of those shown pairs later produced a recorded outcome. */
  pairs_with_outcome: number;
}

// ---- ranking v1: two orderings checked against the outcomes

/** How one ordering agreed with the recorded outcomes. Mirrors `matchmaking_core::OrderingResult`. */
export interface OrderingResult {
  /** Pairs that could be compared: both rows scored, and their outcomes differed. */
  comparable_pairs: number;
  concordant: number;
  discordant: number;
  /** Pairs whose outcomes agreed, so neither ordering could have got them in or out of order. */
  tied: number;
  /** `concordant / (concordant + discordant)`, or `null` when nothing was comparable — never 0. */
  concordance: number | null;
  /** Rows this ordering could place at all (a whole score, no snapshot, or a card that produced no number). */
  usable_rows: number;
  progressed: number;
  not_progressed: number;
  /** R-precision: of the top `progressed` rows by this score, how many actually progressed. */
  top_k: number;
  top_k_rate: number | null;
}

/** Matches left out of the comparison, and why. Nothing here is filled in with a zero. */
export interface ExclusionCounts {
  /** Still moving: no outcome recorded yet, so there is no label. */
  no_outcome: number;
  /** The outcome is `unknown`: recorded, but it says nothing about which way the pair went. */
  unknown_outcome: number;
  /** No stored snapshot, so there are no numbers to put in an order. */
  no_snapshot: number;
  /** A snapshot exists but produced no score, so the pair cannot be placed. */
  no_score: number;
}

/**
 * The two orderings compared against recorded outcomes: the confidence-adjusted `ranking_score`, which orders
 * searches today, against the raw coverage-weighted `overall` it is derived from.
 *
 * Read-only: it reports which ordering agreed better with what happened, and re-orders nothing. Mirrors
 * `mm::ranking_commands::RankingComparison`.
 */
export interface RankingComparison {
  /** How the progressed / not-progressed label is decided, in one sentence written by the backend. */
  classification: string;
  /** Which ordering is in force for searches today (`ranking_score`). */
  enforced: string;
  ready: boolean;
  minimum_finished: number;
  /** One sentence saying whether this is a finding or a below-the-gate inspection. */
  gate_note: string;
  /** Matches on record, and how many of them the comparison could use. */
  matches_total: number;
  matches_compared: number;
  excluded: ExclusionCounts;
  progressed: number;
  not_progressed: number;
  baseline: OrderingResult;
  candidate: OrderingResult;
}
