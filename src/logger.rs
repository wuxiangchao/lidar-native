// src/logger.rs
use chrono::Local;
use log::{LevelFilter, Log, Metadata, Record, SetLoggerError};
use std::sync::{Arc, Mutex};

// Logger现在会持有一个对外部缓冲区的引用
pub struct EguiLogger {
    buffer: Arc<Mutex<Vec<String>>>,
}

impl Log for EguiLogger {
    fn enabled(&self, _metadata: &Metadata) -> bool {
        true
    }

    fn log(&self, record: &Record) {
        if self.enabled(record.metadata()) {
            let timestamp = Local::now().format("%H:%M:%S");
            let message = format!("[{}] [{}] {}", timestamp, record.level(), record.args());

            let mut buffer = self.buffer.lock().unwrap();
            buffer.push(message);

            if buffer.len() > 200 { // 增加缓冲区大小
                buffer.remove(0);
            }
        }
    }

    fn flush(&self) {}
}

// 初始化函数现在需要传入一个缓冲区
pub fn init(buffer: Arc<Mutex<Vec<String>>>) -> Result<(), SetLoggerError> {
    let logger = EguiLogger { buffer };
    // 使用 set_boxed_logger 来安装一个有状态的 logger
    log::set_boxed_logger(Box::new(logger)).map(|()| log::set_max_level(LevelFilter::Info))
}