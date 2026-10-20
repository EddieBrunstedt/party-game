use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

/// Time for resends, handshake retries and simulated delay. The game uses [`Clock::real`];
/// tests use [`Clock::manual`] and advance it themselves. Clones share the same time.
#[derive(Debug, Clone)]
pub enum Clock {
    Real(Instant),
    Manual(Arc<AtomicU64>),
}

impl Clock {
    pub fn real() -> Self {
        Self::Real(Instant::now())
    }

    /// Starts at zero and only moves when [`Clock::advance`] is called.
    pub fn manual() -> Self {
        Self::Manual(Arc::new(AtomicU64::new(0)))
    }

    /// Time since the clock was made.
    pub fn now(&self) -> Duration {
        match self {
            Self::Real(start) => start.elapsed(),
            Self::Manual(micros) => Duration::from_micros(micros.load(Ordering::Relaxed)),
        }
    }

    /// Moves a manual clock forward.
    ///
    /// # Panics
    /// On a real clock.
    pub fn advance(&self, by: Duration) {
        match self {
            Self::Real(_) => panic!("a real clock can't be advanced"),
            Self::Manual(micros) => {
                micros.fetch_add(by.as_micros() as u64, Ordering::Relaxed);
            }
        }
    }
}
