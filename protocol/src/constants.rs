pub const SELF_PORT: u16 = 80;
pub const ENCRYPTION_INFO: &str = "tor-secret-conan-secret";
pub const RATCHET_INFO: &str = "conan-v1-ratchet";
pub const TOR_RELAY_LIST_URL: &str =
    "https://onionoo.torproject.org/summary?type=relay&running=true";
pub const INFO_ROOT: &[u8] = b"conan-v1-root";
pub const INFO_CHAIN: &[u8] = b"conan-v1-chain";
pub const INFO_MESSAGE: &[u8] = b"conan-v1-message";
pub const INFO_INIT_RK: &[u8] = b"conan-v1-init-rk";
pub const INFO_INIT_HKS: &[u8] = b"conan-v1-init-hks";
pub const INFO_INIT_HKR: &[u8] = b"conan-v1-init-hkr";
pub const INFO_ED_TO_X: &[u8] = b"conan-v1-ed25519-to-x25519";

/// Maximum number of message keys that can be skipped in a single chain.
/// High enough to tolerate reordering, low enough to
/// prevent a malicious sender from causing excessive storage.
pub const MAX_SKIP: u64 = 1000;

/// Hard cap on total stored skipped keys across all chains (DOS guard).
pub const MAX_SKIPPED_KEYS: usize = 2000;

/// Maximum size of the bounded channel in `[PeerConnection]`
pub const BOUNDED_CHANNEL_SIZE: usize = 100;

/// Key Storage dir for Arti Client
pub const ARTI_KEYSTORE: &str = "/.conan/tor_state";

/// Private key from Arti Client to sign during key exchange
pub const ARTI_PRIVATE_KEY: &str = "/keystore/hss/conan-daemon/ks_hs_id.ed25519_expanded_private";

/// Cache path for arti client
pub const CACHE_PATH: &str = "/.conan/cache";

/// Socket Location of daemon socket for inter process communication
pub const DAEMON_SOCKET: &str = "/.conan/conan.socket";

/// Config File Path
pub const CONFIG_PATH: &str = "/.config/conan/conan.toml";

/// Databse Path
pub const DATABASE_PATH: &str = "/.conan/database";
