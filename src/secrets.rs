use thiserror::Error;

#[derive(Debug, Error)]
pub enum SecretStoreError {
    #[error("secret store error: {0}")]
    Keyring(String),
}

pub trait SecretStore: Send + Sync {
    fn get(&self, name: &str) -> Result<Option<String>, SecretStoreError>;
    fn set(&self, name: &str, value: &str) -> Result<(), SecretStoreError>;
    #[allow(dead_code)]
    fn delete(&self, name: &str) -> Result<(), SecretStoreError>;
}

#[derive(Default)]
pub struct KeyringSecretStore;
impl KeyringSecretStore {
    fn entry(name: &str) -> Result<keyring::Entry, SecretStoreError> {
        keyring::Entry::new("vocs", name)
            .map_err(|error| SecretStoreError::Keyring(error.to_string()))
    }
}
impl SecretStore for KeyringSecretStore {
    fn get(&self, name: &str) -> Result<Option<String>, SecretStoreError> {
        match Self::entry(name)?.get_password() {
            Ok(value) => Ok(Some(value)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(error) => Err(SecretStoreError::Keyring(error.to_string())),
        }
    }
    fn set(&self, name: &str, value: &str) -> Result<(), SecretStoreError> {
        Self::entry(name)?
            .set_password(value)
            .map_err(|error| SecretStoreError::Keyring(error.to_string()))
    }
    fn delete(&self, name: &str) -> Result<(), SecretStoreError> {
        match Self::entry(name)?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(error) => Err(SecretStoreError::Keyring(error.to_string())),
        }
    }
}

#[cfg(test)]
pub struct MemorySecretStore(std::sync::Mutex<std::collections::BTreeMap<String, String>>);
#[cfg(test)]
impl Default for MemorySecretStore {
    fn default() -> Self {
        Self(Default::default())
    }
}
#[cfg(test)]
impl SecretStore for MemorySecretStore {
    fn get(&self, name: &str) -> Result<Option<String>, SecretStoreError> {
        Ok(self.0.lock().unwrap().get(name).cloned())
    }
    fn set(&self, name: &str, value: &str) -> Result<(), SecretStoreError> {
        self.0.lock().unwrap().insert(name.into(), value.into());
        Ok(())
    }
    fn delete(&self, name: &str) -> Result<(), SecretStoreError> {
        self.0.lock().unwrap().remove(name);
        Ok(())
    }
}

#[cfg(test)]
#[test]
fn memory_store_round_trip_and_delete() {
    let store = MemorySecretStore::default();
    store.set("key", "value").unwrap();
    assert_eq!(store.get("key").unwrap().as_deref(), Some("value"));
    store.delete("key").unwrap();
    assert_eq!(store.get("key").unwrap(), None);
}
