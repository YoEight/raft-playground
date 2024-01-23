use bytes::Bytes;

pub struct RecordedEvent {
    pub stream_id: String,
    pub global: u64,
    pub revision: u64,
    pub payload: Bytes,
}
