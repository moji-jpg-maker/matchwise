use matchmaking_core::autonomy::*;
use matchmaking_core::profile::Provenance;
use matchmaking_core::sharing::NEVER_SHARED;

const NOW: i64 = 1_000_000;

fn open() -> DecisionContext {
    DecisionContext { gate: AiGate::Open, now: NOW }
}

fn all_auto() -> AutonomyPolicy {
    let mut policy = AutonomyPolicy::preset(Level::L0Manual);
    for step in Step::ALL {
        policy.steps.insert(step, StepMode::Auto);
    }
    policy
}

fn apply(field: &str, provenance: Provenance) -> Action {
    Action::ApplyField { step: Step::AcceptExtraction, field: field.into(), provenance }
}

fn denied(reason: DenyReason) -> Decision {
    Decision::Deny { reason }
}

#[test]
fn preset_matrix() {
    for step in Step::ALL {
        assert_eq!(Level::L0Manual.mode_for(step), StepMode::Off, "{step:?}");
        assert_eq!(Level::L1Assisted.mode_for(step), StepMode::Suggest, "{step:?}");
        assert_eq!(Level::L4Autonomous.mode_for(step), StepMode::Auto, "{step:?}");

        let expected_l2 = if step.is_internal() { StepMode::Auto } else { StepMode::Suggest };
        assert_eq!(Level::L2Supervised.mode_for(step), expected_l2, "{step:?}");

        let expected_l3 = if step.is_candidate_facing() {
            StepMode::ReviewWindow { minutes: 60 }
        } else if step == Step::RuleOverride {
            StepMode::Suggest
        } else {
            StepMode::Auto
        };
        assert_eq!(Level::L3Delegated.mode_for(step), expected_l3, "{step:?}");
    }
}

#[test]
fn step_classes_match_the_spec() {
    assert!(Step::AcceptExtraction.is_internal());
    assert!(Step::Reject.is_internal());
    assert!(!Step::Approve.is_internal());
    assert!(!Step::RuleOverride.is_internal());
    for step in [Step::SendIntroduction, Step::SendFollowUp, Step::SendMessage, Step::RequestInfo] {
        assert!(step.is_candidate_facing() && !step.is_internal(), "{step:?}");
    }
}

#[test]
fn unbuilt_steps_always_count_as_off() {
    let policy = all_auto();
    for step in Step::ALL {
        let expected = if step.is_built() { StepMode::Auto } else { StepMode::Off };
        assert_eq!(policy.mode_of(step), expected, "{step:?}");
    }
    assert_eq!(policy.mode_of(Step::AcceptExtraction), StepMode::Auto);
}

#[test]
fn default_is_l0_and_does_nothing() {
    let policy = AutonomyPolicy::default();
    assert_eq!(policy.level, Level::L0Manual);
    assert_eq!(policy.min_confidence, 0.85);
    assert!(!policy.paused);
    assert_eq!(decide(&apply("city", Provenance::AiInferred), &open(), &policy), denied(DenyReason::StepOff));
}

#[test]
fn built_step_follows_the_preset_mode() {
    let action = apply("city", Provenance::AiInferred);
    let at = |level| decide(&action, &open(), &AutonomyPolicy::preset(level));
    assert_eq!(at(Level::L0Manual), denied(DenyReason::StepOff));
    assert_eq!(at(Level::L1Assisted), Decision::Propose);
    assert_eq!(at(Level::L2Supervised), Decision::Execute);
    assert_eq!(at(Level::L3Delegated), Decision::Execute);
}

#[test]
fn phase_two_steps_follow_each_mode_without_enabling_later_steps() {
    for step in [Step::RunExtraction, Step::AcceptExtraction, Step::FindMatches, Step::CreateMatch, Step::AnalysePair] {
        assert!(step.is_built());
        let action = Action::Step(step);
        assert_eq!(decide(&action, &open(), &AutonomyPolicy::default()), denied(DenyReason::StepOff));
        assert_eq!(decide(&action, &open(), &AutonomyPolicy::preset(Level::L1Assisted)), Decision::Propose);
        let mut policy = AutonomyPolicy::preset(Level::L2Supervised);
        assert_eq!(decide(&action, &open(), &policy), Decision::Execute);
        policy.steps.insert(step, StepMode::ReviewWindow { minutes: 60 });
        assert_eq!(decide(&action, &open(), &policy), Decision::Schedule { at: NOW + 3600 });
    }
    for step in [Step::Approve, Step::Recommend, Step::RuleOverride, Step::SendMessage] {
        assert!(!step.is_built());
        assert_eq!(decide(&Action::Step(step), &open(), &all_auto()), denied(DenyReason::StepOff));
    }
}

#[test]
fn individual_override_wins_over_the_preset() {
    let action = apply("city", Provenance::AiInferred);
    let mut policy = AutonomyPolicy::preset(Level::L2Supervised);
    assert!(policy.matches_preset());
    policy.steps.insert(Step::AcceptExtraction, StepMode::Suggest);
    assert!(!policy.matches_preset());
    assert_eq!(decide(&action, &open(), &policy), Decision::Propose);

    policy.steps.insert(Step::AcceptExtraction, StepMode::ReviewWindow { minutes: 30 });
    assert_eq!(decide(&action, &open(), &policy), Decision::Schedule { at: NOW + 30 * 60 });

    policy.steps.insert(Step::AcceptExtraction, StepMode::Off);
    assert_eq!(decide(&action, &open(), &policy), denied(DenyReason::StepOff));
}

#[test]
fn kill_switch_and_gate_stop_everything() {
    let mut policy = all_auto();
    let action = apply("city", Provenance::AiInferred);
    assert_eq!(decide(&action, &open(), &policy), Decision::Execute);

    let closed = DecisionContext { gate: AiGate::Closed, now: NOW };
    let consent = DecisionContext { gate: AiGate::NeedsConsent, now: NOW };
    assert_eq!(decide(&action, &closed, &policy), denied(DenyReason::GateClosed));
    assert_eq!(decide(&action, &consent, &policy), denied(DenyReason::NeedsConsent));
    for step in Step::ALL {
        assert_eq!(decide(&Action::Step(step), &closed, &policy), denied(DenyReason::GateClosed));
    }

    policy.paused = true;
    assert_eq!(decide(&action, &open(), &policy), denied(DenyReason::Paused));
    assert_eq!(decide(&action, &closed, &policy), denied(DenyReason::Paused));
}

#[test]
fn human_only_actions_are_denied_under_all_auto() {
    let policy = all_auto();
    for what in HumanOnly::ALL {
        assert_eq!(decide(&Action::HumanOnly(what), &open(), &policy), denied(DenyReason::HumanOnly), "{what:?}");
    }
}

#[test]
fn the_agent_never_sets_interest() {
    assert_eq!(decide(&Action::SetInterest, &open(), &all_auto()), denied(DenyReason::InterestIsTheCandidates));
}

#[test]
fn only_rank_zero_provenance_may_be_applied() {
    let policy = all_auto();
    for provenance in [Provenance::AiInferred, Provenance::RuleExtracted] {
        assert_eq!(decide(&apply("city", provenance), &open(), &policy), Decision::Execute, "{provenance:?}");
    }
    for provenance in [Provenance::Questionnaire, Provenance::User, Provenance::Matchmaker] {
        assert_eq!(decide(&apply("city", provenance), &open(), &policy), denied(DenyReason::ProvenanceTooHigh), "{provenance:?}");
    }
}

#[test]
fn nothing_private_reaches_a_candidate() {
    let policy = all_auto();
    let disclose = |what| Action::Disclose { step: Step::SendMessage, what };
    for key in NEVER_SHARED {
        assert_eq!(decide(&disclose(Disclosure::Field(key.to_string())), &open(), &policy), denied(DenyReason::NeverShared), "{key}");
    }
    for what in [Disclosure::Identifier, Disclosure::ContactDetail, Disclosure::Score, Disclosure::Rule] {
        assert_eq!(decide(&disclose(what.clone()), &open(), &policy), denied(DenyReason::NeverShared), "{what:?}");
    }
}

#[test]
fn the_never_shared_list_covers_the_private_fields() {
    for key in ["phone", "telegram_username", "full_name", "date_of_birth"] {
        assert!(NEVER_SHARED.contains(&key), "{key}");
    }
}

#[test]
fn disclosure_of_an_ordinary_field_follows_the_step_mode() {
    let action = Action::Disclose { step: Step::SendIntroduction, what: Disclosure::Field("city".into()) };
    assert_eq!(decide(&action, &open(), &AutonomyPolicy::preset(Level::L3Delegated)), denied(DenyReason::StepOff));
}

#[test]
fn decisions_serialize_with_a_tag() {
    assert_eq!(serde_json::to_value(Decision::Execute).unwrap(), serde_json::json!({"decision": "execute"}));
    assert_eq!(serde_json::to_value(Decision::Schedule { at: 5 }).unwrap(), serde_json::json!({"decision": "schedule", "at": 5}));
    assert_eq!(serde_json::to_value(denied(DenyReason::Paused)).unwrap(), serde_json::json!({"decision": "deny", "reason": "paused"}));
}

#[test]
fn validation_rejects_bad_policies() {
    assert!(AutonomyPolicy::preset(Level::L2Supervised).validate().is_ok());
    assert!(AutonomyPolicy::preset(Level::L4Autonomous).validate().is_err());

    let mut policy = AutonomyPolicy::default();
    policy.min_confidence = 1.5;
    assert!(policy.validate().is_err());
    policy.min_confidence = f64::NAN;
    assert!(policy.validate().is_err());

    let mut policy = AutonomyPolicy::default();
    policy.steps.insert(Step::AcceptExtraction, StepMode::ReviewWindow { minutes: 0 });
    assert!(policy.validate().is_err());
}

#[test]
fn actor_text_round_trips() {
    for actor in [Actor::Matchmaker, Actor::System, Actor::Agent { run_id: 0 }, Actor::Agent { run_id: 42 }] {
        assert_eq!(Actor::parse(&actor.as_text()), Some(actor));
    }
    assert_eq!(Actor::agent_deterministic().as_text(), "agent:0");
    assert_eq!(Actor::parse("agent:x"), None);
    assert_eq!(Actor::parse("bot"), None);
    assert!(Actor::agent_deterministic().is_agent() && !Actor::Matchmaker.is_agent());
}

#[test]
fn acceptance_check_cases() {
    let check = |kind_is_field, confidence, conflict, ambiguous, duplicate, rank| {
        acceptance_check(kind_is_field, confidence, conflict, ambiguous, duplicate, rank, 0.85)
    };
    assert_eq!(check(true, 0.9, false, false, false, None), Ok(()));
    assert_eq!(check(true, 0.85, false, false, false, None), Ok(()));
    assert_eq!(check(true, 0.9, false, false, false, Some(0)), Ok(()));
    assert_eq!(check(false, 0.9, false, false, false, None), Err(SkipReason::NotAField));
    assert_eq!(check(true, 0.84, false, false, false, None), Err(SkipReason::BelowThreshold));
    assert_eq!(check(true, f64::NAN, false, false, false, None), Err(SkipReason::BelowThreshold));
    assert_eq!(check(true, 0.99, true, false, false, None), Err(SkipReason::Conflict));
    assert_eq!(check(true, 0.99, false, true, false, None), Err(SkipReason::Ambiguous));
    assert_eq!(check(true, 0.99, false, false, true, None), Err(SkipReason::Duplicate));
    for rank in [1, 2, 3] {
        assert_eq!(check(true, 0.99, false, false, false, Some(rank)), Err(SkipReason::PersonOwned));
    }
}

#[test]
fn policy_json_matches_the_golden_fixture() {
    let fixture: serde_json::Value = serde_json::from_str(include_str!("fixtures/autonomy/policy.json")).unwrap();
    let policy = AutonomyPolicy::preset(Level::L3Delegated);
    assert_eq!(serde_json::to_value(&policy).unwrap(), fixture);
    let back: AutonomyPolicy = serde_json::from_value(fixture).unwrap();
    assert_eq!(back, policy);
}
