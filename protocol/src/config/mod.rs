use std::{env, fs, process};

use clap::Parser;
use config::{Config, FileFormat};

use crate::{
    constants::{ARTI_KEYSTORE, CACHE_PATH, CONFIG_PATH, DAEMON_SOCKET, DATABASE_PATH},
    database::setup::setup_db,
};

#[derive(Debug, Parser)]
#[command(
    about,
    version,
    long_about = "Conan, a Tor based Decentralized Chat App."
)]
pub struct ConanArgs {
    /// Config File
    #[arg(short = 'c', long = "config", default_value = None)]
    pub config: Option<String>,

    /// Socket Location
    #[arg(short = 's', long = "sock", default_value =  None)]
    pub socket: Option<String>,

    /// Key Store path
    #[arg(short = 'k', long = "key", default_value = None)]
    pub key: Option<String>,

    /// Cache Storage Path
    #[arg(short = 'C', long = "cache", default_value = None)]
    pub cache: Option<String>,

    /// Database path
    #[arg(short = 'd', long = "db", default_value = None)]
    pub db_path: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ConanConfig {
    pub socket_path: String,
    pub arti_key_store: String,
    pub cache_path: String,
    pub db_path: String,
}

/// Function to decide final config
///
/// # Errors
pub fn parse_config() -> Result<ConanConfig, Box<dyn std::error::Error>> {
    let args = ConanArgs::parse();
    let home_path = env::var("HOME")?;
    let mut default_config_path = home_path.clone();
    default_config_path.push_str(CONFIG_PATH);
    let config_path = if let Some(s) = args.config {
        s
    } else {
        default_config_path
    };
    let config = match Config::builder()
        .add_source(config::File::new(&config_path, FileFormat::Toml))
        .build()
    {
        Ok(s) => Some(s),
        Err(e) => {
            eprintln!("Config Error, {e}\nUsing default.");
            None
        }
    };
    let socket_path = if let Some(s) = args.socket {
        s
    } else if let Some(ref s) = config
        && let Ok(path) = s.get_string("socket-path")
    {
        path
    } else {
        let mut key_path = home_path.clone();
        key_path.push_str(DAEMON_SOCKET);
        key_path
    };
    let arti_key_store = if let Some(s) = args.key {
        s
    } else if let Some(ref s) = config
        && let Ok(path) = s.get_string("key-path")
    {
        path
    } else {
        let mut key_path = home_path.clone();
        key_path.push_str(ARTI_KEYSTORE);
        key_path
    };
    let cache_path = if let Some(c) = args.cache {
        c
    } else if let Some(ref c) = config
        && let Ok(path) = c.get_string("cache-path")
    {
        path
    } else {
        let mut key_path = home_path.clone();
        key_path.push_str(CACHE_PATH);
        key_path
    };

    let db_path = if let Some(c) = args.db_path {
        c
    } else if let Some(ref db) = config
        && let Ok(path) = db.get_string("database-path")
    {
        path
    } else {
        let mut db_path = home_path.clone();
        db_path.push_str(DATABASE_PATH);
        db_path
    };
    _ = fs::create_dir(&cache_path);
    if setup_db(&db_path).is_err() {
        eprintln!("Could not setup Database.\nAborting");
        process::exit(1);
    }

    let res = ConanConfig {
        socket_path,
        arti_key_store,
        cache_path,
        db_path,
    };

    Ok(res)
}
