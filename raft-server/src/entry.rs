use bytes::Bytes;

#[derive(Clone)]
pub struct Entry {
    index: u64,
    term: u64,
    payload: Bytes,
}

#[derive(Clone, Default)]
pub struct Entries {
    inner: Vec<Entry>,
}

impl Entries {
    pub fn add(&mut self, entry: Entry) {
        self.inner.push(entry);
    }

    pub fn snapshot(&self) -> Snapshot {
        if let Some(last) = self.inner.last() {
            return Snapshot {
                term: last.term,
                index: self.inner.len() as u64 - 1,
            };
        }

        Snapshot::default()
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Snapshot {
    pub term: u64,
    pub index: u64,
}
