use std::time::{SystemTime, UNIX_EPOCH};
use std::fmt;

pub enum LogLevel {
    INFO,
    WARN,
    ERROR,
    DEBUG,
}

pub struct Logger;

impl Logger {
    #[cfg(debug_assertions)]
    pub fn log<D: fmt::Display>(level: LogLevel, module: &str, message: D) {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis();
            
        let level_str = match level {
            LogLevel::INFO => "\x1b[32mINFO\x1b[0m",   // 绿色
            LogLevel::WARN => "\x1b[33mWARN\x1b[0m",   // 黄色
            LogLevel::ERROR => "\x1b[31mERROR\x1b[0m", // 红色
            LogLevel::DEBUG => "\x1b[36mDEBUG\x1b[0m", // 青色
        };

        println!(
            "[{} {} {}] {}",
            timestamp,
            level_str,
            module,
            message
        );
    }

    #[cfg(not(debug_assertions))]
    pub fn log<D: fmt::Display>(_level: LogLevel, _module: &str, _message: D) {
        // Release模式下不打印任何日志
    }

    pub fn info(module: &str, fmt: fmt::Arguments<'_>) {
        Self::log(LogLevel::INFO, module, fmt);
    }

    pub fn warn(module: &str, fmt: fmt::Arguments<'_>) {
        Self::log(LogLevel::WARN, module, fmt);
    }

    pub fn error(module: &str, fmt: fmt::Arguments<'_>) {
        Self::log(LogLevel::ERROR, module, fmt);
    }

    pub fn debug(module: &str, fmt: fmt::Arguments<'_>) {
        Self::log(LogLevel::DEBUG, module, fmt);
    }
}

#[macro_export]
macro_rules! log_info {
    ($module:expr, $($arg:tt)*) => ({
        $crate::utils::Logger::info($module, format_args!($($arg)*))
    })
}

#[macro_export]
macro_rules! log_warn {
    ($module:expr, $($arg:tt)*) => ({
        $crate::utils::Logger::warn($module, format_args!($($arg)*))
    })
}

#[macro_export]
macro_rules! log_error {
    ($module:expr, $($arg:tt)*) => ({
        $crate::utils::Logger::error($module, format_args!($($arg)*))
    })
}

#[macro_export]
macro_rules! log_debug {
    ($module:expr, $($arg:tt)*) => ({
        $crate::utils::Logger::debug($module, format_args!($($arg)*))
    })
} 