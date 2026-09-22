use std::time::Duration;

use ptmp::{Credentials, SessionConfig};

pub const DEFAULT_ADDR: &str = "127.0.0.1:39000";

pub mod env {
    pub const ADDR: &str = "PKTCTL_ADDR";
    pub const APP_ID: &str = "PKTCTL_APP_ID";
    pub const SECRET: &str = "PKTCTL_SECRET";
    pub const CALL_TIMEOUT_SECS: &str = "PKTCTL_CALL_TIMEOUT_SECS";
}

#[derive(Debug, Clone)]
pub struct Config {
    pub session: SessionConfig,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum ConfigError {
    #[error("{0} is not set; see docs/features/exapp-registration.md")]
    Missing(&'static str),
    #[error("{name} must be a positive number of seconds, got {value:?}")]
    InvalidTimeout { name: &'static str, value: String },
}

impl Config {
    pub fn from_env() -> Result<Self, ConfigError> {
        Self::from_lookup(|name| std::env::var(name).ok())
    }

    pub fn from_lookup(lookup: impl Fn(&str) -> Option<String>) -> Result<Self, ConfigError> {
        let required = |name: &'static str| {
            lookup(name)
                .filter(|value| !value.trim().is_empty())
                .ok_or(ConfigError::Missing(name))
        };

        let credentials = Credentials {
            app_id: required(env::APP_ID)?,
            secret: required(env::SECRET)?,
        };
        let addr = lookup(env::ADDR).unwrap_or_else(|| DEFAULT_ADDR.to_owned());
        let mut session = SessionConfig::new(addr, credentials);

        if let Some(raw) = lookup(env::CALL_TIMEOUT_SECS) {
            let seconds = raw
                .parse::<u64>()
                .ok()
                .filter(|seconds| *seconds > 0)
                .ok_or(ConfigError::InvalidTimeout {
                    name: env::CALL_TIMEOUT_SECS,
                    value: raw,
                })?;
            session.call_timeout = Duration::from_secs(seconds);
        }

        Ok(Self { session })
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;

    fn load(vars: &[(&str, &str)]) -> Result<Config, ConfigError> {
        let vars: HashMap<String, String> = vars
            .iter()
            .map(|(key, value)| ((*key).to_owned(), (*value).to_owned()))
            .collect();
        Config::from_lookup(|name| vars.get(name).cloned())
    }

    #[test]
    fn defaults_to_local_packet_tracer() {
        let config = load(&[(env::APP_ID, "app"), (env::SECRET, "key")]).unwrap();
        assert_eq!(config.session.addr, DEFAULT_ADDR);
        assert_eq!(config.session.credentials.app_id, "app");
    }

    #[test]
    fn requires_credentials() {
        assert_eq!(
            load(&[(env::APP_ID, "app")]).unwrap_err(),
            ConfigError::Missing(env::SECRET)
        );
        assert_eq!(
            load(&[(env::APP_ID, " "), (env::SECRET, "key")]).unwrap_err(),
            ConfigError::Missing(env::APP_ID)
        );
    }

    #[test]
    fn reads_optional_overrides() {
        let config = load(&[
            (env::APP_ID, "app"),
            (env::SECRET, "key"),
            (env::ADDR, "10.0.0.5:39001"),
            (env::CALL_TIMEOUT_SECS, "90"),
        ])
        .unwrap();
        assert_eq!(config.session.addr, "10.0.0.5:39001");
        assert_eq!(config.session.call_timeout, Duration::from_secs(90));
    }

    #[test]
    fn rejects_nonsense_timeouts() {
        for value in ["0", "-3", "soon"] {
            let error = load(&[
                (env::APP_ID, "app"),
                (env::SECRET, "key"),
                (env::CALL_TIMEOUT_SECS, value),
            ])
            .unwrap_err();
            assert!(matches!(error, ConfigError::InvalidTimeout { .. }));
        }
    }

    #[test]
    fn debug_output_never_leaks_the_secret() {
        let config = load(&[(env::APP_ID, "app"), (env::SECRET, "hunter2")]).unwrap();
        assert!(!format!("{config:?}").contains("hunter2"));
    }
}
