use serde::{Deserialize, Serialize};

/// https://docs.joinmastodon.org/methods/instance/#domain_blocks
#[derive(Serialize, Deserialize, Debug)]
pub struct DomainBlock {
    /// The domain which is blocked. This may be obfuscated or partially censored.
    pub domain: String,
    /// The SHA256 hash digest of the domain string.
    pub digest: String,
    /// The level to which the domain is blocked.
    pub severity: DomainBlockSeverity,
    /// An optional reason for the domain block.
    pub comment: Option<String>,
}

#[derive(Serialize, Deserialize, Debug)]
pub enum DomainBlockSeverity {
    #[serde(rename = "silence")]
    Silence,
    #[serde(rename = "suspend")]
    Suspend,
}
