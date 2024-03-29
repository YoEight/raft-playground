use crate::data::RecordedEvent;
use raft_common::StatusResp;
use tui_textarea::Input;

pub enum ReplEvent {
    Input(Input),
    Notification(Notification),
    NodeStatusReceived(NodeStatusEvent),
    StreamRead(StreamRead),
    StatusRead(StatusRead),
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

    pub fn node_connectivity(node: usize, status: Option<StatusResp>) -> Self {
        Self::NodeStatusReceived(NodeStatusEvent { node, status })
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

pub struct NodeStatusEvent {
    pub node: usize,
    pub status: Option<StatusResp>,
}

pub struct StreamRead {
    pub node: usize,
    pub stream: String,
    pub events: Vec<RecordedEvent>,
}

pub struct StatusRead {
    pub node: usize,
    pub status: Option<StatusResp>,
}
