use std::fmt::Display;

use safelog::DisplayRedacted;
use tor_hsservice::HsId;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct NodeId([u8; 32]);

impl From<HsId> for NodeId {
    fn from(value: HsId) -> Self {
        let val = value
            .display_unredacted()
            .to_string()
            .into_bytes()
            .try_into()
            .unwrap();
        Self(val)
    }
}

impl From<&str> for NodeId {
    fn from(value: &str) -> Self {
        println!("node address: {value}");
        Self(value.as_bytes().try_into().unwrap())
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

impl Display for NodeId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", String::from_utf8_lossy(&self.0))
    }
}
