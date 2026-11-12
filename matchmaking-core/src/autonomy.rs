//! Configurable autonomy: which steps the agent may take on its own, and the policy engine that checks every action.
//!
//! Pure and deterministic, like the rest of the core: no clock (the caller passes `now`), no I/O, no model.
//!
//! * `decide(action, ctx, policy)` returns [`Decision`]. The **invariants** are checked first and no setting can
//!   configure them away; only then is the step's [`StepMode`] looked at.
//! * A step that is not built yet always counts as `Off`, whatever is stored (see [`Step::is_built`]).
//! * Agent-applied values keep rank-0 provenance, so they never overwrite a person's value.
//! * [`Actor`] says who did something. It is recorded on every event and audit row.

use crate::profile::Provenance;
use crate::sharing::NEVER_SHARED;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

// ------------------------------------------------------------------ actor

/// Who performed an action. Stored as text: `matchmaker`, `system` or `agent:<run_id>`.
///
/// Phase 1 has no agent runs yet, so the deterministic acceptance step uses [`Actor::DETERMINISTIC_RUN`]; later
/// phases put the real `mm_agent_runs` id there, and the text form does not change.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Actor {
    Matchmaker,
    System,
    Agent { run_id: i64 },
}

impl Actor {
    /// The run id recorded for agent actions that belong to no run (the phase 1 deterministic step).
    pub const DETERMINISTIC_RUN: i64 = 0;

    pub fn agent_deterministic() -> Actor {
        Actor::Agent { run_id: Self::DETERMINISTIC_RUN }
    }

    pub fn as_text(&self) -> String {
        match self {
            Actor::Matchmaker => "matchmaker".to_string(),
            Actor::System => "system".to_string(),
            Actor::Agent { run_id } => format!("agent:{run_id}"),
        }
    }

    pub fn parse(text: &str) -> Option<Actor> {
        match text {
            "matchmaker" => Some(Actor::Matchmaker),
            "system" => Some(Actor::System),
            other => other.strip_prefix("agent:")?.parse().ok().map(|run_id| Actor::Agent { run_id }),
        }
    }

    pub fn is_agent(&self) -> bool {
        matches!(self, Actor::Agent { .. })
    }
}

// ------------------------------------------------------------------ steps and modes

/// One thing the agent can be allowed to do. Phases 2-7 build the rest; they are listed now so a stored policy
/// and the UI never need a new shape.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Step {
    RunExtraction,
    AcceptExtraction,
    FindMatches,
    CreateMatch,
    AnalysePair,
    AddNote,
    Recommend,
    Approve,
    Reject,
    /// Approving a pair the rules exclude. Its own step so it can be held back when everything else is Auto.
    RuleOverride,
    InterpretReply,
    RecordOutcome,
    Close,
    Stop,
    SendIntroduction,
    SendFollowUp,
    SendMessage,
    RequestInfo,
}

impl Step {
    pub const ALL: [Step; 18] = [
        Step::RunExtraction,
        Step::AcceptExtraction,
        Step::FindMatches,
        Step::CreateMatch,
        Step::AnalysePair,
        Step::AddNote,
        Step::Recommend,
        Step::Approve,
        Step::Reject,
        Step::RuleOverride,
        Step::InterpretReply,
        Step::RecordOutcome,
        Step::Close,
        Step::Stop,
        Step::SendIntroduction,
        Step::SendFollowUp,
        Step::SendMessage,
        Step::RequestInfo,
    ];

    /// Built in this release. Only `AcceptExtraction` exists in phase 1; each later phase flips its steps here.
    pub fn is_built(self) -> bool {
        matches!(self, Step::AcceptExtraction)
    }

    /// Reaches a candidate (a Telegram message). These get a review window at L3.
    pub fn is_candidate_facing(self) -> bool {
        matches!(self, Step::SendIntroduction | Step::SendFollowUp | Step::SendMessage | Step::RequestInfo)
    }

    /// Everything that stays inside the app: the matchmaker's own workflow.
    pub fn is_internal(self) -> bool {
        !self.is_candidate_facing() && self != Step::RuleOverride && self != Step::Approve
    }
}

/// How a step may run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StepMode {
    /// The agent does not do it.
    Off,
    /// The agent proposes; a person approves.
    Suggest,
    /// The agent will do it after `minutes`, unless a person vetoes.
    ReviewWindow { minutes: u32 },
    /// The agent does it.
    Auto,
}

pub const MAX_REVIEW_MINUTES: u32 = 7 * 24 * 60;
pub const DEFAULT_MIN_CONFIDENCE: f64 = 0.85;
pub const L3_REVIEW_MINUTES: u32 = 60;

// ------------------------------------------------------------------ levels and policy

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Level {
    L0Manual,
    L1Assisted,
    L2Supervised,
    L3Delegated,
    L4Autonomous,
}

impl Level {
    pub const ALL: [Level; 5] = [Level::L0Manual, Level::L1Assisted, Level::L2Supervised, Level::L3Delegated, Level::L4Autonomous];

    /// L4 is enabled in phase 8, once every step exists.
    pub fn is_released(self) -> bool {
        self != Level::L4Autonomous
    }

    pub fn mode_for(self, step: Step) -> StepMode {
        match self {
            Level::L0Manual => StepMode::Off,
            Level::L1Assisted => StepMode::Suggest,
            Level::L2Supervised => {
                if step.is_internal() { StepMode::Auto } else { StepMode::Suggest }
            }
            Level::L3Delegated => {
                if step.is_internal() || step == Step::Approve {
                    StepMode::Auto
                } else if step.is_candidate_facing() {
                    StepMode::ReviewWindow { minutes: L3_REVIEW_MINUTES }
                } else {
                    StepMode::Suggest
                }
            }
            Level::L4Autonomous => StepMode::Auto,
        }
    }
}

/// The stored autonomy setting. `level` is the preset it started from; `steps` holds every step explicitly, so an
/// individual override is just a different entry.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AutonomyPolicy {
    pub version: u32,
    pub level: Level,
    /// The kill switch: while true the agent does nothing.
    pub paused: bool,
    /// Lowest confidence at which `AcceptExtraction` accepts a field.
    pub min_confidence: f64,
    pub steps: BTreeMap<Step, StepMode>,
}

impl Default for AutonomyPolicy {
    fn default() -> Self {
        AutonomyPolicy::preset(Level::L0Manual)
    }
}

impl AutonomyPolicy {
    pub fn preset(level: Level) -> AutonomyPolicy {
        AutonomyPolicy {
            version: 1,
            level,
            paused: false,
            min_confidence: DEFAULT_MIN_CONFIDENCE,
            steps: Step::ALL.iter().map(|step| (*step, level.mode_for(*step))).collect(),
        }
    }

    /// The mode that actually applies: a step that is not built, or not listed, is `Off`.
    pub fn mode_of(&self, step: Step) -> StepMode {
        if !step.is_built() {
            return StepMode::Off;
        }
        self.steps.get(&step).copied().unwrap_or(StepMode::Off)
    }

    /// Does a stored policy still equal its preset? The UI shows "custom" when not.
    pub fn matches_preset(&self) -> bool {
        self.steps == AutonomyPolicy::preset(self.level).steps
    }

    /// Check a policy before it is stored.
    pub fn validate(&self) -> Result<(), String> {
        if !self.level.is_released() {
            return Err("This autonomy level is not available yet".into());
        }
        if !self.min_confidence.is_finite() || !(0.0..=1.0).contains(&self.min_confidence) {
            return Err("The confidence threshold must be between 0 and 1".into());
        }
        for (step, mode) in &self.steps {
            if let StepMode::ReviewWindow { minutes } = mode {
                if *minutes == 0 || *minutes > MAX_REVIEW_MINUTES {
                    return Err(format!("The review window for {step:?} must be between 1 minute and 7 days"));
                }
            }
        }
        Ok(())
    }
}

// ------------------------------------------------------------------ actions

/// Things only a person may do. The policy denies them before it looks at any step mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HumanOnly {
    AiSettings,
    Consent,
    Keys,
    TelegramToken,
    RuleSets,
    ScoringConfigs,
    FieldRegistry,
    SharingSettings,
    DeleteProfile,
    ExportProfile,
    Unlink,
    ActivateLearnedRanking,
}

impl HumanOnly {
    pub const ALL: [HumanOnly; 12] = [
        HumanOnly::AiSettings,
        HumanOnly::Consent,
        HumanOnly::Keys,
        HumanOnly::TelegramToken,
        HumanOnly::RuleSets,
        HumanOnly::ScoringConfigs,
        HumanOnly::FieldRegistry,
        HumanOnly::SharingSettings,
        HumanOnly::DeleteProfile,
        HumanOnly::ExportProfile,
        HumanOnly::Unlink,
        HumanOnly::ActivateLearnedRanking,
    ];
}

/// Something a candidate would be shown.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Disclosure {
    /// A profile field, by key. `NEVER_SHARED` keys are refused.
    Field(String),
    Identifier,
    ContactDetail,
    Score,
    Rule,
}

/// What the agent wants to do.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Action {
    /// A step that needs no further checks than its mode.
    Step(Step),
    /// Write a field value. Only rank-0 provenance is allowed.
    ApplyField { step: Step, field: String, provenance: Provenance },
    /// Show something to a candidate.
    Disclose { step: Step, what: Disclosure },
    /// Set a candidate's Interest. Never allowed for the agent.
    SetInterest,
    /// A human-only action. Never allowed for the agent.
    HumanOnly(HumanOnly),
}

impl Action {
    pub fn step(&self) -> Option<Step> {
        match self {
            Action::Step(step) | Action::ApplyField { step, .. } | Action::Disclose { step, .. } => Some(*step),
            Action::SetInterest | Action::HumanOnly(_) => None,
        }
    }
}

/// The state of the AI gate (`settings::check_gate`) as the policy sees it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AiGate {
    Open,
    Closed,
    /// A cloud provider is configured and the consent is missing or outdated.
    NeedsConsent,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DecisionContext {
    pub gate: AiGate,
    /// Seconds since the Unix epoch. Used only to compute `Schedule(at)`.
    pub now: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DenyReason {
    Paused,
    GateClosed,
    NeedsConsent,
    HumanOnly,
    InterestIsTheCandidates,
    ProvenanceTooHigh,
    NeverShared,
    StepOff,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "decision")]
pub enum Decision {
    Execute,
    Propose,
    /// Run once due; `at` is seconds since the Unix epoch.
    Schedule { at: i64 },
    Deny { reason: DenyReason },
}

/// Check one agent action. Invariants first, then the step's mode.
pub fn decide(action: &Action, ctx: &DecisionContext, policy: &AutonomyPolicy) -> Decision {
    let deny = |reason| Decision::Deny { reason };
    if policy.paused {
        return deny(DenyReason::Paused);
    }
    match ctx.gate {
        AiGate::Open => {}
        AiGate::Closed => return deny(DenyReason::GateClosed),
        AiGate::NeedsConsent => return deny(DenyReason::NeedsConsent),
    }
    match action {
        Action::HumanOnly(_) => return deny(DenyReason::HumanOnly),
        Action::SetInterest => return deny(DenyReason::InterestIsTheCandidates),
        Action::ApplyField { provenance, .. } if provenance.rank() != 0 => return deny(DenyReason::ProvenanceTooHigh),
        Action::Disclose { what, .. } => {
            let allowed = matches!(what, Disclosure::Field(key) if !NEVER_SHARED.contains(&key.as_str()));
            if !allowed {
                return deny(DenyReason::NeverShared);
            }
        }
        _ => {}
    }
    let Some(step) = action.step() else { return deny(DenyReason::HumanOnly) };
    match policy.mode_of(step) {
        StepMode::Off => deny(DenyReason::StepOff),
        StepMode::Suggest => Decision::Propose,
        StepMode::ReviewWindow { minutes } => Decision::Schedule { at: ctx.now + i64::from(minutes) * 60 },
        StepMode::Auto => Decision::Execute,
    }
}

// ------------------------------------------------------------------ extraction acceptance

/// Why the deterministic acceptance step left a suggestion pending.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SkipReason {
    NotAField,
    BelowThreshold,
    Conflict,
    Ambiguous,
    Duplicate,
    PersonOwned,
}

/// Can this pending field suggestion be accepted without a person? `existing_rank` is the provenance rank of the
/// value already stored for the field, if any. A person's value (rank above 0) is never replaced.
#[allow(clippy::too_many_arguments)]
pub fn acceptance_check(
    kind_is_field: bool,
    confidence: f64,
    conflict: bool,
    ambiguous: bool,
    duplicate: bool,
    existing_rank: Option<u8>,
    min_confidence: f64,
) -> Result<(), SkipReason> {
    if !kind_is_field {
        return Err(SkipReason::NotAField);
    }
    if conflict {
        return Err(SkipReason::Conflict);
    }
    if ambiguous {
        return Err(SkipReason::Ambiguous);
    }
    if duplicate {
        return Err(SkipReason::Duplicate);
    }
    if existing_rank.map_or(false, |rank| rank > 0) {
        return Err(SkipReason::PersonOwned);
    }
    if !(confidence >= min_confidence) {
        return Err(SkipReason::BelowThreshold);
    }
    Ok(())
}
