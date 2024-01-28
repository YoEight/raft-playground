use crate::persistence::{FileBackend, Noop, Persistence};
use std::io;
use std::path::Path;

#[derive(Debug)]
pub struct History<A> {
    entries: Vec<String>,
    offset: usize,
    persistence: A,
    temp: String,
}

#[allow(dead_code)]
pub fn in_memory_history() -> io::Result<History<Noop>> {
    History::new(Noop)
}

pub fn file_backed_history(path: impl AsRef<Path>) -> io::Result<History<FileBackend>> {
    let backend = FileBackend::new(path)?;
    History::new(backend)
}

impl<A: Persistence> History<A> {
    pub fn new(mut persistence: A) -> io::Result<Self> {
        let entries = persistence.load()?;
        let offset = entries.len();

        Ok(History {
            temp: String::new(),
            entries,
            offset,
            persistence,
        })
    }

    pub fn push(&mut self, entry: String) -> io::Result<()> {
        if self.entries.last() != Some(&entry) {
            self.entries.push(entry);
            self.persistence.persist(&self.entries)?;
        }

        self.offset = self.entries.len();
        self.temp.clear();

        Ok(())
    }

    /// Are we at the top of the past entries.
    pub fn offset_is_head(&self) -> bool {
        self.offset == self.entries.len()
    }

    pub fn prev_entry(&mut self) -> Option<String> {
        if self.entries.is_empty() {
            return None;
        }

        if self.offset == 1 && self.entries.len() == 1 {
            return self.entries.first().cloned();
        }

        if self.offset >= 1 {
            self.offset -= 1;
        }

        Some(
            self.entries
                .get(self.offset)
                .cloned()
                .expect("My maintainer miscalculated the history offset for prev_entry"),
        )
    }

    pub fn next_entry(&mut self) -> Option<String> {
        if self.entries.is_empty() || self.offset == self.entries.len() {
            return None;
        }

        self.offset += 1;

        if self.offset == self.entries.len() {
            return None;
        }

        Some(
            self.entries
                .get(self.offset)
                .cloned()
                .expect("My maintainer miscalculated the history offset for next_entry"),
        )
    }

    #[allow(dead_code)]
    pub fn entries(&self) -> &Vec<String> {
        &self.entries
    }
}
