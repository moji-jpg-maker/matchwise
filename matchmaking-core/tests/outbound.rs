use matchmaking_core::outbound::{screen, OutboundDenial};

#[test]
fn english_and_persian_outbound_screen() {
    for text in [
        "Call +1 (212) 555-0199",
        "تماس ۰۹۱۲ ۱۲۳ ۴۵۶۷",
        "٠٩١٢١٢٣٤٥٦٧",
        "a@example.com",
        "@candidate",
        "https://example.com",
        "example.org/path",
        "t.me/person",
        "＠candidate",
        "h\u{200b}ttps://example.com",
        "۱۲۳\u{200b}۴۵۶۷",
    ] {
        assert_eq!(screen(text, &[]), Err(OutboundDenial::Contact), "{text}");
    }
    for text in [
        "Your score is eighty",
        "Compatibility looks good",
        "امتیاز شما بالا است",
        "۹۵٪",
        "8/10",
    ] {
        assert_eq!(screen(text, &[]), Err(OutboundDenial::Score), "{text}");
    }
    let protected = vec![
        "Alice Smith".into(),
        "بیماری خاص".into(),
        "income_rule".into(),
        "87.5".into(),
    ];
    for text in [
        "ALICE Smith",
        "Alice-Smith",
        "بیماری‌خاص",
        "income rule",
        "۸۷٫۵",
    ] {
        assert_eq!(
            screen(text, &protected),
            Err(OutboundDenial::ProtectedValue),
            "{text}"
        );
    }
    for text in [
        "How did your conversation go?",
        "گفتگوی شما چطور پیش رفت؟",
        "<b>Hello & welcome</b>",
    ] {
        assert_eq!(screen(text, &protected), Ok(()));
    }
    assert_eq!(screen(&"ش".repeat(600), &[]), Ok(()));
    assert_eq!(screen(&"ش".repeat(601), &[]), Err(OutboundDenial::TooLong));
    assert_eq!(screen(" \n ", &[]), Err(OutboundDenial::Empty));
}
