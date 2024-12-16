#[macro_export]
macro_rules! log_info {
    ($tag:expr, $($arg:tt)*) => {
        println!("[{} INFO {}] {}", chrono::Local::now().format("%H:%M:%S"), $tag, format!($($arg)*));
    };
}

#[macro_export]
macro_rules! log_error {
    ($tag:expr, $($arg:tt)*) => {
        eprintln!("[{} ERROR {}] {}", chrono::Local::now().format("%H:%M:%S"), $tag, format!($($arg)*));
    };
} 