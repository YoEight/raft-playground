use crate::node::Connectivity;
use ratatui_textarea::Input;

pub enum ReplEvent {
    Input(Input),
    Notification(Notification),
    NodeConnectivityChanged(NodeConnectivityEvent),
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

    pub fn node_connectivity(node: usize, connectivity: Connectivity) -> Self {
        Self::NodeConnectivityChanged(NodeConnectivityEvent { node, connectivity })
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

pub struct NodeConnectivityEvent {
    pub node: usize,
    pub connectivity: Connectivity,
}
