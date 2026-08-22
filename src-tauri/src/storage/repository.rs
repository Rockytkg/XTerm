use super::{AppPreferences, Store};

/// Read/write access to the settings table. Kept as a trait so modules that
/// only need settings (file service, proxy) can be tested against an
/// in-memory fake instead of a real redb database.
pub(crate) trait SettingsRepository {
    fn log_level(&self) -> Result<Option<String>, String>;
    fn set_log_level(&self, level: &str) -> Result<(), String>;
    fn preferences(&self) -> Result<AppPreferences, String>;
    fn set_preferences(&self, preferences: &AppPreferences) -> Result<(), String>;
    fn setting_value(&self, key: &str) -> Result<Option<String>, String>;
    fn set_setting(&self, key: &str, value: &str) -> Result<(), String>;
    fn delete_setting(&self, key: &str) -> Result<(), String>;
}

impl SettingsRepository for Store {
    fn log_level(&self) -> Result<Option<String>, String> {
        Store::log_level(self)
    }

    fn set_log_level(&self, level: &str) -> Result<(), String> {
        Store::set_log_level(self, level)
    }

    fn preferences(&self) -> Result<AppPreferences, String> {
        Store::preferences(self)
    }

    fn set_preferences(&self, preferences: &AppPreferences) -> Result<(), String> {
        Store::set_preferences(self, preferences)
    }

    fn setting_value(&self, key: &str) -> Result<Option<String>, String> {
        Store::setting_value(self, key)
    }

    fn set_setting(&self, key: &str, value: &str) -> Result<(), String> {
        Store::set_setting(self, key, value)
    }

    fn delete_setting(&self, key: &str) -> Result<(), String> {
        Store::delete_setting(self, key)
    }
}
