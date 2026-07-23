pub mod comm;
pub mod config;
pub mod constants;
pub mod crypto;
pub mod database;
pub mod entities;
pub mod extras;
pub mod mls;
pub mod msg;
pub mod operations;
pub mod requests;
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
