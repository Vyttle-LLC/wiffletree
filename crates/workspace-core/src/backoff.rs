//! When to try a lost host connection again (design 11 of `remote-hosts`): after 1 s, doubling
//! to at most 30 s, each wait varied by up to 20% so clients that lost a host together do not
//! all come back at once. A network change or Try now tries at once and starts over at 1 s.
use std::time::{Duration, Instant};

const FIRST_RETRY: Duration = Duration::from_secs(1);
const LAST_RETRY: Duration = Duration::from_secs(30);
/// The most a wait varies, either way, as a share of it.
const JITTER: f64 = 0.2;

/// The next try at a lost connection.
#[derive(Clone, Debug)]
pub struct Backoff {
    /// The wait before the try after the next one fails, before jitter.
    delay: Duration,
    due: Instant,
}
impl Backoff {
    /// The connection dropped at `now`: the first try is a second away.
    pub fn dropped(now: Instant) -> Self {
        let mut backoff = Self {
            delay: FIRST_RETRY,
            due: now,
        };
        backoff.failed(now);
        backoff
    }
    pub fn due(&self) -> Instant {
        self.due
    }
    /// The try at `now` failed: wait the next delay.
    pub fn failed(&mut self, now: Instant) {
        self.due = now + jittered(self.delay);
        self.delay = (self.delay * 2).min(LAST_RETRY);
    }
    /// The network changed or the human pressed Try now: try at once, and wait 1 s again if
    /// that fails.
    pub fn try_now(&mut self, now: Instant) {
        self.due = now;
        self.delay = FIRST_RETRY;
    }
}

fn jittered(delay: Duration) -> Duration {
    let unit = (uuid::Uuid::new_v4().as_u128() % 1_000_001) as f64 / 1_000_000.0;
    delay.mul_f64(1.0 + JITTER * (2.0 * unit - 1.0))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn within_jitter(waited: Duration, expected: u64) -> bool {
        let (waited, expected) = (waited.as_secs_f64(), expected as f64);
        (waited - expected).abs() <= expected * JITTER + 1e-9
    }

    #[test]
    fn the_waits_double_from_one_second_to_thirty_within_the_jitter() {
        for _ in 0..50 {
            let mut now = Instant::now();
            let mut backoff = Backoff::dropped(now);
            for expected in [1, 2, 4, 8, 16, 30, 30] {
                let waited = backoff.due() - now;
                assert!(
                    within_jitter(waited, expected),
                    "{waited:?} for {expected} s"
                );
                now = backoff.due();
                backoff.failed(now);
            }
        }
    }

    #[test]
    fn a_network_change_or_try_now_tries_at_once_and_starts_over() {
        let mut now = Instant::now();
        let mut backoff = Backoff::dropped(now);
        for _ in 0..6 {
            now = backoff.due();
            backoff.failed(now);
        }
        assert!(backoff.due() - now > Duration::from_secs(20), "a long wait");

        now += Duration::from_secs(3);
        backoff.try_now(now);
        assert_eq!(backoff.due(), now, "at once");
        backoff.failed(now);
        assert!(within_jitter(backoff.due() - now, 1), "back to 1 s");
        now = backoff.due();
        backoff.failed(now);
        assert!(within_jitter(backoff.due() - now, 2));
    }
}
