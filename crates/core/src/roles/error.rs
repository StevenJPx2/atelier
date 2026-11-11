use std::fmt;

/// How a plugin's call failed, which decides whether atelier retries it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Failure {
    /// A network blip or a busy service: the next poll tries again.
    Transient,
    /// The service asked atelier to wait this many seconds.
    RateLimited { retry_after_secs: u64 },
    /// Retrying cannot help, such as a missing branch or a refused sign-in.
    Permanent,
}

/// A plugin's failure, with a message for the log.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginError {
    pub failure: Failure,
    pub message: String,
}

impl PluginError {
    #[must_use]
    pub fn transient(message: impl Into<String>) -> Self {
        Self {
            failure: Failure::Transient,
            message: message.into(),
        }
    }

    #[must_use]
    pub fn permanent(message: impl Into<String>) -> Self {
        Self {
            failure: Failure::Permanent,
            message: message.into(),
        }
    }

    #[must_use]
    pub const fn is_permanent(&self) -> bool {
        matches!(self.failure, Failure::Permanent)
    }
}

impl fmt::Display for PluginError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for PluginError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_permanent_failure_is_final_and_the_message_is_the_error() {
        let limited = PluginError {
            failure: Failure::RateLimited {
                retry_after_secs: 60,
            },
            message: "slow down".into(),
        };

        assert!(PluginError::permanent("gone").is_permanent());
        assert!(!PluginError::transient("blip").is_permanent());
        assert!(!limited.is_permanent());
        assert_eq!(limited.to_string(), "slow down");
    }
}
