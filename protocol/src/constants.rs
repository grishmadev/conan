pub const SELF_PORT: u16 = 80;
pub const ENCRYPTION_INFO: &str = "tor-secret-conan-secret";
pub const TOR_RELAY_LIST_URL: &str =
    "https://onionoo.torproject.org/summary?type=relay&running=true";

/// Maximum size of the bounded channel in `[PeerConnection]`
pub const BOUNDED_CHANNEL_SIZE: usize = 100;

/// Key Storage dir for Arti Client
pub const ARTI_KEYSTORE: &str = "/.conan/tor_state";

/// Private key from Arti Client to sign during key exchange
pub const ARTI_PRIVATE_KEY: &str = "/keystore/hss/conan-daemon/ks_hs_id.ed25519_expanded_private";

/// Socket Location of daemon socket for inter process communication
pub const DAEMON_SOCKET: &str = "/var/conan/conan.socket";
