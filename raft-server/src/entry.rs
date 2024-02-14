use bytes::Bytes;
use raft_common::Entry;

#[derive(Clone, Default)]
pub struct Entries {
    inner: Vec<Entry>,
    last_index: u64,
    last_term: u64,
}

impl Entries {
    pub fn add(&mut self, entry: Entry) {
        self.last_index = entry.index;
        self.last_term = entry.term;
        self.inner.push(entry);
    }

    pub fn append(&mut self, mut other: Vec<Entry>) {
        assert!(!other.is_empty());

        if let Some(last) = other.last() {
            self.last_index = last.index;
            self.last_term = last.term
        }

        self.inner.append(&mut other);
    }

    pub fn contains_log(&self, index: u64, term: u64) -> bool {
        if index == 0 && self.inner.is_empty() {
            return true;
        }

        for entry in &self.inner {
            if entry.index == index && entry.term == term {
                return true;
            }

            if entry.index > index {
                break;
            }
        }

        false
    }

    pub fn remove_uncommitted_entries_from(&mut self, index: u64, term: u64) {
        self.inner.retain(|entry| {
            if entry.index < index {
                return true;
            }

            entry.index >= index && entry.term == term
        });
    }

    pub fn last_index(&self) -> u64 {
        self.last_index
    }

    pub fn last_term(&self) -> u64 {
        self.last_term
    }

    pub fn entry_term(&self, index: u64) -> Option<u64> {
        for entry in &self.inner {
            if entry.index != index {
                continue;
            }

            return Some(entry.term);
        }

        None
    }

    pub fn get_previous_entry(&self, ref_index: u64) -> (u64, u64) {
        let mut index = 0u64;
        let mut term = 1u64;

        for entry in &self.inner {
            if entry.index == ref_index {
                break;
            }

            index = entry.index;
            term = entry.term;
        }

        (index, term)
    }

    pub fn read_entries_from(&self, index: u64, max_count: u64) -> Vec<Entry> {
        let mut count = 0;
        let mut batch = Vec::new();

        if self.last_index > index {
            for entry in &self.inner {
                if entry.index < index {
                    continue;
                }

                batch.push(entry.clone());
                count += 1;

                if count >= max_count {
                    break;
                }
            }
        }

        batch
    }

    pub fn entries(&self) -> &Vec<Entry> {
        &self.inner
    }
}

#[derive(Clone, Debug)]
pub struct RecordedEvent {
    pub stream_id: String,
    pub global: u64,
    pub revision: u64,
    pub payload: Bytes,
}
