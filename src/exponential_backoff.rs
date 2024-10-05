use std::cmp::min;

use tokio::time::Duration;

/// A handler for giving a configured duration on normal operation, or doing exponential
/// backoff from a given starting point on errors, up to a maximum value.
#[derive(Debug)]
pub struct ExponentialBackoff {
    base_duration: Duration,
    first_error_duration: Duration,
    max_error_duration: Duration,
    current_duration: Duration,
    is_error: bool,
}

impl ExponentialBackoff {
    pub fn new(
        base_duration: Duration,
        first_error_duration: Duration,
        max_error_duration: Duration,
    ) -> Self {
        ExponentialBackoff {
            base_duration,
            first_error_duration,
            max_error_duration,
            current_duration: base_duration,
            is_error: false,
        }
    }

    /// Returns whether the handler is currently in backoff mode or normal operation
    pub fn get_is_error(&self) -> bool {
        self.is_error
    }

    /// Gets the new duration to wait for, depending on any new success or failure
    pub fn get_current_duration(&self) -> Duration {
        self.current_duration
    }

    /// Tells the handler that the last operation was successful or not
    pub fn set_error(&mut self) {
        if !self.is_error {
            // Enter error mode
            self.is_error = true;
            self.current_duration = self.first_error_duration;
        } else if self.is_error {
            // Already in error mode, do the "exponential" part
            self.current_duration = min(self.current_duration * 2, self.max_error_duration);
        }
    }

    pub fn set_success(&mut self) {
        if self.is_error {
            // Exit error mode
            self.is_error = false;
            self.current_duration = self.base_duration;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reports_error_mode_correctly() {
        let mut eb = ExponentialBackoff::new(
            Duration::from_secs(10),
            Duration::from_secs(1),
            Duration::from_secs(20),
        );
        assert!(!eb.get_is_error());
        eb.set_error();
        assert!(eb.get_is_error());
        eb.set_success();
        assert!(!eb.get_is_error());
    }

    #[test]
    fn first_duration_is_normal() {
        let eb = ExponentialBackoff::new(
            Duration::from_secs(10),
            Duration::from_secs(1),
            Duration::from_secs(20),
        );
        assert_eq!(eb.get_current_duration(), Duration::from_secs(10));
    }

    #[test]
    fn stays_normal_after_success() {
        let mut eb = ExponentialBackoff::new(
            Duration::from_secs(10),
            Duration::from_secs(1),
            Duration::from_secs(20),
        );
        eb.set_success();
        assert_eq!(eb.get_current_duration(), Duration::from_secs(10));
    }

    #[test]
    fn moves_to_error_mode_on_failure() {
        let mut eb = ExponentialBackoff::new(
            Duration::from_secs(10),
            Duration::from_secs(1),
            Duration::from_secs(20),
        );
        eb.set_error();
        assert_eq!(eb.get_current_duration(), Duration::from_secs(1));
    }

    #[test]
    fn performs_exp_backoff() {
        let mut eb = ExponentialBackoff::new(
            Duration::from_secs(10),
            Duration::from_secs(1),
            Duration::from_secs(20),
        );
        eb.set_error();
        eb.set_error();
        assert_eq!(eb.get_current_duration(), Duration::from_secs(2));
    }

    #[test]
    fn stays_at_max_error_duration() {
        let mut eb = ExponentialBackoff::new(
            Duration::from_secs(10),
            Duration::from_secs(1),
            Duration::from_secs(20),
        );
        eb.set_error();
        assert_eq!(eb.get_current_duration(), Duration::from_secs(1));
        eb.set_error();
        assert_eq!(eb.get_current_duration(), Duration::from_secs(2));
        eb.set_error();
        assert_eq!(eb.get_current_duration(), Duration::from_secs(4));
        eb.set_error();
        assert_eq!(eb.get_current_duration(), Duration::from_secs(8));
        eb.set_error();
        assert_eq!(eb.get_current_duration(), Duration::from_secs(16));
        eb.set_error();
        assert_eq!(eb.get_current_duration(), Duration::from_secs(20)); // Not 32, we hit max
        eb.set_error();
        assert_eq!(eb.get_current_duration(), Duration::from_secs(20)); // Still at max
    }

    #[test]
    fn resumes_normal_after_new_success() {
        let mut eb = ExponentialBackoff::new(
            Duration::from_secs(10),
            Duration::from_secs(1),
            Duration::from_secs(20),
        );
        eb.set_error();
        eb.set_error();
        eb.set_success();
        assert_eq!(eb.get_current_duration(), Duration::from_secs(10));
    }
}
