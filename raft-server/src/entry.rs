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
    last_index: u64,
    last_term: u64,
}

impl Entries {
    pub fn add(&mut self, entry: Entry) {
        self.last_index += 1;
        self.last_term = entry.term;
        self.inner.push(entry);
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

    pub fn last_index(&self) -> u64 {
        self.last_index
    }

    pub fn last_term(&self) -> u64 {
        self.last_term
    }
}
