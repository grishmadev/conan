use std::{env, error::Error, fs, path::Path, process};

use clap::Parser;
use conan_extras::logs::Log;
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
    println!("conf content:\n{config_content}");
    let path = Path::new(config_path);
    if let Some(parent) = path.parent() {
        if !fs::exists(parent)? {
            Log::debug(&format!(
                "Config Directory doesn't exist. Creating {}.",
                parent.display()
            ));
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
    let home = env::var("HOME")?;
    let config_path = args
        .config
        .unwrap_or_else(|| format!("{home}{CONFIG_PATH}"));
    // load config, generating a default one and retrying on failure
    let Ok(config) = load_config(&config_path, &home) else {
        Log::err("Could not initialize Config. Sorry!");
        process::exit(1);
    };

    // precedence: CLI argument > config file value > home relative default
    let socket_path = resolve_path(args.socket, &config, "socket-path", DAEMON_SOCKET, &home);
    let arti_key_store = resolve_path(args.key, &config, "key-path", ARTI_KEYSTORE, &home);
    let cache_path = resolve_path(args.cache, &config, "cache-path", CACHE_PATH, &home);
    let db_path = resolve_path(args.db_path, &config, "database-path", DATABASE_PATH, &home);

    _ = fs::create_dir_all(&cache_path);
    match fs::create_dir_all(&arti_key_store) {
        Ok(()) => {
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let _ = fs::set_permissions(&arti_key_store, fs::Permissions::from_mode(0o700));
            }
        }
        Err(e) => Log::err(&format!("Warning: could not create keystore dir: {e}")),
    }
    // creating space for other needed files (database, socket)
    for path in [&db_path, &socket_path] {
        if let Some(parent) = Path::new(path).parent() {
            _ = fs::create_dir_all(parent);
        }
    }
    if let Err(e) = setup_db(&db_path) {
        Log::err(&format!("Could not setup Database.\n{e}"));
        process::exit(1);
    }

    Ok(ConanConfig {
        socket_path,
        arti_key_store,
        cache_path,
        db_path,
    })
}

/// Loads the TOML config file, creating a default one and retrying up to 3 times.
///
/// # Errors
/// Returns the error from [`create_default_config`] if writing the default fails.
fn load_config(config_path: &str, home: &str) -> Result<Config, Box<dyn Error>> {
    for i in 0..3 {
        match Config::builder()
            .add_source(config::File::new(config_path, FileFormat::Toml))
            .build()
        {
            Ok(config) => return Ok(config),
            Err(e) => {
                Log::err(&format!("Config Error, Using default.\n{e:?}"));
                // creating default config
                create_default_config(config_path)?;
                // creating space for other needed files (socket, database, keys)
                _ = fs::create_dir_all(format!("{home}/.conan"));
                Log::debug(&format!("Retrying: [{i}/3]"));
            }
        }
    }
    Err("Could not initialize Config. Sorry!".into())
}

/// Resolves a path with precedence: CLI argument > config value > `{home}{default}` fallback.
fn resolve_path(
    arg: Option<String>,
    config: &Config,
    key: &str,
    default: &str,
    home: &str,
) -> String {
    arg.or_else(|| config.get_string(key).ok())
        .unwrap_or_else(|| format!("{home}{default}"))
}

#[cfg(test)]
mod tests {
    use std::fs;

    use crate::{config::create_default_config, constants::CONFIG_PATH};

    #[test]
    fn test_config_creation() {
        let config_path = "config_test";
        let file_path = format!("{config_path}{CONFIG_PATH}");
        // if !fs::exists(&file_path).unwrap() {
        //     fs::create_dir_all(&file_path).unwrap();
        // }
        create_default_config(&file_path).unwrap();
        let exists = fs::exists(&file_path).unwrap();
        assert!(exists);
        fs::remove_file(&file_path).unwrap();
        fs::remove_dir_all(config_path).unwrap();
    }
}
