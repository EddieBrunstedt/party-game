use std::collections::BTreeMap;
use std::time::Duration;

use sim_core::Rng;

use crate::{Clock, Transport};

/// How badly [`Faulty`] treats outgoing datagrams.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Faults {
    pub loss_percent: u32,
    pub duplicate_percent: u32,
    /// Added to every datagram.
    pub delay: Duration,
    /// Extra random delay up to this much per datagram, which also reorders them.
    pub jitter: Duration,
}

/// Wraps any transport and loses, duplicates, delays and reorders what it sends, from a seed.
/// Tests wrap both ends; the game's `--net-delay` / `--net-loss` flags use it too.
pub struct Faulty<T: Transport> {
    inner: T,
    faults: Faults,
    rng: Rng,
    clock: Clock,
    /// Datagrams held back, by when they go out (and a counter, to keep equal times in order).
    pending: BTreeMap<(Duration, u64), (T::Addr, Vec<u8>)>,
    counter: u64,
}

impl<T: Transport> Faulty<T> {
    pub fn new(inner: T, faults: Faults, seed: u64, clock: Clock) -> Self {
        Self {
            inner,
            faults,
            rng: Rng::new(seed),
            clock,
            pending: BTreeMap::new(),
            counter: 0,
        }
    }

    pub fn inner(&self) -> &T {
        &self.inner
    }

    fn schedule(&mut self, to: T::Addr, datagram: &[u8]) {
        let jitter_micros = self.faults.jitter.as_micros() as u32;
        let jitter = Duration::from_micros(self.rng.range_u32(0, jitter_micros) as u64);
        let at = self.clock.now() + self.faults.delay + jitter;
        self.counter += 1;
        self.pending
            .insert((at, self.counter), (to, datagram.to_vec()));
    }

    fn flush(&mut self) {
        let now = self.clock.now();
        while let Some(entry) = self.pending.first_entry() {
            if entry.key().0 > now {
                break;
            }
            let (to, datagram) = entry.remove();
            self.inner.send(to, &datagram);
        }
    }
}

impl<T: Transport> Transport for Faulty<T> {
    type Addr = T::Addr;

    fn send(&mut self, to: T::Addr, datagram: &[u8]) {
        if !self.rng.chance(self.faults.loss_percent) {
            self.schedule(to, datagram);
            if self.rng.chance(self.faults.duplicate_percent) {
                self.schedule(to, datagram);
            }
        }
        self.flush();
    }

    fn receive(&mut self, into: &mut Vec<(T::Addr, Vec<u8>)>) {
        self.flush();
        self.inner.receive(into);
    }
}
