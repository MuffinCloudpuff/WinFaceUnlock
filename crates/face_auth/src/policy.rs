#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AttemptPolicyConfig {
    pub required_consecutive_match_count: u32,
    pub failure_limit_before_cooldown: u32,
    pub cooldown_duration_ms: i64,
}

impl Default for AttemptPolicyConfig {
    fn default() -> Self {
        Self {
            required_consecutive_match_count: 2,
            failure_limit_before_cooldown: 3,
            cooldown_duration_ms: 30_000,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AttemptPolicyState {
    pub consecutive_match_count: u32,
    pub consecutive_failure_count: u32,
    pub cooldown_until_unix_ms: Option<i64>,
}

impl AttemptPolicyState {
    pub fn new() -> Self {
        Self {
            consecutive_match_count: 0,
            consecutive_failure_count: 0,
            cooldown_until_unix_ms: None,
        }
    }
}

impl Default for AttemptPolicyState {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AttemptPolicyDecision {
    AuthenticationAccepted,
    NeedMoreConsecutiveMatches,
    MatchRejectedBelowThreshold,
    CooldownActivated,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AttemptPolicy {
    config: AttemptPolicyConfig,
    state: AttemptPolicyState,
}

impl AttemptPolicy {
    pub fn new(config: AttemptPolicyConfig) -> Self {
        Self {
            config,
            state: AttemptPolicyState::new(),
        }
    }

    pub fn cooldown_is_active(&mut self, current_time_unix_ms: i64) -> bool {
        match self.state.cooldown_until_unix_ms {
            Some(cooldown_until_unix_ms) if current_time_unix_ms < cooldown_until_unix_ms => true,
            Some(_) => {
                self.state.cooldown_until_unix_ms = None;
                self.state.consecutive_failure_count = 0;
                false
            }
            None => false,
        }
    }

    pub fn record_match_result(
        &mut self,
        match_threshold_passed: bool,
        current_time_unix_ms: i64,
    ) -> AttemptPolicyDecision {
        if !match_threshold_passed {
            return self.record_failed_attempt(current_time_unix_ms);
        }

        self.state.consecutive_failure_count = 0;
        self.state.consecutive_match_count = self.state.consecutive_match_count.saturating_add(1);

        if self.state.consecutive_match_count >= self.config.required_consecutive_match_count {
            self.state.consecutive_match_count = 0;
            AttemptPolicyDecision::AuthenticationAccepted
        } else {
            AttemptPolicyDecision::NeedMoreConsecutiveMatches
        }
    }

    pub fn record_failed_attempt(&mut self, current_time_unix_ms: i64) -> AttemptPolicyDecision {
        self.state.consecutive_match_count = 0;
        self.state.consecutive_failure_count =
            self.state.consecutive_failure_count.saturating_add(1);

        if self.state.consecutive_failure_count >= self.config.failure_limit_before_cooldown {
            self.state.cooldown_until_unix_ms =
                Some(current_time_unix_ms + self.config.cooldown_duration_ms);
            AttemptPolicyDecision::CooldownActivated
        } else {
            AttemptPolicyDecision::MatchRejectedBelowThreshold
        }
    }

    pub fn reset_consecutive_matches(&mut self) {
        self.state.consecutive_match_count = 0;
    }

    pub fn state(&self) -> AttemptPolicyState {
        self.state
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn requires_configured_consecutive_matches_before_accepting() {
        let mut policy = AttemptPolicy::new(AttemptPolicyConfig {
            required_consecutive_match_count: 2,
            failure_limit_before_cooldown: 3,
            cooldown_duration_ms: 30_000,
        });

        let first = policy.record_match_result(true, 1_000);
        let second = policy.record_match_result(true, 1_001);

        assert_eq!(first, AttemptPolicyDecision::NeedMoreConsecutiveMatches);
        assert_eq!(second, AttemptPolicyDecision::AuthenticationAccepted);
    }

    #[test]
    fn activates_and_expires_cooldown_after_configured_failures() {
        let mut policy = AttemptPolicy::new(AttemptPolicyConfig {
            required_consecutive_match_count: 1,
            failure_limit_before_cooldown: 2,
            cooldown_duration_ms: 5_000,
        });

        assert_eq!(
            policy.record_failed_attempt(1_000),
            AttemptPolicyDecision::MatchRejectedBelowThreshold
        );
        assert_eq!(
            policy.record_failed_attempt(1_001),
            AttemptPolicyDecision::CooldownActivated
        );
        assert!(policy.cooldown_is_active(2_000));
        assert!(!policy.cooldown_is_active(6_001));
    }

    #[test]
    fn interrupted_sequence_resets_consecutive_matches_without_recording_failure() {
        let mut policy = AttemptPolicy::new(AttemptPolicyConfig {
            required_consecutive_match_count: 2,
            failure_limit_before_cooldown: 3,
            cooldown_duration_ms: 30_000,
        });

        assert_eq!(
            policy.record_match_result(true, 1_000),
            AttemptPolicyDecision::NeedMoreConsecutiveMatches
        );
        policy.reset_consecutive_matches();

        assert_eq!(
            policy.record_match_result(true, 1_001),
            AttemptPolicyDecision::NeedMoreConsecutiveMatches
        );
        assert_eq!(policy.state().consecutive_failure_count, 0);
    }
}
