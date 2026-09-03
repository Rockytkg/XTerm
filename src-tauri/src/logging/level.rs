use std::sync::atomic::{AtomicUsize, Ordering};

use log::LevelFilter;

use crate::storage::{SettingsRepository, Store};

pub(crate) const DEFAULT_LOG_LEVEL: LevelFilter = LevelFilter::Info;

/// Runtime log level shared between the Tauri command and the file-target
/// filter. `log::set_max_level` gates records at the call site; this value
/// additionally drives per-crate clamping inside the daily-file dispatch,
/// which the plugin cannot rebuild after startup.
static ACTIVE_LEVEL: AtomicUsize = AtomicUsize::new(DEFAULT_LOG_LEVEL as usize);

pub fn active_level() -> LevelFilter {
    match ACTIVE_LEVEL.load(Ordering::Relaxed) {
        0 => LevelFilter::Off,
        1 => LevelFilter::Error,
        2 => LevelFilter::Warn,
        3 => LevelFilter::Info,
        4 => LevelFilter::Debug,
        _ => LevelFilter::Trace,
    }
}

pub fn set_active_level(level: LevelFilter) {
    ACTIVE_LEVEL.store(level as usize, Ordering::Relaxed);
}

/// Parses a persisted or user-supplied level name; "warning" is accepted as
/// an alias for "warn".
pub(crate) fn parse_log_level(value: &str) -> Result<LevelFilter, String> {
    match value.trim().to_ascii_lowercase().as_str() {
        "error" => Ok(LevelFilter::Error),
        "warn" | "warning" => Ok(LevelFilter::Warn),
        "info" => Ok(LevelFilter::Info),
        "debug" => Ok(LevelFilter::Debug),
        "trace" => Ok(LevelFilter::Trace),
        _ => Err(format!("unsupported log level '{value}'")),
    }
}

/// Canonical lowercase name used for persistence and the frontend.
pub(crate) fn log_level_name(level: LevelFilter) -> &'static str {
    match level {
        LevelFilter::Off => "off",
        LevelFilter::Error => "error",
        LevelFilter::Warn => "warn",
        LevelFilter::Info => "info",
        LevelFilter::Debug => "debug",
        LevelFilter::Trace => "trace",
    }
}

pub fn persisted_log_level(store: &Store) -> LevelFilter {
    match SettingsRepository::log_level(store) {
        Ok(Some(value)) => match parse_log_level(&value) {
            Ok(level) => level,
            Err(_) => {
                log::warn!(
                    target: "logging.level",
                    "unsupported persisted log level '{value}'; falling back to {}",
                    log_level_name(DEFAULT_LOG_LEVEL)
                );
                DEFAULT_LOG_LEVEL
            }
        },
        Ok(None) => DEFAULT_LOG_LEVEL,
        Err(error) => {
            log::warn!(
                target: "logging.level",
                "failed to read persisted log level: {error}; falling back to {}",
                log_level_name(DEFAULT_LOG_LEVEL)
            );
            DEFAULT_LOG_LEVEL
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_accepts_known_levels_and_alias() {
        assert_eq!(parse_log_level("error"), Ok(LevelFilter::Error));
        assert_eq!(parse_log_level("WARN"), Ok(LevelFilter::Warn));
        assert_eq!(parse_log_level("warning"), Ok(LevelFilter::Warn));
        assert_eq!(parse_log_level(" trace "), Ok(LevelFilter::Trace));
        assert!(parse_log_level("verbose").is_err());
    }

    #[test]
    fn level_name_round_trips_through_parse() {
        for level in [
            LevelFilter::Error,
            LevelFilter::Warn,
            LevelFilter::Info,
            LevelFilter::Debug,
            LevelFilter::Trace,
        ] {
            assert_eq!(parse_log_level(log_level_name(level)), Ok(level));
        }
    }

    #[test]
    fn active_level_round_trips() {
        set_active_level(LevelFilter::Debug);
        assert_eq!(active_level(), LevelFilter::Debug);
        set_active_level(LevelFilter::Info);
        assert_eq!(active_level(), LevelFilter::Info);
    }
}
