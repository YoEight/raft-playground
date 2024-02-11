use crate::data::RecordedEvent;
use crate::node::Connectivity;
use tui_textarea::Input;

pub enum ReplEvent {
    Input(Input),
    Notification(Notification),
    NodeConnectivityChanged(NodeConnectivityEvent),
    StreamRead(StreamRead),
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

    pub fn warn(msg: impl AsRef<str>) -> Self {
        Self::Notification(Notification {
            msg: msg.as_ref().to_string(),
            r#type: NotificationType::Warning,
        })
    }

    pub fn node_connectivity(node: usize, connectivity: Connectivity, external: bool) -> Self {
        Self::NodeConnectivityChanged(NodeConnectivityEvent {
            node,
            connectivity,
            external,
        })
    }

    pub fn stream_read(node: usize, stream: String, events: Vec<RecordedEvent>) -> Self {
        Self::StreamRead(StreamRead {
            node,
            stream,
            events,
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
    Warning,
}

pub struct NodeConnectivityEvent {
    pub node: usize,
    pub connectivity: Connectivity,
    pub external: bool,
}

pub struct StreamRead {
    pub node: usize,
    pub stream: String,
    pub events: Vec<RecordedEvent>,
}
