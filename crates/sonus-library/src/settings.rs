use crate::db::{Database, Result};

/// A tiny key/value store for user preferences.
///
/// Deliberately untyped and stringly-keyed: settings are few and each one is
/// read exactly once at startup. A typed struct would be more ceremony than
/// the problem deserves.
pub struct Settings;

impl Settings {
    pub fn set(db: &Database, key: &str, value: &str) -> Result<()> {
        db.conn().execute(
            "INSERT INTO settings (key, value) VALUES (?1, ?2)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            rusqlite::params![key, value],
        )?;
        Ok(())
    }

    pub fn get(db: &Database, key: &str) -> Result<Option<String>> {
        let value = db
            .conn()
            .query_row(
                "SELECT value FROM settings WHERE key = ?1",
                rusqlite::params![key],
                |row| row.get(0),
            )
            .ok();
        Ok(value)
    }

    /// Read a setting, falling back to `default` when it's missing.
    pub fn get_or(db: &Database, key: &str, default: &str) -> Result<String> {
        Ok(Self::get(db, key)?.unwrap_or_else(|| default.to_string()))
    }

    pub fn get_f32(db: &Database, key: &str, default: f32) -> Result<f32> {
        Ok(Self::get(db, key)?
            .and_then(|v| v.parse().ok())
            .unwrap_or(default))
    }

    pub fn set_f32(db: &Database, key: &str, value: f32) -> Result<()> {
        Self::set(db, key, &value.to_string())
    }

    pub fn get_bool(db: &Database, key: &str, default: bool) -> Result<bool> {
        Ok(Self::get(db, key)?
            .map(|v| v == "true" || v == "1")
            .unwrap_or(default))
    }

    pub fn set_bool(db: &Database, key: &str, value: bool) -> Result<()> {
        Self::set(db, key, if value { "true" } else { "false" })
    }

    pub fn remove(db: &Database, key: &str) -> Result<()> {
        db.conn().execute(
            "DELETE FROM settings WHERE key = ?1",
            rusqlite::params![key],
        )?;
        Ok(())
    }
}

/// Well-known setting keys, so a typo can't silently create a second key.
pub mod keys {
    pub const THEME: &str = "theme";
    pub const VOLUME: &str = "volume";
    pub const EQ_ENABLED: &str = "eq.enabled";
    pub const EQ_PREAMP: &str = "eq.preamp";
    pub const EQ_BAND_PREFIX: &str = "eq.band";
    pub const RADIO_ENABLED: &str = "radio.enabled";
    pub const REPEAT_MODE: &str = "queue.repeat";
    pub const SHUFFLE: &str = "queue.shuffle";

    pub fn eq_band(index: usize) -> String {
        format!("{EQ_BAND_PREFIX}.{index}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn db() -> Database {
        Database::in_memory().unwrap()
    }

    #[test]
    fn set_then_get() {
        let db = db();
        Settings::set(&db, keys::THEME, "Gruvbox").unwrap();
        assert_eq!(
            Settings::get(&db, keys::THEME).unwrap().as_deref(),
            Some("Gruvbox")
        );
    }

    #[test]
    fn get_missing_is_none() {
        assert!(Settings::get(&db(), "nope").unwrap().is_none());
    }

    #[test]
    fn set_overwrites() {
        let db = db();
        Settings::set(&db, keys::THEME, "A").unwrap();
        Settings::set(&db, keys::THEME, "B").unwrap();
        assert_eq!(Settings::get(&db, keys::THEME).unwrap().unwrap(), "B");
    }

    #[test]
    fn get_or_falls_back() {
        let db = db();
        assert_eq!(
            Settings::get_or(&db, "missing", "fallback").unwrap(),
            "fallback"
        );
        Settings::set(&db, "present", "value").unwrap();
        assert_eq!(
            Settings::get_or(&db, "present", "fallback").unwrap(),
            "value"
        );
    }

    #[test]
    fn float_round_trip() {
        let db = db();
        Settings::set_f32(&db, keys::VOLUME, 0.65).unwrap();
        assert!((Settings::get_f32(&db, keys::VOLUME, 0.0).unwrap() - 0.65).abs() < 1e-6);
    }

    #[test]
    fn float_falls_back_on_garbage() {
        let db = db();
        Settings::set(&db, keys::VOLUME, "not a number").unwrap();
        assert_eq!(Settings::get_f32(&db, keys::VOLUME, 0.5).unwrap(), 0.5);
    }

    #[test]
    fn bool_round_trip() {
        let db = db();
        Settings::set_bool(&db, keys::EQ_ENABLED, true).unwrap();
        assert!(Settings::get_bool(&db, keys::EQ_ENABLED, false).unwrap());
        Settings::set_bool(&db, keys::EQ_ENABLED, false).unwrap();
        assert!(!Settings::get_bool(&db, keys::EQ_ENABLED, true).unwrap());
    }

    #[test]
    fn bool_accepts_numeric_form() {
        let db = db();
        Settings::set(&db, "flag", "1").unwrap();
        assert!(Settings::get_bool(&db, "flag", false).unwrap());
    }

    #[test]
    fn bool_falls_back_when_missing() {
        assert!(Settings::get_bool(&db(), "missing", true).unwrap());
        assert!(!Settings::get_bool(&db(), "missing", false).unwrap());
    }

    #[test]
    fn remove_deletes_the_key() {
        let db = db();
        Settings::set(&db, "key", "value").unwrap();
        Settings::remove(&db, "key").unwrap();
        assert!(Settings::get(&db, "key").unwrap().is_none());
    }

    #[test]
    fn eq_band_keys_are_distinct() {
        assert_eq!(keys::eq_band(0), "eq.band.0");
        assert_eq!(keys::eq_band(9), "eq.band.9");
        assert_ne!(keys::eq_band(0), keys::eq_band(1));
    }
}
