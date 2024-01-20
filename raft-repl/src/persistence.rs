use bytes::{BufMut, BytesMut};
use std::fs::{File, OpenOptions};
use std::io;
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::Path;

pub trait Persistence {
    fn load(&mut self) -> io::Result<Vec<String>>;
    fn persist(&mut self, history: &[String]) -> io::Result<()>;
}

#[derive(Clone, Copy, Eq, PartialEq, Ord, PartialOrd)]
pub struct Noop;

impl Persistence for Noop {
    fn load(&mut self) -> io::Result<Vec<String>> {
        Ok(Vec::new())
    }

    fn persist(&mut self, _: &[String]) -> io::Result<()> {
        Ok(())
    }
}

#[derive(Debug)]
pub struct FileBackend {
    file: File,
    buffer: BytesMut,
}

// Dumb implementation of a file-backed persistence layer.
impl FileBackend {
    pub fn new(path: impl AsRef<Path>) -> io::Result<Self> {
        let file = OpenOptions::new()
            .create(true)
            .read(true)
            .write(true)
            .open(path)?;

        Ok(FileBackend {
            file,
            buffer: BytesMut::new(),
        })
    }
}

impl Persistence for FileBackend {
    fn load(&mut self) -> io::Result<Vec<String>> {
        let mut buffer = String::new();
        let mut history = Vec::new();

        self.file.read_to_string(&mut buffer)?;

        for line in buffer.lines() {
            history.push(line.to_string());
        }

        Ok(history)
    }

    fn persist(&mut self, history: &[String]) -> io::Result<()> {
        self.file.seek(SeekFrom::Start(0))?;

        for line in history.iter() {
            self.buffer.extend_from_slice(line.as_bytes());
            self.buffer.put_u8(b'\n');
        }

        self.file.write_all(self.buffer.split().freeze().as_ref())?;

        Ok(())
    }
}
