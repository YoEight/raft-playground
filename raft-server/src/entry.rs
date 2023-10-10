use bytes::Bytes;

#[derive(Clone)]
pub struct Entry {
    index: u64,
    term: u64,
    payload: Bytes,
}
