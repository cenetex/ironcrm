//! Holonet — a tamper-evident hash-linked event chain for license operations.
//!
//! Every event (issue, revoke, extend, verify) appends a block to the chain.
//! Each block contains the previous block's hash, a timestamp, the event
//! payload, and a nonce. The chain can be verified independently — any
//! tamperer would need to rehash every subsequent block.
//!
//! This is *not* a blockchain (no consensus, no mining). It is a
//! hash-append log that makes the license history independently verifiable.

use chrono::Utc;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// The event type for a chain entry.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum HoloEvent {
    /// A new license key was issued.
    Issue {
        key_hash: String,
        label: Option<String>,
    },
    /// A license key was revoked.
    Revoke { key_hash: String },
    /// A license was extended / re-issued.
    Extend {
        key_hash: String,
        label: Option<String>,
    },
    /// An anchor event — used for batch seals or external anchoring.
    Anchor {
        note: String,
        external_hash: Option<String>,
    },
}

/// A single block in the holonet chain.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HoloBlock {
    pub index: u64,
    pub timestamp: String,
    pub previous_hash: String,
    pub event: HoloEvent,
    pub nonce: u64,
    pub hash: String,
}

impl HoloBlock {
    /// Compute this block's hash from its content (not including `hash` field).
    pub fn compute_hash(&self) -> String {
        let mut hasher = Sha256::new();
        hasher.update(self.index.to_le_bytes());
        hasher.update(self.timestamp.as_bytes());
        hasher.update(self.previous_hash.as_bytes());
        hasher.update(serde_json::to_vec(&self.event).unwrap_or_default());
        hasher.update(self.nonce.to_le_bytes());
        hex::encode(hasher.finalize())
    }

    /// Verify this block's hash is internally consistent.
    pub fn verify_self(&self) -> bool {
        self.hash == self.compute_hash()
    }
}

/// The full holonet chain.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HoloChain {
    pub blocks: Vec<HoloBlock>,
}

impl HoloChain {
    pub fn new() -> Self {
        HoloChain { blocks: Vec::new() }
    }

    /// Create the genesis block.
    pub fn genesis() -> Self {
        let block = HoloBlock {
            index: 0,
            timestamp: Utc::now().to_rfc3339(),
            previous_hash: "0000000000000000000000000000000000000000000000000000000000000000"
                .to_string(),
            event: HoloEvent::Anchor {
                note: "IronCRM license holonet genesis".into(),
                external_hash: None,
            },
            nonce: 0,
            hash: String::new(),
        };
        let hash = block.compute_hash();
        let block = HoloBlock { hash, ..block };

        HoloChain {
            blocks: vec![block],
        }
    }

    /// Append a new event, linking to the previous block.
    pub fn append(&mut self, event: HoloEvent) -> &HoloBlock {
        let previous = self.blocks.last().expect("holonet must have genesis");
        let block = HoloBlock {
            index: previous.index + 1,
            timestamp: Utc::now().to_rfc3339(),
            previous_hash: previous.hash.clone(),
            event,
            nonce: 0,
            hash: String::new(),
        };
        let hash = block.compute_hash();
        let block = HoloBlock { hash, ..block };
        self.blocks.push(block);
        self.blocks.last().unwrap()
    }

    /// Verify the entire chain from genesis forward.
    /// Returns (valid, last_index, error_description).
    pub fn verify_chain(&self) -> (bool, u64, String) {
        if self.blocks.is_empty() {
            return (false, 0, "empty chain".into());
        }

        for (i, block) in self.blocks.iter().enumerate() {
            if !block.verify_self() {
                return (false, block.index, format!("block {} hash mismatch", i));
            }
            if i > 0 {
                let prev = &self.blocks[i - 1];
                if block.previous_hash != prev.hash {
                    return (
                        false,
                        block.index,
                        format!("block {} previous hash mismatch", i),
                    );
                }
            } else if block.index != 0 {
                return (false, 0, "genesis block must have index 0".into());
            }
        }

        let last = self.blocks.last().unwrap();
        (true, last.index, "chain intact".into())
    }

    /// Number of blocks (including genesis).
    pub fn len(&self) -> usize {
        self.blocks.len()
    }

    pub fn is_empty(&self) -> bool {
        self.blocks.is_empty()
    }

    /// Export the chain as JSON for audit or distribution.
    pub fn export_json(&self) -> String {
        serde_json::to_string_pretty(self).unwrap_or_default()
    }

    /// Load chain from JSON.
    pub fn load_json(data: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(data)
    }

    /// Load chain from a file.
    pub fn load(path: impl AsRef<std::path::Path>) -> Result<Self, anyhow::Error> {
        let data = std::fs::read_to_string(path.as_ref())?;
        Ok(serde_json::from_str(&data)?)
    }

    /// Save chain to a file.
    pub fn save(&self, path: impl AsRef<std::path::Path>) -> Result<(), anyhow::Error> {
        if let Some(parent) = path.as_ref().parent() {
            std::fs::create_dir_all(parent)?;
        }
        let data = self.export_json();
        std::fs::write(path.as_ref(), &data)?;
        Ok(())
    }
}

impl Default for HoloChain {
    fn default() -> Self {
        Self::genesis()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn genesis_is_valid() {
        let chain = HoloChain::genesis();
        let (valid, idx, msg) = chain.verify_chain();
        assert!(valid, "genesis chain should be valid: {}", msg);
        assert_eq!(idx, 0);
    }

    #[test]
    fn append_and_verify() {
        let mut chain = HoloChain::genesis();
        chain.append(HoloEvent::Issue {
            key_hash: "abc123".into(),
            label: Some("test user".into()),
        });
        chain.append(HoloEvent::Issue {
            key_hash: "def456".into(),
            label: None,
        });
        chain.append(HoloEvent::Revoke {
            key_hash: "abc123".into(),
        });

        let (valid, idx, msg) = chain.verify_chain();
        assert!(valid, "chain should be valid: {} (at block {})", msg, idx);
        assert_eq!(chain.len(), 4);
    }

    #[test]
    fn tampered_block_detected() {
        let mut chain = HoloChain::genesis();
        chain.append(HoloEvent::Issue {
            key_hash: "original".into(),
            label: None,
        });

        // Tamper with the block
        if let Some(block) = chain.blocks.get_mut(1) {
            block.event = HoloEvent::Issue {
                key_hash: "tampered".into(),
                label: None,
            };
        }

        let (valid, idx, _msg) = chain.verify_chain();
        assert!(!valid, "tampered chain should be detected");
        assert_eq!(idx, 1);
    }

    #[test]
    fn save_load_roundtrip() {
        let mut chain = HoloChain::genesis();
        chain.append(HoloEvent::Issue {
            key_hash: "roundtrip".into(),
            label: None,
        });

        let json = chain.export_json();
        let loaded = HoloChain::load_json(&json).unwrap();
        assert_eq!(chain.blocks.len(), loaded.blocks.len());
        assert_eq!(chain.blocks[1].hash, loaded.blocks[1].hash);
        assert!(loaded.verify_chain().0);
    }
}
