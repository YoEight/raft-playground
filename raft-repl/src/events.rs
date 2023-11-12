use ratatui_textarea::Input;

pub enum ReplEvent {
    Input(Input),
    Notification(Notification),
}

impl ReplEvent {
    pub fn msg(msg: impl AsRef<str>) -> Self {
        Self::Notification(Notification {
            msg: msg.as_ref().to_string(),
            r#type: NotificationType::Positive,
        })
    }

    pub fn error(msg: impl AsRef<str>) -> Self {
        Self::Notification(Notification {
            msg: msg.as_ref().to_string(),
            r#type: NotificationType::Negative,
        })
    }
}

pub struct Notification {
    pub msg: String,
    pub r#type: NotificationType,
}

pub enum NotificationType {
    Positive,
    Negative,
}
