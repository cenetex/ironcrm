use super::key::{KeyCommitment, LicenseKey};
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KeyRegistry {
    pub keys: Vec<KeyCommitment>,
}

impl KeyRegistry {
    pub fn load(path: impl AsRef<Path>) -> Result<Self> {
        let data = std::fs::read_to_string(path.as_ref())
            .context("failed to read registry file")?;
        Ok(serde_json::from_str(&data)?)
    }

    pub fn save(&self, path: impl AsRef<Path>) -> Result<()> {
        if let Some(parent) = path.as_ref().parent() {
            std::fs::create_dir_all(parent)?;
        }
        let data = serde_json::to_string_pretty(self)?;
        std::fs::write(path.as_ref(), &data)?;
        Ok(())
    }

    pub fn validate(&self, key: &LicenseKey) -> bool {
        let secret = key.secret_bytes();
        for entry in &self.keys {
            let salt = match hex::decode(&entry.salt) {
                Ok(s) => s,
                Err(_) => continue,
            };
            let mut hasher = Sha256::new();
            hasher.update(secret);
            hasher.update(&salt);
            let digest = hex::encode(hasher.finalize());
            if digest == entry.hash {
                return true;
            }
        }
        false
    }

    pub fn add_key(&mut self, key: &LicenseKey, _label: Option<&str>) -> KeyCommitment {
        let salt: [u8; 16] = rand::random();
        let mut hasher = Sha256::new();
        hasher.update(key.secret_bytes());
        hasher.update(&salt);
        let hash = hex::encode(hasher.finalize());

        let next_id = self.keys.iter().filter_map(|k| {
            k.id.strip_prefix("key-").and_then(|s| s.parse::<u32>().ok())
        }).max().unwrap_or(0) + 1;

        let entry = KeyCommitment {
            id: format!("key-{:04}", next_id),
            salt: hex::encode(salt),
            hash,
        };
        self.keys.push(entry.clone());
        entry
    }

    pub fn revoke_key(&mut self, key: &LicenseKey) -> bool {
        let secret = key.secret_bytes();
        let before = self.keys.len();
        self.keys.retain(|entry| {
            let salt = match hex::decode(&entry.salt) {
                Ok(s) => s,
                Err(_) => return true,
            };
            let mut hasher = Sha256::new();
            hasher.update(secret);
            hasher.update(&salt);
            hex::encode(hasher.finalize()) != entry.hash
        });
        self.keys.len() < before
    }

    pub fn len(&self) -> usize { self.keys.len() }
    pub fn is_empty(&self) -> bool { self.keys.is_empty() }
}

impl Default for KeyRegistry {
    fn default() -> Self { KeyRegistry { keys: Vec::new() } }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn validate_generated() {
        let key = LicenseKey::generate();
        let mut reg = KeyRegistry::default();
        assert!(!reg.validate(&key));
        reg.add_key(&key, None);
        assert!(reg.validate(&key));
    }

    #[test]
    fn revoke_removes() {
        let key = LicenseKey::generate();
        let mut reg = KeyRegistry::default();
        reg.add_key(&key, None);
        assert!(reg.validate(&key));
        assert!(reg.revoke_key(&key));
        assert!(!reg.validate(&key));
    }

    #[test]
    fn save_load_roundtrip() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("registry.json");
        let key = LicenseKey::generate();
        let mut reg = KeyRegistry::default();
        reg.add_key(&key, None);
        reg.save(&path).unwrap();
        let loaded = KeyRegistry::load(&path).unwrap();
        assert_eq!(reg.keys.len(), loaded.keys.len());
        assert!(loaded.validate(&key));
    }
}
