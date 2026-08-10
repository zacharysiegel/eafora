//! User-settings framework: a typed key/value model and a persistence interface each platform
//! implements. The individual settings a platform exposes are defined by that platform, not here.

/// A setting's stable persistence key. Implemented by each platform's key enum.
pub trait SettingKey: Copy {
    fn storage_key(self) -> &'static str;
}

/// A setting's typed value. Each variant owns its byte encoding; a leading tag byte makes the bytes
/// self-describing, so decoding needs no external type context and a corrupt or wrong-typed value
/// simply fails to decode.
#[derive(Debug, Clone, PartialEq)]
pub enum SettingValue {
    Bool(bool),
}

const TAG_BOOL: u8 = 1;

impl SettingValue {
    pub fn to_bytes(&self) -> Vec<u8> {
        match self {
            SettingValue::Bool(value) => vec![TAG_BOOL, u8::from(*value)],
        }
    }

    /// The value the bytes encode, or `None` if they are empty, truncated, or carry an unknown tag.
    pub fn from_bytes(bytes: &[u8]) -> Option<SettingValue> {
        match bytes {
            [TAG_BOOL, 0] => Some(SettingValue::Bool(false)),
            [TAG_BOOL, 1] => Some(SettingValue::Bool(true)),
            _ => None,
        }
    }
}

impl From<bool> for SettingValue {
    fn from(value: bool) -> SettingValue {
        SettingValue::Bool(value)
    }
}

impl TryFrom<SettingValue> for bool {
    type Error = SettingValue;

    fn try_from(value: SettingValue) -> Result<bool, SettingValue> {
        match value {
            SettingValue::Bool(inner) => Ok(inner),
        }
    }
}

/// A persistent store for user settings, addressed by a typed key and value. The value's byte
/// encoding and how it is persisted are the implementation's concern; this interface never exposes
/// raw bytes or strings.
pub trait SettingsStore {
    fn load<K: SettingKey>(&self, key: K) -> Option<SettingValue>;
    fn store<K: SettingKey>(&self, key: K, value: SettingValue);
}

/// A typed setting: its key and default, bundled so both live in exactly one place. Reading and
/// writing go through here, the only path; there is no public raw byte or string accessor.
pub struct Setting<K: SettingKey, T> {
    key: K,
    default: T,
}

impl<K, T> Setting<K, T>
where
    K: SettingKey,
    T: Clone + Into<SettingValue> + TryFrom<SettingValue>,
{
    pub const fn new(key: K, default: T) -> Setting<K, T> {
        Setting { key, default }
    }

    pub fn read(&self, store: &impl SettingsStore) -> T {
        store
            .load(self.key)
            .and_then(|value| T::try_from(value).ok())
            .unwrap_or_else(|| self.default.clone())
    }

    pub fn write(&self, store: &impl SettingsStore, value: T) {
        store.store(self.key, value.into());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::collections::HashMap;

    #[derive(Clone, Copy)]
    enum TestKey {
        Flag,
    }

    impl SettingKey for TestKey {
        fn storage_key(self) -> &'static str {
            match self {
                TestKey::Flag => "flag",
            }
        }
    }

    #[derive(Default)]
    struct InMemoryStore {
        entries: RefCell<HashMap<&'static str, Vec<u8>>>,
    }

    impl SettingsStore for InMemoryStore {
        fn load<K: SettingKey>(&self, key: K) -> Option<SettingValue> {
            let bytes: Vec<u8> = self.entries.borrow().get(key.storage_key()).cloned()?;

            SettingValue::from_bytes(&bytes)
        }

        fn store<K: SettingKey>(&self, key: K, value: SettingValue) {
            self.entries.borrow_mut().insert(key.storage_key(), value.to_bytes());
        }
    }

    #[test]
    fn read_returns_the_default_when_absent() {
        let store: InMemoryStore = InMemoryStore::default();
        let flag: Setting<TestKey, bool> = Setting::new(TestKey::Flag, true);

        assert!(flag.read(&store));
    }

    #[test]
    fn write_then_read_round_trips_through_the_store() {
        let store: InMemoryStore = InMemoryStore::default();
        let flag: Setting<TestKey, bool> = Setting::new(TestKey::Flag, true);

        flag.write(&store, false);
        assert!(!flag.read(&store));

        flag.write(&store, true);
        assert!(flag.read(&store));
    }

    #[test]
    fn setting_value_bytes_round_trip() {
        for value in [SettingValue::Bool(true), SettingValue::Bool(false)] {
            assert_eq!(SettingValue::from_bytes(&value.to_bytes()), Some(value));
        }
    }

    #[test]
    fn from_bytes_rejects_empty_truncated_and_unknown_tag() {
        assert_eq!(SettingValue::from_bytes(&[]), None);
        assert_eq!(SettingValue::from_bytes(&[TAG_BOOL]), None);
        assert_eq!(SettingValue::from_bytes(&[9, 9]), None);
    }

    #[test]
    fn read_falls_back_to_the_default_on_undecodable_bytes() {
        let store: InMemoryStore = InMemoryStore::default();
        store.entries.borrow_mut().insert("flag", vec![0xff, 0xff]);
        let flag: Setting<TestKey, bool> = Setting::new(TestKey::Flag, true);

        assert!(flag.read(&store));
    }
}
