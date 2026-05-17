//! 存储模块 - Cookie & LocalStorage

pub mod cookie;
pub mod history;
pub mod local_storage;

pub use cookie::{Cookie, CookieExpires, CookieJar, SameSite, COOKIE_JAR};
pub use history::{default_history_path, History, HistoryEntry, HISTORY};
pub use local_storage::*;
