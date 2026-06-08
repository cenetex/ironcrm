use hex;
use serde::{Deserialize, Serialize};
use std::fmt;

/// A license key: 8 random bytes formatted as IRON-XXXX-XXXX-XXXX-XXXX.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LicenseKey {
    pub secret: [u8; 8],
}

impl LicenseKey {
    /// Parse an IRON-XXXX-XXXX-XXXX-XXXX key (dashes and prefix optional).
    pub fn parse(s: &str) -> Option<Self> {
        let s = s.trim().to_uppercase();
        let hex_str = s.strip_prefix("IRON-").unwrap_or(&s);
        let hex: String = hex_str.chars().filter(|c| *c != '-').collect();
        if hex.len() != 16 {
            return None;
        }
        let bytes = hex::decode(&hex).ok()?;
        if bytes.len() != 8 {
            return None;
        }
        let mut secret = [0u8; 8];
        secret.copy_from_slice(&bytes);
        Some(LicenseKey { secret })
    }

    /// Generate a new random key.
    pub fn generate() -> Self {
        LicenseKey { secret: rand::random() }
    }

    pub fn secret_bytes(&self) -> &[u8; 8] {
        &self.secret
    }

    pub fn secret_hex(&self) -> String {
        hex::encode(self.secret)
    }
}

impl fmt::Display for LicenseKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let h = self.secret_hex().to_uppercase();
        write!(f, "IRON-{}-{}-{}-{}", &h[0..4], &h[4..8], &h[8..12], &h[12..16])
    }
}

/// A salted commitment entry stored in the registry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KeyCommitment {
    pub id: String,
    pub salt: String,
    pub hash: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_roundtrip() {
        let key = LicenseKey::generate();
        let s = key.to_string();
        assert!(s.starts_with("IRON-"));
        assert_eq!(s.len(), 24);
        let parsed = LicenseKey::parse(&s).unwrap();
        assert_eq!(key.secret, parsed.secret);
    }

    #[test]
    fn parse_various() {
        let key = LicenseKey::parse("IRON-A1B2-C3D4-E5F6-7890").unwrap();
        assert_eq!(key.to_string(), "IRON-A1B2-C3D4-E5F6-7890");
        let key2 = LicenseKey::parse("A1B2-C3D4-E5F6-7890").unwrap();
        assert_eq!(key, key2);
    }

    #[test]
    fn invalid_rejected() {
        assert!(LicenseKey::parse("IRON-XXXX").is_none());
        assert!(LicenseKey::parse("").is_none());
    }
}
