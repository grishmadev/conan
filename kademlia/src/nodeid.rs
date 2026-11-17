use std::fmt::Display;

use safelog::DisplayRedacted;
use sha2::{Digest, Sha256};
use tor_hsservice::HsId;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct NodeId([u8; 32]);

impl From<&str> for NodeId {
    fn from(value: &str) -> Self {
        let clean = value.trim_end_matches(".onion");
        let hash = Sha256::digest(clean.as_bytes()).into();
        Self(hash)
    }
}

impl NodeId {
    #[must_use]
    pub fn distance(&self, other: &Self) -> Self {
        let mut result = [0u8; 32];
        for i in 0..32 {
            result[i] = self.0[i] ^ other.0[i];
        }
        Self(result)
    }

    #[must_use]
    pub fn bucket_index(&self, other: &Self) -> usize {
        let distance = self.distance(other);
        for (idx, byte) in distance.0.iter().enumerate() {
            if *byte != 0 {
                let leading_zero = byte.leading_zeros() as usize;
                return idx * 8 + leading_zero;
            }
        }
        255
    }
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Node {
    id: NodeId,
    address: String,
}

impl Node {
    #[must_use]
    pub fn new(address: &str) -> Self {
        Self {
            id: NodeId::from(address),
            address: address.to_string(),
        }
    }
    #[must_use]
    pub fn node_id(&self) -> &NodeId {
        &self.id
    }
    #[must_use]
    pub fn address(&self) -> String {
        self.address.clone()
    }
}

impl Display for Node {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.address)
    }
}
impl From<HsId> for Node {
    fn from(value: HsId) -> Self {
        let address = value.display_unredacted().to_string();
        Node {
            id: NodeId::from(address.as_str()),
            address,
        }
    }
}

impl From<&str> for Node {
    fn from(value: &str) -> Self {
        Self {
            id: NodeId::from(value),
            address: value.to_string(),
        }
    }
}
