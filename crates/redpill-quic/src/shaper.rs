//! Traffic shaping: token bucket rate limiter + adaptive shaper.

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

/// Token bucket rate limiter.
///
/// Tokens represent bytes. Uses x1000 fixed-point for sub-byte precision.
/// Thread-safe via atomics (lock-free).
pub struct TokenBucket {
    tokens: AtomicU64,
    rate: AtomicU64,
    burst: AtomicU64,
    last_refill: AtomicU64,
    epoch: Instant,
}

impl TokenBucket {
    /// Create a new token bucket. `rate` is bytes/sec, `burst` is max accumulated bytes.
    pub fn new(rate: u64, burst: u64) -> Self {
        let epoch = Instant::now();
        Self {
            tokens: AtomicU64::new(burst * 1000),
            rate: AtomicU64::new(rate),
            burst: AtomicU64::new(burst),
            last_refill: AtomicU64::new(0),
            epoch,
        }
    }

    /// Try to consume `bytes` tokens. Returns true if allowed.
    pub fn check(&self, bytes: usize) -> bool {
        if self.rate.load(Ordering::Relaxed) == 0 {
            return true; // unlimited
        }

        self.refill();

        let cost = (bytes as u64) * 1000;
        let mut current = self.tokens.load(Ordering::Relaxed);
        loop {
            if current < cost {
                return false;
            }
            match self.tokens.compare_exchange_weak(
                current,
                current - cost,
                Ordering::Relaxed,
                Ordering::Relaxed,
            ) {
                Ok(_) => return true,
                Err(c) => current = c,
            }
        }
    }

    fn refill(&self) {
        let now_micros = self.epoch.elapsed().as_micros() as u64;
        let last = self.last_refill.load(Ordering::Relaxed);
        let elapsed_micros = now_micros.saturating_sub(last);

        if elapsed_micros < 100 {
            return;
        }

        if self
            .last_refill
            .compare_exchange(last, now_micros, Ordering::Relaxed, Ordering::Relaxed)
            .is_err()
        {
            return;
        }

        let rate = self.rate.load(Ordering::Relaxed);
        let burst = self.burst.load(Ordering::Relaxed);
        let add = rate * elapsed_micros / 1000;
        let max = burst * 1000;

        let mut current = self.tokens.load(Ordering::Relaxed);
        loop {
            let new = (current + add).min(max);
            if new == current {
                break;
            }
            match self.tokens.compare_exchange_weak(
                current,
                new,
                Ordering::Relaxed,
                Ordering::Relaxed,
            ) {
                Ok(_) => break,
                Err(c) => current = c,
            }
        }
    }

    /// Update the rate and burst dynamically.
    pub fn set_rate(&self, rate_bytes_per_sec: u64) {
        let burst = if rate_bytes_per_sec > 0 {
            rate_bytes_per_sec / 10
        } else {
            0
        };
        self.rate.store(rate_bytes_per_sec, Ordering::Relaxed);
        self.burst.store(burst, Ordering::Relaxed);
        // Clamp tokens to new burst ceiling
        let max = burst * 1000;
        let mut current = self.tokens.load(Ordering::Relaxed);
        while current > max {
            match self.tokens.compare_exchange_weak(
                current,
                max,
                Ordering::Relaxed,
                Ordering::Relaxed,
            ) {
                Ok(_) => break,
                Err(c) => current = c,
            }
        }
    }
}

/// Rate limiter wrapping a token bucket with drop counters.
pub struct RateLimiter {
    bucket: TokenBucket,
    pub dropped_bytes: AtomicU64,
    pub dropped_packets: AtomicU64,
}

impl RateLimiter {
    /// Create a new rate limiter. `rate_mbps` = max Mbps, 0 = unlimited.
    pub fn new(rate_mbps: u64) -> Self {
        let rate_bytes = rate_mbps * 1_000_000 / 8;
        let burst = if rate_bytes > 0 { rate_bytes / 10 } else { 0 };
        Self {
            bucket: TokenBucket::new(rate_bytes, burst),
            dropped_bytes: AtomicU64::new(0),
            dropped_packets: AtomicU64::new(0),
        }
    }

    /// Check if a packet should pass. Increments drop counters on reject.
    pub fn check(&self, bytes: usize) -> bool {
        if self.bucket.check(bytes) {
            true
        } else {
            self.dropped_bytes
                .fetch_add(bytes as u64, Ordering::Relaxed);
            self.dropped_packets.fetch_add(1, Ordering::Relaxed);
            false
        }
    }

    pub fn set_rate(&self, rate_bytes_per_sec: u64) {
        self.bucket.set_rate(rate_bytes_per_sec);
    }
}

/// Adaptive shaper that adjusts rate based on queuing delay (RTT inflation).
///
/// Algorithm (run every 100ms):
/// - qdelay = current_rtt - base_rtt
/// - qdelay < 5ms → increase rate by 5%
/// - qdelay > 25ms → decrease rate by 10%
/// - Rate clamped to [1 Mbps, max_bandwidth_mbps]
pub struct AdaptiveShaper {
    rate_limiter: RateLimiter,
    base_rtt: AtomicU64,
    current_rate: AtomicU64,
    min_rate: u64,
    max_rate: u64,
    qdelay_low: Duration,
    qdelay_high: Duration,
}

impl AdaptiveShaper {
    /// Create a new adaptive shaper. `max_bandwidth_mbps` = 0 means unlimited.
    pub fn new(max_bandwidth_mbps: u64) -> Self {
        let max_rate = if max_bandwidth_mbps > 0 {
            max_bandwidth_mbps * 1_000_000 / 8
        } else {
            0
        };
        let min_rate = 1_000_000 / 8;

        let initial_rate = if max_rate > 0 { max_rate } else { 0 };

        Self {
            rate_limiter: RateLimiter::new(max_bandwidth_mbps),
            base_rtt: AtomicU64::new(u64::MAX),
            current_rate: AtomicU64::new(initial_rate),
            min_rate,
            max_rate,
            qdelay_low: Duration::from_millis(5),
            qdelay_high: Duration::from_millis(25),
        }
    }

    pub fn check(&self, bytes: usize) -> bool {
        self.rate_limiter.check(bytes)
    }

    /// Update with a new RTT measurement. Called periodically (every 100ms).
    pub fn update_rtt(&self, rtt: Duration) {
        if self.max_rate == 0 {
            return;
        }

        let rtt_us = rtt.as_micros() as u64;

        let mut base = self.base_rtt.load(Ordering::Relaxed);
        while rtt_us < base {
            match self.base_rtt.compare_exchange_weak(
                base,
                rtt_us,
                Ordering::Relaxed,
                Ordering::Relaxed,
            ) {
                Ok(_) => break,
                Err(c) => base = c,
            }
        }
        base = self.base_rtt.load(Ordering::Relaxed);
        if base == u64::MAX {
            return;
        }

        let qdelay = Duration::from_micros(rtt_us.saturating_sub(base));
        let current = self.current_rate.load(Ordering::Relaxed);

        let new_rate = if qdelay < self.qdelay_low {
            (current + current / 20).min(self.max_rate)
        } else if qdelay > self.qdelay_high {
            (current - current / 10).max(self.min_rate)
        } else {
            current
        };

        if new_rate != current {
            self.current_rate.store(new_rate, Ordering::Relaxed);
            self.rate_limiter.set_rate(new_rate);
        }
    }

    /// Get dropped packet/byte counts.
    pub fn dropped_packets(&self) -> u64 {
        self.rate_limiter.dropped_packets.load(Ordering::Relaxed)
    }

    pub fn dropped_bytes(&self) -> u64 {
        self.rate_limiter.dropped_bytes.load(Ordering::Relaxed)
    }

    /// Get current adaptive rate in bytes/sec.
    pub fn current_rate(&self) -> u64 {
        self.current_rate.load(Ordering::Relaxed)
    }

    /// Get base (minimum observed) RTT.
    pub fn base_rtt(&self) -> Option<Duration> {
        let base = self.base_rtt.load(Ordering::Relaxed);
        if base == u64::MAX {
            None
        } else {
            Some(Duration::from_micros(base))
        }
    }
}
