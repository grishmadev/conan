use std::{env, path::PathBuf};

pub mod mls_provider;
#[must_use]
pub fn default_sock_path() -> PathBuf {
    std::env::var("XDG_RUNTIME_DIR")
        .map_or_else(|_| env::temp_dir(), |_| PathBuf::new())
        .join("conan.sock")
}
