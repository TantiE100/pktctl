use std::{path::PathBuf, time::Duration};

use ptmp::{Credentials, SessionConfig};

pub const DEFAULT_ADDR: &str = "127.0.0.1:39000";
/// Without `PKTCTL_ADDR`, the ports tried in order. Packet Tracer listens on 39000, or
/// on the next free port when 39000 is still taken, for example right after a crash.
pub const DEFAULT_PORTS: std::ops::RangeInclusive<u16> = 39000..=39009;
const DEFAULT_HOST: &str = "127.0.0.1";

pub mod env {
    pub const ADDR: &str = "PKTCTL_ADDR";
    pub const APP_ID: &str = "PKTCTL_APP_ID";
    pub const SECRET: &str = "PKTCTL_SECRET";
    pub const CALL_TIMEOUT_SECS: &str = "PKTCTL_CALL_TIMEOUT_SECS";
    pub const PT_HOME: &str = "PKTCTL_PT_HOME";
    pub const SETUP_DIR: &str = "PKTCTL_SETUP_DIR";
    pub const HOME: &str = "HOME";
    pub const USER_PROFILE: &str = "USERPROFILE";
}

#[derive(Debug, Clone)]
pub struct SetupSettings {
    pub credentials: Credentials,
    pub packet_tracer_home: Option<PathBuf>,
    pub output_dir: PathBuf,
}

#[derive(Debug, Clone)]
pub struct Config {
    pub session: SessionConfig,
    pub setup: SetupSettings,
    /// Where to look for Packet Tracer, in order: `PKTCTL_ADDR` alone when set,
    /// otherwise every port of `DEFAULT_PORTS` on this computer.
    pub addresses: Vec<String>,
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
        let addresses = lookup(env::ADDR)
            .map(|addr| addr.trim().to_owned())
            .filter(|addr| !addr.is_empty())
            .map_or_else(
                || {
                    DEFAULT_PORTS
                        .map(|port| format!("{DEFAULT_HOST}:{port}"))
                        .collect()
                },
                |addr| vec![addr],
            );
        let addr = addresses[0].clone();
        let setup = SetupSettings {
            credentials: credentials.clone(),
            packet_tracer_home: lookup(env::PT_HOME).map(PathBuf::from),
            output_dir: lookup(env::SETUP_DIR).map_or_else(
                || {
                    lookup(env::HOME)
                        .or_else(|| lookup(env::USER_PROFILE))
                        .map_or_else(std::env::temp_dir, PathBuf::from)
                        .join(".config")
                        .join("pktctl")
                },
                PathBuf::from,
            ),
        };
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

        Ok(Self {
            session,
            setup,
            addresses,
        })
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
        assert_eq!(config.addresses.len(), DEFAULT_PORTS.len());
        assert_eq!(config.addresses[0], DEFAULT_ADDR);
        assert_eq!(config.addresses[9], "127.0.0.1:39009");
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
        assert_eq!(config.addresses, ["10.0.0.5:39001"]);
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
    fn setup_files_go_under_the_home_config_folder() {
        let config = load(&[
            (env::APP_ID, "app"),
            (env::SECRET, "key"),
            (env::HOME, "/Users/student"),
        ])
        .unwrap();
        assert_eq!(
            config.setup.output_dir,
            PathBuf::from("/Users/student/.config/pktctl")
        );
        assert_eq!(config.setup.packet_tracer_home, None);

        let custom = load(&[
            (env::APP_ID, "app"),
            (env::SECRET, "key"),
            (env::SETUP_DIR, "/srv/pktctl"),
            (env::PT_HOME, "/opt/pt"),
        ])
        .unwrap();
        assert_eq!(custom.setup.output_dir, PathBuf::from("/srv/pktctl"));
        assert_eq!(
            custom.setup.packet_tracer_home,
            Some(PathBuf::from("/opt/pt"))
        );
    }

    #[test]
    fn debug_output_never_leaks_the_secret() {
        let config = load(&[(env::APP_ID, "app"), (env::SECRET, "hunter2")]).unwrap();
        assert!(!format!("{config:?}").contains("hunter2"));
    }
}
