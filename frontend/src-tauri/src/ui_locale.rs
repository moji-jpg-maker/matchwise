use matchmaking_core::Locale;
use std::sync::atomic::{AtomicU8, Ordering};

static UI_LOCALE: AtomicU8 = AtomicU8::new(0);

pub fn get() -> Locale {
    match UI_LOCALE.load(Ordering::Relaxed) {
        1 => Locale::Fa,
        _ => Locale::En,
    }
}

pub fn set(locale: Locale) {
    let code = match locale {
        Locale::En => 0,
        Locale::Fa => 1,
    };
    UI_LOCALE.store(code, Ordering::Relaxed);
}

#[tauri::command]
pub fn set_ui_locale<R: tauri::Runtime>(app: tauri::AppHandle<R>, locale: String) {
    let parsed = Locale::parse_or_default(&locale);
    if parsed == get() {
        return;
    }
    set(parsed);
    crate::tray::update_tray_menu(&app);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn locale_round_trips() {
        set(Locale::Fa);
        assert_eq!(get(), Locale::Fa);
        set(Locale::En);
        assert_eq!(get(), Locale::En);
    }
}
