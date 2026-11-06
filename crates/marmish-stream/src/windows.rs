//! Memory-bounded byte windows. Acceptance is not durable storage or pipe delivery.
use crate::{Error, MAX_BLOCK, MAX_RECORDS, Result, WINDOW_BYTES, end};
use sha2::{Digest, Sha256};
use std::collections::VecDeque;

struct Chunk {
    offset: u64,
    bytes: Vec<u8>,
}

/// Retains output until the peer acknowledges successful consumption.
#[derive(Default)]
pub struct SendWindow {
    chunks: VecDeque<Chunk>,
    produced: u64,
    acknowledged: u64,
    eof: Option<u64>,
}

impl SendWindow {
    pub fn produced(&self) -> u64 {
        self.produced
    }
    pub fn acknowledged(&self) -> u64 {
        self.acknowledged
    }
    pub fn eof(&self) -> Option<u64> {
        self.eof
    }
    pub fn buffered(&self) -> usize {
        (self.produced - self.acknowledged) as usize
    }
    pub fn credit(&self) -> usize {
        if self.eof.is_some() || self.chunks.len() == MAX_RECORDS {
            return 0;
        }
        WINDOW_BYTES - self.buffered()
    }
    pub fn push(&mut self, bytes: &[u8]) -> Result<u64> {
        if self.eof.is_some() {
            return Err(Error::Eof);
        }
        if bytes.is_empty() || bytes.len() > MAX_BLOCK {
            return Err(Error::Limit);
        }
        let next = end(self.produced, bytes.len())?;
        if bytes.len() > self.credit() {
            return Err(Error::Limit);
        }
        let offset = self.produced;
        self.chunks.push_back(Chunk {
            offset,
            bytes: bytes.to_vec(),
        });
        self.produced = next;
        Ok(offset)
    }
    pub fn read(&self, cursor: u64, limit: usize) -> Result<Vec<u8>> {
        if cursor < self.acknowledged || cursor > self.produced {
            return Err(Error::Offset);
        }
        if limit == 0 || limit > MAX_BLOCK {
            return Err(Error::Limit);
        }
        let mut bytes = Vec::new();
        for chunk in &self.chunks {
            let chunk_end = chunk.offset + chunk.bytes.len() as u64;
            if chunk_end <= cursor {
                continue;
            }
            let start = cursor.saturating_sub(chunk.offset) as usize;
            let count = (chunk.bytes.len() - start).min(limit - bytes.len());
            bytes.extend_from_slice(&chunk.bytes[start..start + count]);
            if bytes.len() == limit {
                break;
            }
        }
        Ok(bytes)
    }
    pub fn acknowledge(&mut self, cursor: u64) -> Result<()> {
        if cursor < self.acknowledged || cursor > self.produced {
            return Err(Error::Ack);
        }
        while let Some(front) = self.chunks.front_mut() {
            let chunk_end = front.offset + front.bytes.len() as u64;
            if chunk_end <= cursor {
                self.chunks.pop_front();
            } else {
                if cursor > front.offset {
                    front.bytes.drain(..(cursor - front.offset) as usize);
                    front.offset = cursor;
                }
                break;
            }
        }
        self.acknowledged = cursor;
        Ok(())
    }
    pub fn finish(&mut self, final_offset: u64) -> Result<()> {
        if final_offset != self.produced || self.eof.is_some_and(|n| n != final_offset) {
            return Err(Error::Eof);
        }
        self.eof = Some(final_offset);
        Ok(())
    }
    pub fn drained(&self) -> bool {
        self.eof == Some(self.acknowledged)
    }
}

struct Receipt {
    offset: u64,
    end: u64,
    sha256: [u8; 32],
}

/// Sequential input, with bounded duplicate receipts and separate delivery ACKs.
#[derive(Default)]
pub struct ReceiveWindow {
    queue: SendWindow,
    receipts: VecDeque<Receipt>,
}

impl ReceiveWindow {
    pub fn accepted(&self) -> u64 {
        self.queue.produced()
    }
    pub fn delivered(&self) -> u64 {
        self.queue.acknowledged()
    }
    pub fn buffered(&self) -> usize {
        self.queue.buffered()
    }
    pub fn credit(&self) -> usize {
        self.queue.credit()
    }
    pub fn eof(&self) -> Option<u64> {
        self.queue.eof()
    }
    /// True for a new accepted block, false for an exactly matching retained retry.
    pub fn accept(&mut self, offset: u64, bytes: &[u8]) -> Result<bool> {
        if bytes.is_empty() || bytes.len() > MAX_BLOCK {
            return Err(Error::Limit);
        }
        let next = end(offset, bytes.len())?;
        let sha256: [u8; 32] = Sha256::digest(bytes).into();
        if offset < self.accepted() {
            return match self.receipts.iter().find(|r| r.offset == offset) {
                Some(r) if r.end == next && r.sha256 == sha256 => Ok(false),
                Some(_) => Err(Error::Conflict),
                None => Err(Error::OldRetry),
            };
        }
        if offset > self.accepted() {
            return Err(Error::Gap);
        }
        if self.receipts.len() == MAX_RECORDS {
            if self
                .receipts
                .front()
                .is_none_or(|r| r.end > self.delivered())
            {
                return Err(Error::Limit);
            }
            // Reclaim only after all admission checks pass.
            if bytes.len() > self.credit() {
                return Err(Error::Limit);
            }
            self.queue.push(bytes)?;
            self.receipts.pop_front();
        } else {
            self.queue.push(bytes)?;
        }
        self.receipts.push_back(Receipt {
            offset,
            end: next,
            sha256,
        });
        Ok(true)
    }
    pub fn pending(&self, limit: usize) -> Result<Vec<u8>> {
        self.queue.read(self.delivered(), limit)
    }
    /// Call only after the bytes have actually been written to their destination.
    pub fn delivered_through(&mut self, cursor: u64) -> Result<()> {
        self.queue.acknowledge(cursor)
    }
    pub fn finish(&mut self, final_offset: u64) -> Result<()> {
        self.queue.finish(final_offset)
    }
    pub fn drained(&self) -> bool {
        self.queue.drained()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bounded_output_reclaims_only_consumed_prefix() {
        let mut w = SendWindow::default();
        for _ in 0..WINDOW_BYTES / MAX_BLOCK {
            w.push(&[7; MAX_BLOCK]).unwrap();
        }
        assert_eq!(w.credit(), 0);
        assert_eq!(w.push(b"x"), Err(Error::Limit));
        assert_eq!(w.read(3, 11).unwrap(), [7; 11]);
        w.acknowledge(3).unwrap();
        assert_eq!(w.buffered(), WINDOW_BYTES - 3);
        assert_eq!(w.read(0, 1), Err(Error::Offset));
        assert_eq!(w.acknowledge(2), Err(Error::Ack));
        assert_eq!(w.acknowledge(WINDOW_BYTES as u64 + 1), Err(Error::Ack));
        w.push(b"abc").unwrap();
        w.finish(w.produced()).unwrap();
        w.finish(w.produced()).unwrap();
        assert!(!w.drained());
        assert_eq!(w.push(b"x"), Err(Error::Eof));
        w.acknowledge(w.produced()).unwrap();
        assert!(w.drained());
    }

    #[test]
    fn exact_retry_conflict_gap_eof_and_partial_delivery() {
        let mut w = ReceiveWindow::default();
        assert_eq!(w.accept(0, b"hello"), Ok(true));
        assert_eq!(w.accept(0, b"hello"), Ok(false));
        assert_eq!(w.accept(0, b"hell!"), Err(Error::Conflict));
        assert_eq!(w.accept(1, b"ello"), Err(Error::OldRetry));
        assert_eq!(w.accept(6, b"!"), Err(Error::Gap));
        assert_eq!(w.finish(4), Err(Error::Eof));
        w.finish(5).unwrap();
        assert_eq!(w.accept(0, b"hello"), Ok(false));
        assert_eq!(w.accept(5, b"!"), Err(Error::Eof));
        w.delivered_through(2).unwrap();
        assert_eq!(w.pending(MAX_BLOCK).unwrap(), b"llo");
        assert_eq!((w.accepted(), w.delivered()), (5, 2));
        w.delivered_through(5).unwrap();
        assert!(w.drained());
    }

    #[test]
    fn receipt_memory_and_tiny_chunk_counts_are_bounded() {
        let mut w = ReceiveWindow::default();
        for n in 0..MAX_RECORDS {
            w.accept(n as u64, b"x").unwrap();
        }
        assert_eq!(w.credit(), 0);
        assert_eq!(w.accept(MAX_RECORDS as u64, b"x"), Err(Error::Limit));
        w.delivered_through(1).unwrap();
        w.accept(MAX_RECORDS as u64, b"y").unwrap();
        assert_eq!(w.accept(0, b"x"), Err(Error::OldRetry));
        assert_eq!(w.receipts.len(), MAX_RECORDS);
    }

    #[test]
    fn many_windows_remain_binary_exact_and_bounded() {
        let mut sender = SendWindow::default();
        let mut receiver = ReceiveWindow::default();
        let data: Vec<u8> = (0..MAX_BLOCK).map(|i| (i % 251) as u8).collect();
        for _ in 0..1024 {
            let offset = sender.push(&data).unwrap();
            let block = sender.read(offset, MAX_BLOCK).unwrap();
            receiver.accept(offset, &block).unwrap();
            assert_eq!(receiver.pending(MAX_BLOCK).unwrap(), data);
            receiver
                .delivered_through(offset + data.len() as u64)
                .unwrap();
            sender.acknowledge(receiver.accepted()).unwrap();
            assert!(receiver.buffered() <= MAX_BLOCK);
            assert_eq!(sender.buffered(), 0);
            assert!(receiver.receipts.len() <= MAX_RECORDS);
        }
    }

    #[test]
    fn boundary_failures_do_not_mutate_windows() {
        let mut sender = SendWindow {
            produced: crate::MAX_OFFSET,
            acknowledged: crate::MAX_OFFSET,
            ..Default::default()
        };
        assert_eq!(sender.push(b"x"), Err(Error::Offset));
        assert_eq!(sender.push(b""), Err(Error::Limit));
        assert_eq!(sender.produced(), crate::MAX_OFFSET);
        let mut receiver = ReceiveWindow::default();
        assert_eq!(receiver.accept(crate::MAX_OFFSET, b"x"), Err(Error::Offset));
        assert_eq!(receiver.buffered(), 0);
        receiver.finish(0).unwrap();
        assert!(receiver.drained());
        assert_eq!(receiver.accept(0, b"x"), Err(Error::Eof));
    }
}
