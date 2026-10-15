use matchmaking_core::Locale;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Notification {
    pub id: Option<String>,
    pub title: String,
    pub body: String,
    pub notification_type: NotificationType,
    pub priority: NotificationPriority,
    pub timeout: NotificationTimeout,
    pub icon: Option<String>,
    pub sound: bool,
    pub actions: Vec<NotificationAction>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum NotificationType {
    RecordingStarted,
    RecordingStopped,
    RecordingPaused,
    RecordingResumed,
    TranscriptionComplete,
    MeetingReminder(u64), // Duration in minutes
    SystemError(String),
    Test, // For testing notifications
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum NotificationPriority {
    Low,
    Normal,
    High,
    Critical,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum NotificationTimeout {
    Never,
    Seconds(u64),
    Default,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NotificationAction {
    pub id: String,
    pub title: String,
    pub action_type: NotificationActionType,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum NotificationActionType {
    Button,
    Reply,
}

impl Notification {
    pub fn new(title: impl Into<String>, body: impl Into<String>, notification_type: NotificationType) -> Self {
        Self {
            id: None,
            title: title.into(),
            body: body.into(),
            notification_type,
            priority: NotificationPriority::Normal,
            timeout: NotificationTimeout::Default,
            icon: None,
            sound: true,
            actions: vec![],
        }
    }

    pub fn with_priority(mut self, priority: NotificationPriority) -> Self {
        self.priority = priority;
        self
    }

    pub fn with_timeout(mut self, timeout: NotificationTimeout) -> Self {
        self.timeout = timeout;
        self
    }

    pub fn with_sound(mut self, sound: bool) -> Self {
        self.sound = sound;
        self
    }

    pub fn with_icon(mut self, icon: impl Into<String>) -> Self {
        self.icon = Some(icon.into());
        self
    }

    pub fn with_id(mut self, id: impl Into<String>) -> Self {
        self.id = Some(id.into());
        self
    }

    pub fn add_action(mut self, action: NotificationAction) -> Self {
        self.actions.push(action);
        self
    }
}

impl Default for NotificationPriority {
    fn default() -> Self {
        NotificationPriority::Normal
    }
}

impl Default for NotificationTimeout {
    fn default() -> Self {
        NotificationTimeout::Default
    }
}

// Helper functions for creating common notifications
impl Notification {
    pub fn recording_started(meeting_name: Option<String>) -> Self {
        Self::recording_started_for(meeting_name, Locale::En)
    }

    pub fn recording_started_for(meeting_name: Option<String>, locale: Locale) -> Self {
        let body = match meeting_name {
            Some(name) => match locale {
                Locale::En => format!("Recording started for meeting: {}", name),
                Locale::Fa => format!("ضبط جلسه «{}» آغاز شد", name),
            },
            None => locale
                .pick(
                    "Recording has started. Please inform others in the meeting that you are recording.",
                    "ضبط آغاز شد. لطفاً به دیگر حاضران جلسه اطلاع دهید که در حال ضبط هستید.",
                )
                .to_string(),
        };

        Notification::new("Matchwise", body, NotificationType::RecordingStarted)
            .with_priority(NotificationPriority::High)
            .with_timeout(NotificationTimeout::Seconds(5))
    }

    pub fn recording_stopped() -> Self {
        Self::recording_stopped_for(Locale::En)
    }

    pub fn recording_stopped_for(locale: Locale) -> Self {
        Notification::new(
            "Matchwise",
            locale.pick("Recording has been stopped and saved", "ضبط متوقف و ذخیره شد"),
            NotificationType::RecordingStopped
        )
        .with_priority(NotificationPriority::Normal)
        .with_timeout(NotificationTimeout::Seconds(3))
    }

    pub fn recording_paused() -> Self {
        Self::recording_paused_for(Locale::En)
    }

    pub fn recording_paused_for(locale: Locale) -> Self {
        Notification::new(
            "Matchwise",
            locale.pick("Recording has been paused", "ضبط موقتاً متوقف شد"),
            NotificationType::RecordingPaused
        )
        .with_priority(NotificationPriority::Normal)
        .with_timeout(NotificationTimeout::Seconds(3))
    }

    pub fn recording_resumed() -> Self {
        Self::recording_resumed_for(Locale::En)
    }

    pub fn recording_resumed_for(locale: Locale) -> Self {
        Notification::new(
            "Matchwise",
            locale.pick("Recording has been resumed", "ضبط از سر گرفته شد"),
            NotificationType::RecordingResumed
        )
        .with_priority(NotificationPriority::Normal)
        .with_timeout(NotificationTimeout::Seconds(3))
    }

    pub fn transcription_complete(file_path: Option<String>) -> Self {
        Self::transcription_complete_for(file_path, Locale::En)
    }

    pub fn transcription_complete_for(file_path: Option<String>, locale: Locale) -> Self {
        let body = match file_path {
            Some(path) => match locale {
                Locale::En => format!("Transcription completed and saved to: {}", path),
                Locale::Fa => format!("رونویسی کامل و در این مسیر ذخیره شد: {}", path),
            },
            None => locale
                .pick("Transcription has been completed", "رونویسی کامل شد")
                .to_string(),
        };

        Notification::new("Matchwise", body, NotificationType::TranscriptionComplete)
            .with_priority(NotificationPriority::Normal)
            .with_timeout(NotificationTimeout::Seconds(5))
    }

    pub fn meeting_reminder(minutes_until: u64, meeting_title: Option<String>) -> Self {
        Self::meeting_reminder_for(minutes_until, meeting_title, Locale::En)
    }

    pub fn meeting_reminder_for(
        minutes_until: u64,
        meeting_title: Option<String>,
        locale: Locale,
    ) -> Self {
        let body = match (meeting_title, locale) {
            (Some(title), Locale::En) => {
                format!("Meeting '{}' starts in {} minutes", title, minutes_until)
            }
            (None, Locale::En) => format!("Meeting starts in {} minutes", minutes_until),
            (Some(title), Locale::Fa) => {
                format!("جلسه «{}» تا {} دقیقه دیگر شروع می‌شود", title, minutes_until)
            }
            (None, Locale::Fa) => format!("جلسه تا {} دقیقه دیگر شروع می‌شود", minutes_until),
        };

        Notification::new("Matchwise", body, NotificationType::MeetingReminder(minutes_until))
            .with_priority(NotificationPriority::High)
            .with_timeout(NotificationTimeout::Seconds(10))
    }

    pub fn system_error(error: impl Into<String>) -> Self {
        Self::system_error_for(error, Locale::En)
    }

    pub fn system_error_for(error: impl Into<String>, locale: Locale) -> Self {
        let error_string = error.into();
        Notification::new(
            locale.pick("Matchwise Error", "خطای Matchwise"),
            error_string.clone(),
            NotificationType::SystemError(error_string)
        )
        .with_priority(NotificationPriority::Critical)
        .with_timeout(NotificationTimeout::Never)
    }

    pub fn test_notification() -> Self {
        Self::test_notification_for(Locale::En)
    }

    pub fn test_notification_for(locale: Locale) -> Self {
        Notification::new(
            "Matchwise",
            locale.pick(
                "This is a test notification to verify the system is working correctly",
                "این یک اعلان آزمایشی برای اطمینان از کارکرد درست سامانه است",
            ),
            NotificationType::Test
        )
        .with_priority(NotificationPriority::Normal)
        .with_timeout(NotificationTimeout::Seconds(5))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn persian_recording_notice_asks_to_inform_others() {
        let notification = Notification::recording_started_for(None, Locale::Fa);
        assert!(notification.body.contains("اطلاع دهید"));
        let english = Notification::recording_started(None);
        assert!(english.body.starts_with("Recording has started"));
    }

    #[test]
    fn persian_reminder_quotes_the_title() {
        let notification =
            Notification::meeting_reminder_for(5, Some("Demo".to_string()), Locale::Fa);
        assert!(notification.body.contains("«Demo»"));
        assert!(notification.body.contains('5'));
    }
}
