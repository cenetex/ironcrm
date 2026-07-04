//! White Warren Licensing Module
//!
//! A salted-commitment license key system with holonet audit chain.
//!
//! ## Architecture
//!
//! - **Key format**: `IRON-XXXX-XXXX-XXXX-XXXX` (8 random bytes)
//! - **Commitment registry**: each key stored as `{salt, SHA256(secret || salt)}`
//!   — zero key material is disclosed in the registry.
//! - **Holonet**: an append-only hash-linked chain of license events.
//!   Each block links to the previous by SHA256 hash, making the entire
//!   history tamper-evident. The chain can be verified independently —
//!   no server needed.
//! - **Encrypted registry**: optional AES-256-CBC + HMAC wrapping for
//!   distribution, matching the Ares approach.
//!
//! ## Usage
//!
//! ```rust
//! use ironcrm::licensing::{LicenseKey, Licensor};
//!
//! let mut licensor = Licensor::new("keys/registry.json", "keys/holonet.json").unwrap();
//! let key = LicenseKey::generate();
//! licensor.issue_key(&key, Some("customer@example.com")).unwrap();
//! assert!(licensor.validate_key(&key));
//! ```

mod chain;
mod key;
mod registry;

pub use chain::{HoloBlock, HoloChain, HoloEvent};
pub use key::{KeyCommitment, LicenseKey};
pub use registry::KeyRegistry;

use anyhow::{Context, Result};
use std::path::PathBuf;

/// High-level licensor: manages a commitment registry and holonet chain together.
pub struct Licensor {
    registry_path: PathBuf,
    chain_path: PathBuf,
    registry: KeyRegistry,
    chain: HoloChain,
}

impl Licensor {
    /// Open or create a licensor at the given file paths.
    pub fn new(registry_path: impl Into<PathBuf>, chain_path: impl Into<PathBuf>) -> Result<Self> {
        let registry_path = registry_path.into();
        let chain_path = chain_path.into();

        let registry = if registry_path.exists() {
            KeyRegistry::load(&registry_path)?
        } else {
            KeyRegistry::default()
        };

        let chain = if chain_path.exists() {
            HoloChain::load(&chain_path)?
        } else {
            HoloChain::genesis()
        };

        Ok(Licensor {
            registry_path,
            chain_path,
            registry,
            chain,
        })
    }

    /// Validate a key against the registry.
    pub fn validate_key(&self, key: &LicenseKey) -> bool {
        self.registry.validate(key)
    }

    /// Issue a new key: add to registry + append to holonet.
    pub fn issue_key(&mut self, key: &LicenseKey, label: Option<&str>) -> Result<()> {
        let _entry = self.registry.add_key(key, label);
        self.chain.append(HoloEvent::Issue {
            key_hash: key.secret_hex(),
            label: label.map(String::from),
        });
        self.save()?;
        Ok(())
    }

    /// Revoke a key: remove from registry + append to holonet.
    pub fn revoke_key(&mut self, key: &LicenseKey) -> Result<bool> {
        if !self.registry.revoke_key(key) {
            return Ok(false);
        }
        self.chain.append(HoloEvent::Revoke {
            key_hash: key.secret_hex(),
        });
        self.save()?;
        Ok(true)
    }

    /// Extend a key (re-issue with new label): append extend event to holonet.
    pub fn extend_key(&mut self, key: &LicenseKey, label: Option<&str>) -> Result<()> {
        self.chain.append(HoloEvent::Extend {
            key_hash: key.secret_hex(),
            label: label.map(String::from),
        });
        self.save()?;
        Ok(())
    }

    /// Anchor the holonet with an external hash (e.g. a git commit or IPFS CID).
    pub fn anchor(&mut self, note: &str, external_hash: Option<&str>) -> Result<()> {
        self.chain.append(HoloEvent::Anchor {
            note: note.to_string(),
            external_hash: external_hash.map(String::from),
        });
        self.save()?;
        Ok(())
    }

    /// Verify the entire holonet chain.
    pub fn verify_chain(&self) -> (bool, u64, String) {
        self.chain.verify_chain()
    }

    /// Number of registered keys.
    pub fn key_count(&self) -> usize {
        self.registry.len()
    }

    /// Number of holonet blocks (including genesis).
    pub fn chain_length(&self) -> usize {
        self.chain.len()
    }

    pub fn registry(&self) -> &KeyRegistry {
        &self.registry
    }

    pub fn chain(&self) -> &HoloChain {
        &self.chain
    }

    pub fn chain_json(&self) -> String {
        self.chain.export_json()
    }

    pub fn registry_json(&self) -> String {
        serde_json::to_string_pretty(&self.registry).unwrap_or_default()
    }

    fn save(&self) -> Result<()> {
        self.registry
            .save(&self.registry_path)
            .context("failed to save license registry")?;
        self.chain
            .save(&self.chain_path)
            .context("failed to save holonet chain")?;
        Ok(())
    }
}
