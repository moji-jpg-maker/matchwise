use matchmaking_core::*;
use MatchStatus::*;

fn ctx(current: MatchStatus, eligible: bool) -> TransitionContext {
    TransitionContext { current, eligible, a: Interest::Unknown, b: Interest::Unknown, outcome: None }
}
fn go(c: &TransitionContext, to: MatchStatus) -> Result<TransitionPlan, String> {
    validate_transition(c, to, &TransitionRequest::default())
}

#[test]
fn happy_path_walks_the_whole_pipeline() {
    let mut c = ctx(Recommended, true);
    for next in [Reviewed, Approved, IntroductionProposed] {
        go(&c, next).unwrap();
        c.current = next;
    }
    c.a = Interest::Interested;
    c.b = Interest::Interested;
    for next in [BothInterested, ContactExchanged, Conversation, Meeting, Feedback] {
        go(&c, next).unwrap();
        c.current = next;
    }
    // feedback can lead to another meeting, or be closed with an outcome
    assert!(go(&c, Meeting).is_ok());
    assert!(go(&c, Closed).is_err(), "closing needs an outcome");
    let req = TransitionRequest { outcome: Some(Outcome::RelationshipFormed), ..Default::default() };
    assert_eq!(validate_transition(&c, Closed, &req).unwrap().outcome, Some(Outcome::RelationshipFormed));
}

#[test]
fn invalid_jumps_and_terminal_states_are_refused() {
    assert!(go(&ctx(Recommended, true), Meeting).is_err());
    assert!(go(&ctx(Approved, true), Reviewed).is_err(), "no going backwards");
    for t in [Closed, Declined, Stopped] {
        assert!(t.allowed_next().is_empty() && t.is_terminal());
        let e = go(&ctx(t, true), Reviewed).unwrap_err();
        assert!(e.contains("finished"), "{e}");
    }
    // a rejection can be reconsidered
    assert!(go(&ctx(Rejected, true), Reviewed).is_ok());
    assert!(go(&ctx(Rejected, true), Approved).is_err());
}

#[test]
fn every_pipeline_stage_can_reach_a_terminal_state_and_stages_are_ordered() {
    for (i, s) in MatchStatus::PIPELINE.iter().enumerate() {
        assert_eq!(s.stage(), Some(i));
        assert!(!s.is_terminal());
        // walk any path of allowed moves: it must end (no cycles except Feedback<->Meeting, which can also close/stop)
        let mut seen = vec![*s];
        let mut cur = *s;
        let mut steps = 0;
        while !cur.is_terminal() && steps < 30 {
            let next = *cur.allowed_next().last().unwrap(); // last option is always the "exit" direction
            seen.push(next);
            cur = next;
            steps += 1;
        }
        assert!(cur.is_terminal(), "{:?} never terminates: {seen:?}", s);
    }
}

#[test]
fn overrides_are_required_for_excluded_pairs_and_recorded() {
    let excluded = ctx(Recommended, false);
    let e = go(&excluded, Approved).unwrap_err();
    assert!(e.contains("override"), "{e}");
    let blank = TransitionRequest { override_reason: Some("  ".into()), ..Default::default() };
    assert!(validate_transition(&excluded, Approved, &blank).is_err());
    let ok = TransitionRequest { override_reason: Some("Both families know each other; city rule is outdated".into()), ..Default::default() };
    assert!(validate_transition(&excluded, Approved, &ok).unwrap().override_used);
    // identified -> recommended for an excluded pair also needs it; rejecting never does
    assert!(go(&ctx(Identified, false), Recommended).is_err());
    assert!(go(&ctx(Identified, false), Rejected).is_ok());
    // an eligible pair needs no override and none is recorded even if one is typed
    let plan = validate_transition(&ctx(Recommended, true), Approved, &ok).unwrap();
    assert!(!plan.override_used);
}

#[test]
fn responses_drive_the_introduction() {
    use Interest::*;
    assert_eq!(apply_responses(IntroductionProposed, Interested, Unknown), None);
    assert_eq!(apply_responses(IntroductionProposed, Interested, Interested), Some(MatchStatus::BothInterested));
    assert_eq!(apply_responses(IntroductionProposed, Interested, NotInterested), Some(MatchStatus::Declined));
    assert_eq!(apply_responses(BothInterested, Interested, NotInterested), Some(MatchStatus::Declined));
    assert_eq!(apply_responses(BothInterested, Interested, Interested), None);
    assert_eq!(apply_responses(Approved, Interested, Interested), None, "responses only count during an introduction");
    assert!(can_record_responses(IntroductionProposed) && !can_record_responses(Approved));
    // BothInterested cannot be set by hand without two yes answers
    let c = ctx(IntroductionProposed, true);
    assert!(go(&c, BothInterested).is_err());
}

#[test]
fn outcomes_text_limits_and_serialization() {
    assert!(can_record_outcome(Conversation) && !can_record_outcome(Approved));
    let long = TransitionRequest { reason: Some("x".repeat(MAX_TEXT_FOR_TEST + 1)), ..Default::default() };
    assert!(validate_transition(&ctx(Recommended, true), Rejected, &long).is_err());
    assert_eq!(serde_json::to_string(&IntroductionProposed).unwrap(), "\"introduction_proposed\"");
    for s in MatchStatus::PIPELINE {
        assert_eq!(s.as_str().parse::<MatchStatus>().unwrap(), s, "string form round-trips");
    }
    assert!("nonsense".parse::<MatchStatus>().is_err());
    assert_eq!("married".parse::<Outcome>().unwrap(), Outcome::Married);
    assert_eq!("not_interested".parse::<Interest>().unwrap(), Interest::NotInterested);
}
const MAX_TEXT_FOR_TEST: usize = 2000;
