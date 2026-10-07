use std::{env, error::Error, fs, path::Path, process};

use clap::Parser;
use config::{Config, FileFormat};

use crate::constants::{ARTI_KEYSTORE, CACHE_PATH, CONFIG_PATH, DAEMON_SOCKET, DATABASE_PATH};
use conandatabase::setup_db;

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

const DEFAULT_CONFIG_CONTENT: &str = include_str!("./default_config.toml");

fn create_default_config(config_path: &str) -> Result<(), Box<dyn Error>> {
    let home_dir = env::home_dir()
        .ok_or("Failed to get home directory.")?
        .to_string_lossy()
        .to_string();
    let config_content = DEFAULT_CONFIG_CONTENT.replace("{home}", &home_dir);
    println!("conf content: {config_content}");
    let path = Path::new(config_path);
    if let Some(parent) = path.parent() {
        if !fs::exists(parent)? {
            println!(
                "Config Directory doesn't exist. Creating {}.",
                parent.display()
            );
        }
        fs::create_dir_all(parent)?;
    }
    fs::write(config_path, config_content)?;
    Ok(())
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
    let mut config: Option<Config> = None;
    for i in 0..3 {
        let file = match Config::builder()
            .add_source(config::File::new(&config_path, FileFormat::Toml))
            .build()
        {
            Ok(s) => Some(s),
            Err(e) => {
                eprintln!("Config Error, Using default.\n{e:?}");
                // creating default config
                create_default_config(&config_path)?;
                // creating space for other needed files (socket, database, keys)
                let mut conan_dir = home_path.clone();
                conan_dir.push_str("/.conan");
                _ = fs::create_dir_all(&conan_dir);
                None
            }
        };
        if file.is_some() {
            config = file;
            break;
        }
        if i <= 2 {
            println!("Retrying: [{i}/3]");
        }
    }
    if config.is_none() {
        eprintln!("Could not initialize Config. Sorry!");
        process::exit(1);
    }
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
    _ = fs::create_dir_all(&cache_path);
    if let Err(e) = fs::create_dir_all(&arti_key_store) {
        eprintln!("Warning: could not create keystore dir: {e}");
    } else {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = fs::set_permissions(&arti_key_store, fs::Permissions::from_mode(0o700));
        }
    }
    if let Some(parent) = std::path::Path::new(&db_path).parent() {
        _ = fs::create_dir_all(parent);
    }
    if let Some(parent) = std::path::Path::new(&socket_path).parent() {
        _ = fs::create_dir_all(parent);
    }
    if let Err(e) = setup_db(&db_path) {
        eprintln!("Could not setup Database.\n{e}");
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

#[cfg(test)]
mod tests {
    use std::fs;

    use crate::{config::create_default_config, constants::CONFIG_PATH};

    #[test]
    fn test_config_creation() {
        let config_path = "./";
        create_default_config(config_path).unwrap();
        let file_path = format!("{config_path}{CONFIG_PATH}");
        let exists = fs::exists(&file_path).unwrap();
        assert!(exists);
        fs::remove_file(&file_path).unwrap();
    }
}
