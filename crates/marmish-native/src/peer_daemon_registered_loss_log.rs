//! Fixed projection of a private dispatcher child log.
use super::{
    super::{EDGE_STAGE, Edge, Refusal},
    ChildEnd,
};
use std::{fs::File, io, os::unix::fs::FileExt};

pub(super) const CAP: usize = 128 * 1024;
pub(super) const OWNED_FD_LINE: &[u8] =
    b"fatal runtime error: IO Safety violation: owned file descriptor already closed, aborting";

#[derive(Debug, PartialEq, Eq)]
pub(super) enum ReadState {
    Unreadable,
    Empty,
    Complete,
    Truncated,
}

#[derive(Debug, PartialEq, Eq)]
pub(super) enum AbortClass {
    NotAborted,
    Unknown,
    Unrecognized,
    OwnedFdClosed,
}

#[derive(Debug, PartialEq, Eq)]
pub(super) struct Observation {
    pub(super) state: ReadState,
    pub(super) known: Vec<&'static str>,
    pub(super) context: bool,
    pub(super) edges: Vec<Edge>,
    pub(super) out_of_order: bool,
    pub(super) unclassified_tail: bool,
    pub(super) abort: AbortClass,
}

pub(super) fn observe(log: &File, end: ChildEnd) -> Observation {
    let mut buffer = vec![0; CAP + 1];
    let bytes = read_prefix(log, &mut buffer).map(|len| &buffer[..len]);
    project(bytes, end)
}

fn read_prefix(log: &File, buffer: &mut [u8]) -> io::Result<usize> {
    let mut offset = 0;
    while offset < buffer.len() {
        let count = log.read_at(&mut buffer[offset..], offset as u64)?;
        if count == 0 {
            break;
        }
        offset += count;
    }
    Ok(offset)
}

pub(super) fn project(bytes: io::Result<&[u8]>, end: ChildEnd) -> Observation {
    let state = match bytes.as_ref().map(|bytes| bytes.len()) {
        Err(_) => ReadState::Unreadable,
        Ok(0) => ReadState::Empty,
        Ok(len) if len > CAP => ReadState::Truncated,
        Ok(_) => ReadState::Complete,
    };
    let mut result = Observation {
        abort: match (end, &state) {
            (ChildEnd::Signaled(libc::SIGABRT), ReadState::Truncated | ReadState::Unreadable) => {
                AbortClass::Unknown
            }
            (ChildEnd::Signaled(libc::SIGABRT), _) => AbortClass::Unrecognized,
            _ => AbortClass::NotAborted,
        },
        state,
        known: Vec::new(),
        context: false,
        edges: Vec::new(),
        out_of_order: false,
        unclassified_tail: false,
    };
    let Ok(bytes) = bytes else { return result };
    for line in bytes[..bytes.len().min(CAP)].split_inclusive(|&byte| byte == b'\n') {
        let Some(line) = line.strip_suffix(b"\n") else {
            break;
        };
        result.add_line(line);
    }
    result
}

impl Observation {
    fn add_line(&mut self, line: &[u8]) {
        if line == OWNED_FD_LINE {
            if self.abort != AbortClass::NotAborted {
                self.abort = AbortClass::OwnedFdClosed;
            }
            return;
        }
        match serde_json::from_slice::<serde_json::Value>(line) {
            Ok(entry) if entry["schema"].as_u64() == Some(1) => self.add_event(&entry),
            Ok(_) => {}
            Err(_) => {
                if (self.context || !self.edges.is_empty())
                    && std::str::from_utf8(line).is_ok_and(|text| !text.trim().is_empty())
                {
                    self.unclassified_tail = true;
                }
            }
        }
    }

    fn add_event(&mut self, entry: &serde_json::Value) {
        match entry["event"].as_str() {
            Some("context") => {
                self.context = true;
                self.unclassified_tail = false;
            }
            Some("diagnostic") => {
                let stage = entry["stage"].as_str();
                let category = entry["category"].as_str();
                if stage == Some(EDGE_STAGE)
                    && let Some(edge) = Edge::ALL
                        .into_iter()
                        .find(|edge| Some(edge.code()) == category)
                {
                    self.unclassified_tail = false;
                    if !self.edges.contains(&edge) {
                        self.out_of_order |= self.edges.last().is_some_and(|&last| edge < last);
                        self.edges.push(edge);
                    }
                } else if let Some(category) = known_category(stage, category)
                    && !self.known.contains(&category)
                {
                    self.known.push(category);
                }
            }
            _ => {}
        }
    }
}

fn known_category(stage: Option<&str>, category: Option<&str>) -> Option<&'static str> {
    Some(match (stage?, category?) {
        ("registered_shutdown", "native_startup_failed") => "native_startup_failed",
        ("registered_shutdown", "owned_stop_failed") => "owned_stop_failed",
        ("registered_shutdown", "owned_stop_unproved") => "owned_stop_unproved",
        ("registered_shutdown", "owned_stop_incomplete") => "owned_stop_incomplete",
        ("registered_shutdown", "registration_indeterminate") => "registration_indeterminate",
        ("registered_fault_fixture", category) => Refusal::from_bytes(category.as_bytes())?.code(),
        _ => return None,
    })
}
