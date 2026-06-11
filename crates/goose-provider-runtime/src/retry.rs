use serde::{Deserialize, Serialize};
use std::time::Duration;

pub const DEFAULT_MAX_RETRIES: usize = 3;
pub const DEFAULT_INITIAL_RETRY_INTERVAL_MS: u64 = 1000;
pub const DEFAULT_BACKOFF_MULTIPLIER: f64 = 2.0;
pub const DEFAULT_MAX_RETRY_INTERVAL_MS: u64 = 30_000;

#[derive(Debug, Clone)]
pub struct RetryConfig {
    /// Maximum number of retry attempts.
    max_retries: usize,
    /// Initial interval between retries in milliseconds.
    initial_interval_ms: u64,
    /// Multiplier for exponential backoff.
    backoff_multiplier: f64,
    /// Maximum interval between retries in milliseconds.
    max_interval_ms: u64,
    /// When true, hosts should only retry transient provider failures.
    transient_only: bool,
}

impl Default for RetryConfig {
    fn default() -> Self {
        Self {
            max_retries: DEFAULT_MAX_RETRIES,
            initial_interval_ms: DEFAULT_INITIAL_RETRY_INTERVAL_MS,
            backoff_multiplier: DEFAULT_BACKOFF_MULTIPLIER,
            max_interval_ms: DEFAULT_MAX_RETRY_INTERVAL_MS,
            transient_only: false,
        }
    }
}

impl RetryConfig {
    pub fn new(
        max_retries: usize,
        initial_interval_ms: u64,
        backoff_multiplier: f64,
        max_interval_ms: u64,
    ) -> Self {
        Self {
            max_retries,
            initial_interval_ms,
            backoff_multiplier,
            max_interval_ms,
            transient_only: false,
        }
    }

    pub fn transient_only(mut self) -> Self {
        self.transient_only = true;
        self
    }

    pub fn max_retries(&self) -> usize {
        self.max_retries
    }

    pub fn is_transient_only(&self) -> bool {
        self.transient_only
    }

    pub fn delay_for_attempt(&self, attempt: usize) -> Duration {
        if attempt == 0 {
            return Duration::from_millis(0);
        }

        let exponent = (attempt - 1) as u32;
        let base_delay_ms = (self.initial_interval_ms as f64
            * self.backoff_multiplier.powi(exponent as i32)) as u64;

        let capped_delay_ms = std::cmp::min(base_delay_ms, self.max_interval_ms);

        let jitter_factor_to_avoid_thundering_herd = 0.8 + (rand::random::<f64>() * 0.4);
        let jitter_delay_ms =
            (capped_delay_ms as f64 * jitter_factor_to_avoid_thundering_herd) as u64;

        Duration::from_millis(jitter_delay_ms)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ProviderRetryPolicy {
    pub max_attempts: u32,
    pub retry_transient_errors: bool,
    pub timeout_seconds: Option<u64>,
}

impl Default for ProviderRetryPolicy {
    fn default() -> Self {
        Self {
            max_attempts: 1,
            retry_transient_errors: true,
            timeout_seconds: None,
        }
    }
}

impl ProviderRetryPolicy {
    pub fn to_retry_config(&self) -> RetryConfig {
        let max_retries = self.max_attempts.saturating_sub(1) as usize;
        let config = RetryConfig::new(
            max_retries,
            DEFAULT_INITIAL_RETRY_INTERVAL_MS,
            DEFAULT_BACKOFF_MULTIPLIER,
            DEFAULT_MAX_RETRY_INTERVAL_MS,
        );

        if self.retry_transient_errors {
            config.transient_only()
        } else {
            config
        }
    }
}
