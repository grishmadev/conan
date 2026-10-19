use chrono::Local;

pub struct Log;
impl Log {
    pub fn debug(val: &str) {
        let time = Local::now();
        let lines = val.split('\n');
        for val in lines {
            println!("{} [DEBUG] {}", time, val);
        }
    }
    pub fn info(val: &str) {
        let time = Local::now();
        let lines = val.split('\n');
        for val in lines {
            println!("{} [INFO] {}", time, val);
        }
    }

    pub fn warn(val: &str) {
        let time = Local::now();
        let lines = val.split('\n');
        for val in lines {
            println!("{} [WARN] {}", time, val);
        }
    }

    pub fn err(val: &str) {
        let time = Local::now();
        let lines = val.split('\n');
        for val in lines {
            eprintln!("{} [ERROR] {}", time, val);
        }
    }
}
