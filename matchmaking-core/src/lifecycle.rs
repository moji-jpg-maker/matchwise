//! The life of a match, from "identified" to an outcome, as an explicit state machine.
//!
//! Human judgment stays in charge: nothing here moves a match forward by itself except recording that both
//! people said yes (or that one said no). Approving or recommending a pair that the rules exclude needs a
//! written override reason, so every exception is on record.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MatchStatus {
    /// A candidate pair was found but is not (yet) recommended, for example because the rules exclude it.
    Identified,
    Recommended,
    Reviewed,
    Approved,
    IntroductionProposed,
    BothInterested,
    ContactExchanged,
    Conversation,
    Meeting,
    Feedback,
    // ---- terminal states
    /// Tracking ended with an outcome recorded.
    Closed,
    /// The matchmaker decided against the pair.
    Rejected,
    /// A person said they are not interested.
    Declined,
    /// The relationship ended after contact began.
    Stopped,
}

impl MatchStatus {
    pub const PIPELINE: [MatchStatus; 10] = [
        MatchStatus::Identified,
        MatchStatus::Recommended,
        MatchStatus::Reviewed,
        MatchStatus::Approved,
        MatchStatus::IntroductionProposed,
        MatchStatus::BothInterested,
        MatchStatus::ContactExchanged,
        MatchStatus::Conversation,
        MatchStatus::Meeting,
        MatchStatus::Feedback,
    ];

    pub fn is_terminal(self) -> bool {
        matches!(self, MatchStatus::Closed | MatchStatus::Rejected | MatchStatus::Declined | MatchStatus::Stopped)
    }

    /// Position in the main pipeline (None for terminal states).
    pub fn stage(self) -> Option<usize> {
        Self::PIPELINE.iter().position(|s| *s == self)
    }

    pub fn allowed_next(self) -> Vec<MatchStatus> {
        use MatchStatus::*;
        match self {
            Identified => vec![Recommended, Rejected],
            Recommended => vec![Reviewed, Approved, Rejected],
            Reviewed => vec![Approved, Rejected],
            Approved => vec![IntroductionProposed, Rejected],
            IntroductionProposed => vec![BothInterested, Declined, Rejected],
            BothInterested => vec![ContactExchanged, Declined],
            ContactExchanged => vec![Conversation, Stopped],
            Conversation => vec![Meeting, Stopped],
            Meeting => vec![Feedback, Stopped],
            Feedback => vec![Meeting, Closed, Stopped],
            // A matchmaker may reconsider a rejection; other terminal states are final.
            Rejected => vec![Reviewed],
            Closed | Declined | Stopped => vec![],
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            MatchStatus::Identified => "identified",
            MatchStatus::Recommended => "recommended",
            MatchStatus::Reviewed => "reviewed",
            MatchStatus::Approved => "approved",
            MatchStatus::IntroductionProposed => "introduction_proposed",
            MatchStatus::BothInterested => "both_interested",
            MatchStatus::ContactExchanged => "contact_exchanged",
            MatchStatus::Conversation => "conversation",
            MatchStatus::Meeting => "meeting",
            MatchStatus::Feedback => "feedback",
            MatchStatus::Closed => "closed",
            MatchStatus::Rejected => "rejected",
            MatchStatus::Declined => "declined",
            MatchStatus::Stopped => "stopped",
        }
    }
}

impl std::str::FromStr for MatchStatus {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, String> {
        serde_json::from_value(serde_json::Value::String(s.to_string())).map_err(|_| format!("unknown match status '{s}'"))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum Interest {
    #[default]
    Unknown,
    Interested,
    NotInterested,
}

impl Interest {
    pub fn as_str(self) -> &'static str {
        match self {
            Interest::Unknown => "unknown",
            Interest::Interested => "interested",
            Interest::NotInterested => "not_interested",
        }
    }
}

impl std::str::FromStr for Interest {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, String> {
        serde_json::from_value(serde_json::Value::String(s.to_string())).map_err(|_| format!("unknown response '{s}'"))
    }
}

/// What eventually happened (the list from the product plan). Recorded once contact has begun; it becomes
/// the label for later learning, so it is kept separate from the status.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Outcome {
    Interested,
    NotInterested,
    FirstConversation,
    FirstMeeting,
    Continued,
    Stopped,
    RelationshipFormed,
    Married,
    Unknown,
}

impl Outcome {
    pub fn as_str(self) -> &'static str {
        match self {
            Outcome::Interested => "interested",
            Outcome::NotInterested => "not_interested",
            Outcome::FirstConversation => "first_conversation",
            Outcome::FirstMeeting => "first_meeting",
            Outcome::Continued => "continued",
            Outcome::Stopped => "stopped",
            Outcome::RelationshipFormed => "relationship_formed",
            Outcome::Married => "married",
            Outcome::Unknown => "unknown",
        }
    }
}

impl std::str::FromStr for Outcome {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, String> {
        serde_json::from_value(serde_json::Value::String(s.to_string())).map_err(|_| format!("unknown outcome '{s}'"))
    }
}

pub const MAX_TEXT: usize = 2000;

#[derive(Debug, Clone)]
pub struct TransitionContext {
    pub current: MatchStatus,
    /// Whether the latest score snapshot says the pair is eligible under the rules.
    pub eligible: bool,
    pub a: Interest,
    pub b: Interest,
    pub outcome: Option<Outcome>,
}

#[derive(Debug, Clone, Default)]
pub struct TransitionRequest {
    pub reason: Option<String>,
    pub override_reason: Option<String>,
    pub outcome: Option<Outcome>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TransitionPlan {
    /// The pair is excluded by the rules and the matchmaker went ahead with a written reason.
    pub override_used: bool,
    pub outcome: Option<Outcome>,
    pub reason: Option<String>,
}

fn clean(s: &Option<String>) -> Option<String> {
    s.as_ref().map(|t| t.trim().to_string()).filter(|t| !t.is_empty())
}

pub fn validate_text(label: &str, s: &Option<String>) -> Result<(), String> {
    match s {
        Some(t) if t.chars().count() > MAX_TEXT => Err(format!("{label} is too long (max {MAX_TEXT} characters)")),
        _ => Ok(()),
    }
}

/// Check a requested move. Returns what to record, or a message explaining why it is not allowed.
pub fn validate_transition(ctx: &TransitionContext, to: MatchStatus, req: &TransitionRequest) -> Result<TransitionPlan, String> {
    if !ctx.current.allowed_next().contains(&to) {
        return Err(format!(
            "A match that is '{}' cannot move to '{}'{}",
            ctx.current.as_str(),
            to.as_str(),
            if ctx.current.is_terminal() { " (it is finished)" } else { "" }
        ));
    }
    validate_text("The reason", &req.reason)?;
    validate_text("The override reason", &req.override_reason)?;

    let mut override_used = false;
    if matches!(to, MatchStatus::Recommended | MatchStatus::Approved) && !ctx.eligible {
        match clean(&req.override_reason) {
            Some(r) if r.chars().count() >= 3 => override_used = true,
            _ => {
                return Err("The rules exclude this pair. Write a short override reason to proceed anyway; it is kept on record.".into());
            }
        }
    }
    if to == MatchStatus::BothInterested && !(ctx.a == Interest::Interested && ctx.b == Interest::Interested) {
        return Err("Both people must have said they are interested first".into());
    }
    let outcome = req.outcome.or(ctx.outcome);
    if to == MatchStatus::Closed && outcome.is_none() {
        return Err("Record an outcome before closing the match".into());
    }
    Ok(TransitionPlan { override_used, outcome, reason: clean(&req.reason) })
}

/// Recording what the people said can move the match on its own: a "no" ends it, two "yes" answers advance it.
pub fn apply_responses(status: MatchStatus, a: Interest, b: Interest) -> Option<MatchStatus> {
    if !matches!(status, MatchStatus::IntroductionProposed | MatchStatus::BothInterested) {
        return None;
    }
    if a == Interest::NotInterested || b == Interest::NotInterested {
        return Some(MatchStatus::Declined);
    }
    if status == MatchStatus::IntroductionProposed && a == Interest::Interested && b == Interest::Interested {
        return Some(MatchStatus::BothInterested);
    }
    None
}

/// Responses can only be recorded while an introduction is in play.
pub fn can_record_responses(status: MatchStatus) -> bool {
    matches!(status, MatchStatus::IntroductionProposed | MatchStatus::BothInterested)
}

/// An outcome is only meaningful once contact has begun, or when recording that a person declined.
pub fn can_record_outcome(status: MatchStatus) -> bool {
    matches!(
        status,
        MatchStatus::ContactExchanged | MatchStatus::Conversation | MatchStatus::Meeting | MatchStatus::Feedback
            | MatchStatus::Closed | MatchStatus::Declined | MatchStatus::Stopped
    )
}
