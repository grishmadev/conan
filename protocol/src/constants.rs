pub const SELF_PORT: u16 = 80;
pub const ENCRYPTION_INFO: &str = "tor-secret-conan-secret";
pub const TOR_RELAY_LIST_URL: &str =
    "https://onionoo.torproject.org/summary?type=relay&running=true";

/// Maximum size of the bounded channel in `[PeerConnection]`
pub const BOUNDED_CHANNEL_SIZE: usize = 100;
