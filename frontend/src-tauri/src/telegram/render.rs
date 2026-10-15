//! Message texts (Telegram HTML), rendered in the recipient's language. Everything user-supplied goes through [`esc`].

use matchmaking_core::{display_value, localize_digits, FieldRegistry, IntroCard, Locale, MatchStatus, Profile};

pub fn esc(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

/// Escapes user text and, in right-to-left languages, isolates it so Latin names and numbers keep their place in the sentence.
pub fn user_text(locale: Locale, s: &str) -> String {
    let escaped = esc(s);
    match locale {
        Locale::En => escaped,
        Locale::Fa => format!("\u{2068}{escaped}\u{2069}"),
    }
}

fn number(locale: Locale, n: impl std::fmt::Display) -> String {
    localize_digits(locale, &n.to_string())
}

pub fn language_prompt_text() -> String {
    "<b>Language / زبان</b>\nChoose a language. / یک زبان انتخاب کنید.".to_string()
}

pub fn language_changed_text(locale: Locale) -> String {
    locale.pick("Language set to English.", "زبان روی فارسی تنظیم شد.").to_string()
}

pub fn help_text(locale: Locale) -> String {
    match locale {
        Locale::En => "<b>How this works</b>\n\
     Your matchmaker uses this chat to keep your profile complete and to introduce you to people who may suit you.\n\n\
     /profile - see what is saved about you\n\
     /edit - complete or change your profile\n\
     /preferences - what you are looking for\n\
     /matches - your introductions\n\
     /questionnaires - questions your matchmaker would like you to answer\n\
     /status - where things stand\n\
     /settings - notifications and privacy\n\
     /language - change the language\n\n\
     You can also just write a message here and your matchmaker will read it.\n\
     /cancel stops whatever we are in the middle of."
            .to_string(),
        Locale::Fa => "<b>این ربات چطور کار می‌کند</b>\n\
     مشاور شما از این گفتگو برای کامل نگه داشتن پروفایل شما و معرفی افرادی که ممکن است با شما سازگار باشند استفاده می‌کند.\n\n\
     /profile - دیدن اطلاعات ذخیره‌شده درباره شما\n\
     /edit - تکمیل یا ویرایش پروفایل\n\
     /preferences - آنچه به دنبال آن هستید\n\
     /matches - معرفی‌های شما\n\
     /questionnaires - پرسش‌هایی که مشاور شما می‌خواهد پاسخ دهید\n\
     /status - وضعیت کارها\n\
     /settings - اعلان‌ها و حریم خصوصی\n\
     /language - تغییر زبان\n\n\
     می‌توانید همین‌جا پیام بنویسید و مشاورتان آن را می‌خواند.\n\
     /cancel هر کاری را که در میانه آن هستیم متوقف می‌کند."
            .to_string(),
    }
}

pub fn unlinked_text(locale: Locale) -> String {
    locale
        .pick(
            "This bot works with an invitation from your matchmaker. Please open the link they sent you, or ask them for a new one.",
            "این ربات با دعوت‌نامه مشاور شما کار می‌کند. لطفاً پیوندی را که برایتان فرستاده باز کنید یا از او پیوند تازه‌ای بخواهید.",
        )
        .to_string()
}

pub fn invalid_code_text(locale: Locale) -> String {
    locale
        .pick(
            "That invitation is not valid or has expired. Please ask your matchmaker for a new link.",
            "این دعوت‌نامه معتبر نیست یا منقضی شده است. لطفاً از مشاور خود پیوند تازه‌ای بخواهید.",
        )
        .to_string()
}

pub fn too_many_attempts_text(locale: Locale) -> String {
    locale
        .pick(
            "Too many attempts. Please wait an hour and try again, or ask your matchmaker for help.",
            "تلاش‌های بیش از حد. لطفاً یک ساعت صبر کنید و دوباره امتحان کنید یا از مشاور خود کمک بخواهید.",
        )
        .to_string()
}

/// The notice people agree to before anything else happens. Its wording is versioned (`CONSENT_VERSION`).
/// The Persian wording is a draft that needs legal review before release.
pub fn consent_text(locale: Locale) -> String {
    match locale {
        Locale::En => "<b>Before we start</b>\n\
     Your matchmaker has linked this chat to your profile. Please read how your information is handled:\n\n\
     • Messages in this chat pass through Telegram's servers. Do not send anything here you are not comfortable with Telegram handling.\n\
     • Your profile is kept by your matchmaker, who can see everything you send here.\n\
     • I will never show other people your contact details. If you are introduced to someone, they see only the facts your matchmaker chose to share, and your first name.\n\
     • I will not display sensitive details you have given (for example health or finances) back to you in this chat.\n\
     • You can stop messages (/stop), unlink this chat (/unlink) or ask for your data to be deleted (/forget) at any time.\n\n\
     Do you agree?"
            .to_string(),
        Locale::Fa => "<b>پیش از شروع</b>\n\
     مشاور شما این گفتگو را به پروفایل شما متصل کرده است. لطفاً بخوانید اطلاعات شما چگونه نگهداری می‌شود:\n\n\
     • پیام‌های این گفتگو از سرورهای تلگرام عبور می‌کنند. چیزی را اینجا نفرستید که راضی نیستید تلگرام آن را پردازش کند.\n\
     • پروفایل شما نزد مشاورتان نگهداری می‌شود و او هر چه را اینجا بفرستید می‌بیند.\n\
     • من هرگز اطلاعات تماس شما را به دیگران نشان نمی‌دهم. اگر به کسی معرفی شوید، او فقط اطلاعاتی را می‌بیند که مشاورتان برای اشتراک انتخاب کرده و نام کوچک شما را.\n\
     • من اطلاعات حساسی را که داده‌اید (مثلاً سلامت یا امور مالی) در این گفتگو دوباره به شما نشان نمی‌دهم.\n\
     • هر زمان بخواهید می‌توانید پیام‌ها را متوقف کنید (/stop)، اتصال این گفتگو را قطع کنید (/unlink) یا درخواست حذف اطلاعاتتان را بدهید (/forget).\n\n\
     موافقید؟"
            .to_string(),
    }
}

pub fn consent_declined_text(locale: Locale) -> String {
    locale
        .pick(
            "No problem. I have unlinked this chat and will not send you anything. Your matchmaker can send you a new invitation if you change your mind.",
            "مشکلی نیست. اتصال این گفتگو را قطع کردم و چیزی برایتان نمی‌فرستم. اگر نظرتان عوض شد، مشاورتان می‌تواند دعوت‌نامه تازه‌ای بفرستد.",
        )
        .to_string()
}

pub fn welcome_text(locale: Locale, name: &str) -> String {
    match locale {
        Locale::En => format!("Welcome, {}! You are linked. Send /edit to complete your profile, or /help to see what I can do.", esc(name)),
        Locale::Fa => format!("خوش آمدید، {}! اتصال برقرار شد. برای تکمیل پروفایل /edit را بفرستید یا با /help ببینید چه کارهایی از من برمی‌آید.", user_text(locale, name)),
    }
}

/// A person's own profile, without any sensitive value: those are only counted.
pub fn profile_summary(locale: Locale, profile: &Profile, registry: &FieldRegistry) -> String {
    let mut lines = vec![];
    let mut hidden = 0;
    for def in registry.defs() {
        let Some(v) = profile.get(&def.key) else { continue };
        let label = esc(def.label_for(locale));
        if matches!(def.kind, matchmaking_core::FieldKind::Records(_)) {
            lines.push(format!("{label}: {}", locale.pick("saved", "ذخیره شد")));
            continue;
        }
        if def.sensitive {
            hidden += 1;
            continue;
        }
        if let Some(t) = display_value(v) {
            lines.push(format!("{label}: {}", user_text(locale, &t)));
        }
    }
    let mut out = format!("<b>{}</b>\n", locale.pick("Your profile", "پروفایل شما"));
    if lines.is_empty() {
        out.push_str(locale.pick("Nothing saved yet.\n", "هنوز چیزی ذخیره نشده است.\n"));
    }
    for l in lines {
        out.push_str(&format!("• {l}\n"));
    }
    if hidden > 0 {
        out.push_str(&match locale {
            Locale::En => format!("\n{hidden} sensitive detail(s) are saved but not shown here."),
            Locale::Fa => format!("\n{} مورد اطلاعات حساس ذخیره شده اما اینجا نشان داده نمی‌شود.", number(locale, hidden)),
        });
    }
    let missing = profile.missing_required(registry);
    if !missing.is_empty() {
        let labels: Vec<String> = missing.iter().filter_map(|k| registry.get(k).map(|d| esc(d.label_for(locale)))).collect();
        let separator = locale.pick(", ", "، ");
        out.push_str(&match locale {
            Locale::En => format!("\n\nStill needed: {}. Send /edit to add them.", labels.join(separator)),
            Locale::Fa => format!("\n\nهنوز لازم است: {}. برای افزودن آن‌ها /edit را بفرستید.", labels.join(separator)),
        });
    } else if let Some(c) = profile.completeness(registry) {
        let percent = (c * 100.0).round();
        out.push_str(&match locale {
            Locale::En => format!("\n\nYour profile is {percent}% complete."),
            Locale::Fa => format!("\n\nپروفایل شما {} درصد کامل است.", number(locale, percent)),
        });
    }
    out
}

pub fn introduction_text(locale: Locale, card: &IntroCard, awaiting_answer: bool) -> String {
    let mut out = match locale {
        Locale::En => format!("<b>Introduction</b>\nYour matchmaker would like to introduce you to <b>{}</b>.\n", esc(&card.title)),
        Locale::Fa => format!("<b>معرفی</b>\nمشاور شما می‌خواهد شما را به <b>{}</b> معرفی کند.\n", user_text(locale, &card.title)),
    };
    if card.lines.is_empty() {
        out.push_str(locale.pick("\nYour matchmaker will tell you more.\n", "\nمشاور شما بیشتر توضیح می‌دهد.\n"));
    } else {
        out.push('\n');
        for (label, value) in &card.lines {
            out.push_str(&format!("• {}: {}\n", esc(label), user_text(locale, value)));
        }
    }
    if awaiting_answer {
        out.push_str(locale.pick(
            "\nWould you like to be introduced? Your answer is shared with your matchmaker only; the other person is told only if you are both interested.",
            "\nمایل به معرفی هستید؟ پاسخ شما فقط با مشاورتان در میان گذاشته می‌شود؛ فقط اگر هر دو علاقه‌مند باشید به طرف مقابل گفته می‌شود.",
        ));
    }
    out
}

/// What a person may know about an introduction. No scores, no internal states, no reasons.
pub fn friendly_status(locale: Locale, status: MatchStatus, my_answer_given: bool, other_name: &str) -> String {
    match status {
        MatchStatus::IntroductionProposed if !my_answer_given => locale.pick("Waiting for your answer", "در انتظار پاسخ شما").to_string(),
        MatchStatus::IntroductionProposed => match locale {
            Locale::En => format!("Waiting for {} to answer", esc(other_name)),
            Locale::Fa => format!("در انتظار پاسخ {}", user_text(locale, other_name)),
        },
        MatchStatus::BothInterested => locale.pick("You are both interested. Your matchmaker will be in touch", "هر دو علاقه‌مندید. مشاورتان با شما تماس می‌گیرد").to_string(),
        MatchStatus::ContactExchanged | MatchStatus::Conversation | MatchStatus::Meeting | MatchStatus::Feedback => locale.pick("In progress", "در جریان").to_string(),
        _ => locale.pick("Finished", "پایان‌یافته").to_string(),
    }
}

pub fn both_interested_text(locale: Locale, other_name: &str) -> String {
    match locale {
        Locale::En => format!("Good news: you and <b>{}</b> are both interested. Your matchmaker will be in touch to arrange the next step.", esc(other_name)),
        Locale::Fa => format!("خبر خوب: شما و <b>{}</b> هر دو علاقه‌مند هستید. مشاورتان برای هماهنگی مرحله بعد با شما تماس می‌گیرد.", user_text(locale, other_name)),
    }
}

pub fn not_going_ahead_text(locale: Locale) -> String {
    locale
        .pick(
            "Thank you for your interest. Unfortunately this introduction will not go ahead. Your matchmaker will keep looking for someone who suits you.",
            "از علاقه شما سپاسگزاریم. متأسفانه این معرفی پیش نمی‌رود. مشاورتان به جست‌وجوی فرد مناسب برای شما ادامه می‌دهد.",
        )
        .to_string()
}

pub fn withdrawn_text(locale: Locale) -> String {
    locale
        .pick(
            "Your matchmaker has withdrawn an earlier introduction. Nothing more is needed from you.",
            "مشاور شما یک معرفی قبلی را پس گرفته است. نیازی به اقدام از جانب شما نیست.",
        )
        .to_string()
}

pub fn reminder_intro_text(locale: Locale, card_title: &str) -> String {
    match locale {
        Locale::En => format!("A gentle reminder: your matchmaker is still waiting for your answer about <b>{}</b>. You can answer with the buttons on the introduction above, or send /matches.", esc(card_title)),
        Locale::Fa => format!("یادآوری مهربانانه: مشاور شما هنوز منتظر پاسخ شما درباره <b>{}</b> است. می‌توانید با دکمه‌های معرفی بالا پاسخ دهید یا /matches را بفرستید.", user_text(locale, card_title)),
    }
}

pub fn reminder_profile_text(locale: Locale, missing: &[String]) -> String {
    let joined = missing.iter().map(|m| esc(m)).collect::<Vec<_>>().join(locale.pick(", ", "، "));
    match locale {
        Locale::En => format!("Your profile is almost ready. To suggest the best matches your matchmaker still needs: {joined}. Send /edit to add them."),
        Locale::Fa => format!("پروفایل شما تقریباً آماده است. برای پیشنهاد بهترین گزینه‌ها مشاورتان هنوز به این موارد نیاز دارد: {joined}. برای افزودن آن‌ها /edit را بفرستید."),
    }
}

pub fn info_request_text(locale: Locale, text: &str) -> String {
    match locale {
        Locale::En => format!("<b>Your matchmaker asks:</b>\n{}\n\nJust reply here.", esc(text)),
        Locale::Fa => format!("<b>مشاور شما می‌پرسد:</b>\n{}\n\nکافی است همین‌جا پاسخ دهید.", user_text(locale, text)),
    }
}

pub fn questionnaire_assigned_text(locale: Locale, name: &str) -> String {
    match locale {
        Locale::En => format!("Your matchmaker would like you to answer a short questionnaire: <b>{}</b>. Send /questionnaires to start; you can stop and continue later.", esc(name)),
        Locale::Fa => format!("مشاور شما از شما می‌خواهد به یک پرسشنامهٔ کوتاه پاسخ دهید: <b>{}</b>. برای شروع /questionnaires را بفرستید؛ می‌توانید هر زمان متوقف شوید و بعداً ادامه دهید.", user_text(locale, name)),
    }
}

pub fn matchmaker_message_text(locale: Locale, text: &str) -> String {
    format!("<b>{}</b>\n{}", locale.pick("Message from your matchmaker", "پیام از مشاور شما"), user_text(locale, text))
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
