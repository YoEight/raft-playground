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
}
