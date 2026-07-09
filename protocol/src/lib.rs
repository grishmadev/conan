pub mod comm;
pub mod config;
pub mod constants;
pub mod database;
pub mod database_entities;
pub mod extras;
pub mod msg;
pub mod operations;
pub mod requests;
pub mod server_entities;
#[cfg(test)]
pub mod tests;

#[macro_export]
macro_rules! debug {
    ($($arg:tt)*) => {
        #[cfg(debug_assertions)]
        {
            println!("[DEBUG] {}", format_args!($($arg)*));
        }
    };
}
