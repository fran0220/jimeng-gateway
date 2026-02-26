use std::time::Instant;

use dashmap::DashMap;

/// Token-bucket rate limiter (per API key, in-memory).
pub struct RateLimiter {
    buckets: DashMap<String, TokenBucket>,
}

struct TokenBucket {
    tokens: f64,
    max_tokens: f64,
    refill_rate: f64, // tokens per second
    last_refill: Instant,
}

/// Result of a rate-limit check.
pub struct RateLimitResult {
    pub allowed: bool,
    pub limit: u32,
    pub remaining: u32,
    pub reset_secs: u32,
}

impl RateLimiter {
    pub fn new() -> Self {
        Self {
            buckets: DashMap::new(),
        }
    }

    /// Try to consume 1 token for the given key.
    /// `rate_limit` = max requests per minute (0 = unlimited).
    pub fn check(&self, key_id: &str, rate_limit: u32) -> RateLimitResult {
        if rate_limit == 0 {
            return RateLimitResult {
                allowed: true,
                limit: 0,
                remaining: 0,
                reset_secs: 0,
            };
        }

        let mut entry = self.buckets.entry(key_id.to_string()).or_insert_with(|| {
            TokenBucket {
                tokens: rate_limit as f64,
                max_tokens: rate_limit as f64,
                refill_rate: rate_limit as f64 / 60.0,
                last_refill: Instant::now(),
            }
        });

        let bucket = entry.value_mut();

        // Update max if rate_limit changed
        if (bucket.max_tokens - rate_limit as f64).abs() > 0.01 {
            bucket.max_tokens = rate_limit as f64;
            bucket.refill_rate = rate_limit as f64 / 60.0;
        }

        // Refill tokens
        let now = Instant::now();
        let elapsed = now.duration_since(bucket.last_refill).as_secs_f64();
        bucket.tokens = (bucket.tokens + elapsed * bucket.refill_rate).min(bucket.max_tokens);
        bucket.last_refill = now;

        if bucket.tokens >= 1.0 {
            bucket.tokens -= 1.0;
            let remaining = bucket.tokens as u32;
            let reset_secs = if remaining == 0 {
                (1.0 / bucket.refill_rate).ceil() as u32
            } else {
                0
            };
            RateLimitResult {
                allowed: true,
                limit: rate_limit,
                remaining,
                reset_secs,
            }
        } else {
            let wait = (1.0 - bucket.tokens) / bucket.refill_rate;
            RateLimitResult {
                allowed: false,
                limit: rate_limit,
                remaining: 0,
                reset_secs: wait.ceil() as u32,
            }
        }
    }

    /// Remove a key's bucket (e.g. on key deletion).
    pub fn remove(&self, key_id: &str) {
        self.buckets.remove(key_id);
    }
}
