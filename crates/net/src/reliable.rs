//! A reliable, ordered message stream to one peer over lossy datagrams: messages are split into
//! numbered fragments, the receiver acks what it has (cumulatively plus a 32-fragment bitmask),
//! and the sender resends anything unacked after a timeout learned from round trips.
//!
//! Sequence numbers are `u32` and never wrap in practice (a fragment a frame for two years).

use std::collections::{BTreeMap, VecDeque};
use std::time::Duration;

use crate::frame::Frame;

/// Payload bytes per fragment, small enough that a datagram never fragments at the IP layer.
pub(crate) const FRAGMENT: usize = 1024;
/// Fragments in flight at once; the rest wait. The receiver ignores fragments further ahead.
const WINDOW: u32 = 256;
const INITIAL_RTO: Duration = Duration::from_millis(200);
const MIN_RTO: Duration = Duration::from_millis(40);
const MAX_RTO: Duration = Duration::from_secs(2);

pub(crate) struct Channel {
    next_seq: u32,
    /// Fragments not yet sent because the window is full.
    waiting: VecDeque<(bool, Vec<u8>)>,
    in_flight: BTreeMap<u32, InFlight>,
    smoothed_rtt: Option<Duration>,
    rto: Duration,

    expected: u32,
    /// Fragments that arrived ahead of `expected`.
    early: BTreeMap<u32, (bool, Vec<u8>)>,
    /// The message being reassembled from in-order fragments.
    partial: Vec<u8>,
}

struct InFlight {
    last: bool,
    bytes: Vec<u8>,
    first_sent: Duration,
    sent_at: Duration,
    resent: bool,
}

impl Default for Channel {
    fn default() -> Self {
        Self {
            next_seq: 0,
            waiting: VecDeque::new(),
            in_flight: BTreeMap::new(),
            smoothed_rtt: None,
            rto: INITIAL_RTO,
            expected: 0,
            early: BTreeMap::new(),
            partial: Vec::new(),
        }
    }
}

impl Channel {
    /// Queues a message; [`Channel::transmit`] sends it.
    pub(crate) fn push(&mut self, message: &[u8]) {
        let mut chunks = message.chunks(FRAGMENT).peekable();
        if chunks.peek().is_none() {
            self.waiting.push_back((true, Vec::new()));
        }
        while let Some(chunk) = chunks.next() {
            self.waiting
                .push_back((chunks.peek().is_none(), chunk.to_vec()));
        }
    }

    /// The fragments to send now: new ones while the window has room, and any whose resend
    /// timeout has passed.
    pub(crate) fn transmit(&mut self, now: Duration) -> Vec<Frame> {
        let mut frames = Vec::new();
        let mut timed_out = false;
        for (&seq, fragment) in &mut self.in_flight {
            if now >= fragment.sent_at + self.rto {
                fragment.sent_at = now;
                fragment.resent = true;
                timed_out = true;
                frames.push(Frame::Data {
                    seq,
                    last: fragment.last,
                    bytes: fragment.bytes.clone(),
                });
            }
        }
        if timed_out {
            self.rto = (self.rto * 2).min(MAX_RTO);
        }
        while self.in_flight.len() < WINDOW as usize {
            let Some((last, bytes)) = self.waiting.pop_front() else {
                break;
            };
            let seq = self.next_seq;
            self.next_seq += 1;
            frames.push(Frame::Data {
                seq,
                last,
                bytes: bytes.clone(),
            });
            self.in_flight.insert(
                seq,
                InFlight {
                    last,
                    bytes,
                    first_sent: now,
                    sent_at: now,
                    resent: false,
                },
            );
        }
        frames
    }

    /// Takes in a fragment. Returns the ack to send back (always, so a lost ack is repaired by
    /// the next resend) and every message this fragment completed.
    pub(crate) fn receive(
        &mut self,
        seq: u32,
        last: bool,
        bytes: Vec<u8>,
    ) -> (Frame, Vec<Vec<u8>>) {
        if seq >= self.expected && seq < self.expected + WINDOW {
            self.early.entry(seq).or_insert((last, bytes));
        }
        let mut complete = Vec::new();
        while let Some((last, bytes)) = self.early.remove(&self.expected) {
            self.expected += 1;
            self.partial.extend_from_slice(&bytes);
            if last {
                complete.push(std::mem::take(&mut self.partial));
            }
        }
        let beyond = (0..32)
            .filter(|i| self.early.contains_key(&(self.expected + 1 + i)))
            .fold(0, |bits, i| bits | 1 << i);
        let ack = Frame::Ack {
            next: self.expected,
            beyond,
        };
        (ack, complete)
    }

    pub(crate) fn acknowledge(&mut self, now: Duration, next: u32, beyond: u32) {
        let acked: Vec<u32> = self
            .in_flight
            .keys()
            .copied()
            .filter(|&seq| {
                seq < next
                    || (seq > next && seq - next - 1 < 32 && beyond & 1 << (seq - next - 1) != 0)
            })
            .collect();
        for seq in acked {
            let fragment = self.in_flight.remove(&seq).expect("listed above");
            // Karn's rule: a resent fragment's ack can't tell which copy it answers.
            if !fragment.resent {
                self.sample_rtt(now.saturating_sub(fragment.first_sent));
            }
        }
    }

    fn sample_rtt(&mut self, sample: Duration) {
        let smoothed = match self.smoothed_rtt {
            None => sample,
            Some(rtt) => (rtt * 7 + sample) / 8,
        };
        self.smoothed_rtt = Some(smoothed);
        self.rto = (smoothed * 2 + Duration::from_millis(10)).clamp(MIN_RTO, MAX_RTO);
    }

    /// Nothing waiting or unacked.
    #[cfg(test)]
    pub(crate) fn idle(&self) -> bool {
        self.waiting.is_empty() && self.in_flight.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn data(frame: Frame) -> (u32, bool, Vec<u8>) {
        match frame {
            Frame::Data { seq, last, bytes } => (seq, last, bytes),
            _ => panic!("expected a data frame"),
        }
    }

    fn ack(frame: &Frame) -> (u32, u32) {
        match frame {
            Frame::Ack { next, beyond } => (*next, *beyond),
            _ => panic!("expected an ack"),
        }
    }

    #[test]
    fn splits_large_messages_and_reassembles_them() {
        let message: Vec<u8> = (0..FRAGMENT * 2 + 10).map(|i| i as u8).collect();
        let mut sender = Channel::default();
        let mut receiver = Channel::default();
        sender.push(&message);
        let frames = sender.transmit(Duration::ZERO);
        assert_eq!(frames.len(), 3);
        let mut got = Vec::new();
        for frame in frames {
            let (seq, last, bytes) = data(frame);
            got.extend(receiver.receive(seq, last, bytes).1);
        }
        assert_eq!(got, vec![message]);
    }

    #[test]
    fn empty_messages_still_arrive() {
        let mut sender = Channel::default();
        let mut receiver = Channel::default();
        sender.push(&[]);
        let (seq, last, bytes) = data(sender.transmit(Duration::ZERO).remove(0));
        assert_eq!(receiver.receive(seq, last, bytes).1, vec![Vec::<u8>::new()]);
    }

    #[test]
    fn out_of_order_fragments_wait_and_are_acked_selectively() {
        let mut sender = Channel::default();
        let mut receiver = Channel::default();
        for i in 0..3u8 {
            sender.push(&[i]);
        }
        let frames: Vec<_> = sender
            .transmit(Duration::ZERO)
            .into_iter()
            .map(data)
            .collect();

        let (third_ack, got) = receiver.receive(frames[2].0, frames[2].1, frames[2].2.clone());
        assert!(got.is_empty());
        assert_eq!(ack(&third_ack), (0, 0b10));

        sender.acknowledge(Duration::from_millis(5), 0, 0b10);
        assert_eq!(sender.in_flight.keys().copied().collect::<Vec<_>>(), [0, 1]);

        let (_, got) = receiver.receive(frames[0].0, frames[0].1, frames[0].2.clone());
        assert_eq!(got, vec![vec![0]]);
        let (last_ack, got) = receiver.receive(frames[1].0, frames[1].1, frames[1].2.clone());
        assert_eq!(got, vec![vec![1], vec![2]]);
        assert_eq!(ack(&last_ack), (3, 0));
    }

    #[test]
    fn duplicates_are_delivered_once() {
        let mut sender = Channel::default();
        let mut receiver = Channel::default();
        sender.push(b"once");
        let (seq, last, bytes) = data(sender.transmit(Duration::ZERO).remove(0));
        assert_eq!(receiver.receive(seq, last, bytes.clone()).1.len(), 1);
        assert!(receiver.receive(seq, last, bytes).1.is_empty());
    }

    #[test]
    fn unacked_fragments_are_resent_with_backoff() {
        let mut sender = Channel::default();
        sender.push(b"x");
        assert_eq!(sender.transmit(Duration::ZERO).len(), 1);
        assert!(sender.transmit(INITIAL_RTO / 2).is_empty());
        assert_eq!(sender.transmit(INITIAL_RTO).len(), 1);
        // The timeout doubled, so the next resend comes 2 × the first timeout later.
        assert!(sender.transmit(INITIAL_RTO * 2).is_empty());
        assert_eq!(sender.transmit(INITIAL_RTO * 3).len(), 1);
        sender.acknowledge(INITIAL_RTO * 3, 1, 0);
        assert!(sender.idle());
    }

    #[test]
    fn the_window_holds_back_fragments_until_acked() {
        let mut sender = Channel::default();
        for _ in 0..WINDOW + 5 {
            sender.push(b"m");
        }
        assert_eq!(sender.transmit(Duration::ZERO).len(), WINDOW as usize);
        sender.acknowledge(Duration::from_millis(1), 5, 0);
        assert_eq!(sender.transmit(Duration::from_millis(1)).len(), 5);
    }

    #[test]
    fn round_trips_set_the_resend_timeout() {
        let mut sender = Channel::default();
        for i in 0..20 {
            let now = Duration::from_millis(i * 100);
            sender.push(b"m");
            sender.transmit(now);
            sender.acknowledge(now + Duration::from_millis(30), i as u32 + 1, 0);
        }
        assert!(sender.rto >= Duration::from_millis(60) && sender.rto <= Duration::from_millis(80));
    }
}
