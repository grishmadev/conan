use chrono::{Local, format::StrftimeItems};
use colored::Colorize;

#[derive(PartialEq, Debug)]
enum LogType {
    Debug,
    Info,
    Warn,
    Error,
}

pub struct Log;
impl Log {
    fn log(val: &str, logtype: &LogType) {
        let fmt = StrftimeItems::new("%Y-%m-%d %H:%M:%S");
        let time = Local::now()
            .naive_local()
            .format_with_items(fmt)
            .to_string();
        let lines = val.split('\n');
        let tstring = time.truecolor(100, 100, 100);
        for l in lines {
            match logtype {
                LogType::Debug => {
                    println!("{} [{}] {l}", tstring, "DEBUG".green());
                }
                LogType::Error => {
                    eprintln!("{} [{}] {l}", tstring, "ERROR".red());
                }
                LogType::Info => {
                    println!("{} [{}] {l}", tstring, "INFO".green());
                }
                LogType::Warn => {
                    println!("{} [{}] {l}", tstring, "WARN".yellow());
                }
            }
        }
    }

    pub fn debug(val: &str) {
        Self::log(val, &LogType::Debug);
    }
    pub fn info(val: &str) {
        Self::log(val, &LogType::Info);
    }
    pub fn warn(val: &str) {
        Self::log(val, &LogType::Warn);
    }
    pub fn err(val: &str) {
        Self::log(val, &LogType::Error);
    }
}
