//! Message texts (Telegram HTML). Everything user-supplied goes through [`esc`].

use matchmaking_core::{display_value, FieldRegistry, IntroCard, MatchStatus, Profile};

pub fn esc(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

pub fn help_text() -> String {
    "<b>How this works</b>\n\
     Your matchmaker uses this chat to keep your profile complete and to introduce you to people who may suit you.\n\n\
     /profile - see what is saved about you\n\
     /edit - complete or change your profile\n\
     /preferences - what you are looking for\n\
     /matches - your introductions\n\
     /status - where things stand\n\
     /settings - notifications and privacy\n\n\
     You can also just write a message here and your matchmaker will read it.\n\
     /cancel stops whatever we are in the middle of."
        .to_string()
}

pub fn unlinked_text() -> String {
    "This bot works with an invitation from your matchmaker. Please open the link they sent you, or ask them for a new one.".to_string()
}

pub fn invalid_code_text() -> String {
    "That invitation is not valid or has expired. Please ask your matchmaker for a new link.".to_string()
}

pub fn too_many_attempts_text() -> String {
    "Too many attempts. Please wait an hour and try again, or ask your matchmaker for help.".to_string()
}

/// The notice people agree to before anything else happens. Its wording is versioned (`CONSENT_VERSION`).
pub fn consent_text() -> String {
    "<b>Before we start</b>\n\
     Your matchmaker has linked this chat to your profile. Please read how your information is handled:\n\n\
     • Messages in this chat pass through Telegram's servers. Do not send anything here you are not comfortable with Telegram handling.\n\
     • Your profile is kept by your matchmaker, who can see everything you send here.\n\
     • I will never show other people your contact details. If you are introduced to someone, they see only the facts your matchmaker chose to share, and your first name.\n\
     • I will not display sensitive details you have given (for example health or finances) back to you in this chat.\n\
     • You can stop messages (/stop), unlink this chat (/unlink) or ask for your data to be deleted (/forget) at any time.\n\n\
     Do you agree?"
        .to_string()
}

pub fn consent_declined_text() -> String {
    "No problem. I have unlinked this chat and will not send you anything. Your matchmaker can send you a new invitation if you change your mind.".to_string()
}

pub fn welcome_text(name: &str) -> String {
    format!("Welcome, {}! You are linked. Send /edit to complete your profile, or /help to see what I can do.", esc(name))
}

/// A person's own profile, without any sensitive value: those are only counted.
pub fn profile_summary(profile: &Profile, registry: &FieldRegistry) -> String {
    let mut lines = vec![];
    let mut hidden = 0;
    for def in registry.defs() {
        let Some(v) = profile.get(&def.key) else { continue };
        if matches!(def.kind, matchmaking_core::FieldKind::Records(_)) {
            lines.push(format!("{}: saved", esc(&def.label)));
            continue;
        }
        if def.sensitive {
            hidden += 1;
            continue;
        }
        if let Some(t) = display_value(v) {
            lines.push(format!("{}: {}", esc(&def.label), esc(&t)));
        }
    }
    let mut out = String::from("<b>Your profile</b>\n");
    if lines.is_empty() {
        out.push_str("Nothing saved yet.\n");
    }
    for l in lines {
        out.push_str(&format!("• {l}\n"));
    }
    if hidden > 0 {
        out.push_str(&format!("\n{hidden} sensitive detail(s) are saved but not shown here."));
    }
    let missing = profile.missing_required(registry);
    if !missing.is_empty() {
        let labels: Vec<String> = missing.iter().filter_map(|k| registry.get(k).map(|d| esc(&d.label))).collect();
        out.push_str(&format!("\n\nStill needed: {}. Send /edit to add them.", labels.join(", ")));
    } else if let Some(c) = profile.completeness(registry) {
        out.push_str(&format!("\n\nYour profile is {}% complete.", (c * 100.0).round()));
    }
    out
}

pub fn introduction_text(card: &IntroCard, awaiting_answer: bool) -> String {
    let mut out = format!("<b>Introduction</b>\nYour matchmaker would like to introduce you to <b>{}</b>.\n", esc(&card.title));
    if card.lines.is_empty() {
        out.push_str("\nYour matchmaker will tell you more.\n");
    } else {
        out.push('\n');
        for (label, value) in &card.lines {
            out.push_str(&format!("• {}: {}\n", esc(label), esc(value)));
        }
    }
    if awaiting_answer {
        out.push_str("\nWould you like to be introduced? Your answer is shared with your matchmaker only; the other person is told only if you are both interested.");
    }
    out
}

/// What a person may know about an introduction. No scores, no internal states, no reasons.
pub fn friendly_status(status: MatchStatus, my_answer_given: bool, other_name: &str) -> String {
    match status {
        MatchStatus::IntroductionProposed if !my_answer_given => "Waiting for your answer".to_string(),
        MatchStatus::IntroductionProposed => format!("Waiting for {} to answer", esc(other_name)),
        MatchStatus::BothInterested => "You are both interested. Your matchmaker will be in touch".to_string(),
        MatchStatus::ContactExchanged | MatchStatus::Conversation | MatchStatus::Meeting | MatchStatus::Feedback => "In progress".to_string(),
        _ => "Finished".to_string(),
    }
}

pub fn both_interested_text(other_name: &str) -> String {
    format!("Good news: you and <b>{}</b> are both interested. Your matchmaker will be in touch to arrange the next step.", esc(other_name))
}

pub fn not_going_ahead_text() -> String {
    "Thank you for your interest. Unfortunately this introduction will not go ahead. Your matchmaker will keep looking for someone who suits you.".to_string()
}

pub fn withdrawn_text() -> String {
    "Your matchmaker has withdrawn an earlier introduction. Nothing more is needed from you.".to_string()
}

pub fn reminder_intro_text(card_title: &str) -> String {
    format!("A gentle reminder: your matchmaker is still waiting for your answer about <b>{}</b>. You can answer with the buttons on the introduction above, or send /matches.", esc(card_title))
}

pub fn reminder_profile_text(missing: &[String]) -> String {
    format!("Your profile is almost ready. To suggest the best matches your matchmaker still needs: {}. Send /edit to add them.", missing.iter().map(|m| esc(m)).collect::<Vec<_>>().join(", "))
}

pub fn info_request_text(text: &str) -> String {
    format!("<b>Your matchmaker asks:</b>\n{}\n\nJust reply here.", esc(text))
}

pub fn matchmaker_message_text(text: &str) -> String {
    format!("<b>Message from your matchmaker</b>\n{}", esc(text))
}

/// Telegram HTML to plain text, for showing the matchmaker exactly what was (or will be) sent.
pub fn plain(html: &str) -> String {
    let mut out = String::with_capacity(html.len());
    let mut in_tag = false;
    for c in html.chars() {
        match c {
            '<' => in_tag = true,
            '>' if in_tag => in_tag = false,
            _ if !in_tag => out.push(c),
            _ => {}
        }
    }
    out.replace("&lt;", "<").replace("&gt;", ">").replace("&amp;", "&")
}
