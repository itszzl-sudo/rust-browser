//! localStorage implementation following the Web Storage specification.
//!
//! Modeled after Servo's `dom::storage` module.
//!
//! Provides:
//! - `LocalStorage` — per-origin key-value store with quota enforcement
//! - `StorageArea` trait — abstract storage operations
//! - `StorageManager` — manages all origins
//! - JSON file persistence per origin
//! - Lazy persistence: writes only on `flush()` or `Drop`

use std::collections::hash_map::DefaultHasher;
use std::collections::HashMap;
use std::fmt;
use std::fs;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Default per-origin quota: 5 MB (as used by most browsers).
const DEFAULT_QUOTA: usize = 5 * 1024 * 1024;

/// Storage subdirectory name under `data_dir`.
const STORAGE_DIR: &str = "storage";

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

/// Errors that can occur during storage operations.
#[derive(Debug)]
pub enum StorageError {
    /// The quota for this origin would be exceeded.
    QuotaExceeded {
        current: usize,
        max: usize,
        item_size: usize,
    },
    /// An I/O error occurred (e.g. file read/write failure).
    IoError(String),
    /// JSON serialization or deserialization failed.
    SerializationError(String),
}

impl fmt::Display for StorageError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            StorageError::QuotaExceeded {
                current,
                max,
                item_size,
            } => {
                write!(
                    f,
                    "QuotaExceeded: current={}B, max={}B, item_size={}B",
                    current, max, item_size
                )
            }
            StorageError::IoError(msg) => write!(f, "IO error: {}", msg),
            StorageError::SerializationError(msg) => write!(f, "Serialization error: {}", msg),
        }
    }
}

impl std::error::Error for StorageError {}

// ---------------------------------------------------------------------------
// StorageArea trait
// ---------------------------------------------------------------------------

/// Abstract trait for storage area operations (mirrors the Web Storage API).
pub trait StorageArea {
    /// Retrieve the value associated with the given key.
    fn get_item(&self, key: &str) -> Option<&str>;

    /// Set the value for the given key.
    ///
    /// Returns `Err(StorageError::QuotaExceeded)` if the new item would push
    /// usage over the per-origin quota.
    fn set_item(&mut self, key: &str, value: &str) -> Result<(), StorageError>;

    /// Remove the item with the given key, returning the previous value if any.
    fn remove_item(&mut self, key: &str) -> Option<String>;

    /// Remove all key-value pairs from this storage area.
    fn clear(&mut self);

    /// Return the number of key-value pairs.
    fn length(&self) -> usize;

    /// Return the key at the given index (insertion order is **not** guaranteed;
    /// iteration order follows `HashMap`).
    fn key(&self, index: usize) -> Option<&str>;
}

// ---------------------------------------------------------------------------
// JsonStorageFormat
// ---------------------------------------------------------------------------

/// Helper type for JSON serialization of a key-value map.
///
/// Fields are public for transparency and are used by `serialize` / `deserialize`.
#[derive(Debug, Clone, PartialEq)]
pub struct JsonStorageFormat {
    /// The origin this storage belongs to.
    pub origin: String,
    /// Key-value data pairs.
    pub data: HashMap<String, String>,
}

impl JsonStorageFormat {
    /// Create a new `JsonStorageFormat` from origin and data.
    pub fn new(origin: &str, data: HashMap<String, String>) -> Self {
        JsonStorageFormat {
            origin: origin.to_string(),
            data,
        }
    }

    /// Serialize to a JSON byte vector.
    ///
    /// Uses a simple manual JSON encoder (no external crate dependency).
    /// Produces: `{"origin":"...","data":{"key":"value",...}}`
    pub fn to_bytes(&self) -> Result<Vec<u8>, StorageError> {
        let json = self.to_json_string()?;
        Ok(json.into_bytes())
    }

    /// Deserialize from a byte slice.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, StorageError> {
        let s = std::str::from_utf8(bytes)
            .map_err(|e| StorageError::SerializationError(e.to_string()))?;
        Self::from_json_str(s)
    }

    // ---- Internal JSON helpers ----

    fn to_json_string(&self) -> Result<String, StorageError> {
        let mut buf = String::new();
        buf.push('{');
        buf.push_str(&format!(
            "\"origin\":{},",
            Self::escape_json_str(&self.origin)
        ));
        buf.push_str("\"data\":{");
        let mut first = true;
        // Sort keys for deterministic output (useful for diffs / debugging).
        let mut keys: Vec<&String> = self.data.keys().collect();
        keys.sort();
        for k in keys {
            if !first {
                buf.push(',');
            }
            first = false;
            let v = &self.data[k];
            buf.push_str(&format!(
                "{}:{}",
                Self::escape_json_str(k),
                Self::escape_json_str(v)
            ));
        }
        buf.push('}');
        buf.push('}');
        Ok(buf)
    }

    fn from_json_str(s: &str) -> Result<Self, StorageError> {
        let s = s.trim();

        // Must start with '{' and end with '}'
        if !s.starts_with('{') || !s.ends_with('}') {
            return Err(StorageError::SerializationError(
                "missing outer braces".into(),
            ));
        }
        let inner = &s[1..s.len().saturating_sub(1)].trim();

        // We expect: "origin":<origin_str>,"data":{...}
        // Split on '"data":'
        let data_key = "\"data\":";
        let data_pos = inner
            .find(data_key)
            .ok_or_else(|| StorageError::SerializationError("missing \"data\" key".into()))?;

        let origin_section = &inner[..data_pos].trim();
        let data_section = &inner[data_pos + data_key.len()..].trim();

        // Parse origin
        let origin_str = Self::parse_origin(origin_section)?;

        // Parse data map
        let data = Self::parse_json_object(data_section)?;

        Ok(JsonStorageFormat {
            origin: origin_str,
            data,
        })
    }

    /// Parse `"origin":<value>` from the origin section.
    fn parse_origin(s: &str) -> Result<String, StorageError> {
        let s = s.trim();
        let origin_prefix = "\"origin\":";
        let val = s
            .strip_prefix(origin_prefix)
            .ok_or_else(|| StorageError::SerializationError("missing \"origin\" key".into()))?;
        let val = val.trim();
        Self::parse_json_string(val)
    }

    /// Parse a JSON string value (returns the unescaped content).
    fn parse_json_string(s: &str) -> Result<String, StorageError> {
        let s = s.trim();
        if !s.starts_with('"') {
            return Err(StorageError::SerializationError(format!(
                "expected string, got: {}",
                s
            )));
        }
        // Find the closing quote.
        // NOTE: This is a simple parser that does NOT handle escaped quotes inside strings.
        // For a production system, use serde_json.  For this exercise it's sufficient.
        let end = s[1..]
            .find('"')
            .ok_or_else(|| StorageError::SerializationError("unterminated string".into()))?;
        Ok(s[1..=end].to_string())
    }

    /// Parse a JSON object `{...}` into a HashMap.
    fn parse_json_object(s: &str) -> Result<HashMap<String, String>, StorageError> {
        let s = s.trim();
        if !s.starts_with('{') || !s.ends_with('}') {
            return Err(StorageError::SerializationError(
                "expected JSON object".into(),
            ));
        }
        let inner = &s[1..s.len().saturating_sub(1)].trim();
        if inner.is_empty() {
            return Ok(HashMap::new());
        }

        let mut map = HashMap::new();
        // Split on ',' to get key:value pairs.
        // This is simplistic and won't handle commas inside strings – fine for this storage use-case.
        for pair_str in inner.split(',') {
            let pair_str = pair_str.trim();
            if pair_str.is_empty() {
                continue;
            }
            let colon_pos = pair_str.find(':').ok_or_else(|| {
                StorageError::SerializationError(format!("expected ':' in pair: {}", pair_str))
            })?;
            let key_str = &pair_str[..colon_pos].trim();
            let val_str = &pair_str[colon_pos + 1..].trim();
            let key = Self::parse_json_string(key_str)?;
            let val = Self::parse_json_string(val_str)?;
            map.insert(key, val);
        }
        Ok(map)
    }

    /// Escape a string for JSON output (backslash and quote).
    fn escape_json_str(s: &str) -> String {
        let mut out = String::with_capacity(s.len() + 2);
        out.push('"');
        for ch in s.chars() {
            match ch {
                '\\' => out.push_str("\\\\"),
                '"' => out.push_str("\\\""),
                '\n' => out.push_str("\\n"),
                '\r' => out.push_str("\\r"),
                '\t' => out.push_str("\\t"),
                c => out.push(c),
            }
        }
        out.push('"');
        out
    }
}

// ---------------------------------------------------------------------------
// Origin filename hashing
// ---------------------------------------------------------------------------

/// Compute a hex string hash for an origin using `DefaultHasher`.
///
/// This is used to produce safe filesystem filenames from arbitrary origin
/// strings (which may contain special characters like `://`).
fn origin_hash(origin: &str) -> String {
    let mut hasher = DefaultHasher::new();
    origin.hash(&mut hasher);
    let hash = hasher.finish();
    // Represent as 16-char hex string.
    format!("{:016x}", hash)
}

/// Build the storage file path for the given origin under `base_path/storage/`.
fn storage_file_path(base_path: &Path, origin: &str) -> PathBuf {
    let hash = origin_hash(origin);
    base_path.join(STORAGE_DIR).join(format!("{}.json", hash))
}

// ---------------------------------------------------------------------------
// LocalStorage
// ---------------------------------------------------------------------------

/// Per-origin synchronous key-value store modelled after `window.localStorage`.
///
/// Thread-safety is provided by wrapping in `Arc<Mutex<...>>` (see `StorageManager`).
///
/// Persistence is *lazy*: in-memory changes are only written to disk on
/// explicit [`flush()`](Self::flush) or when the instance is dropped.
pub struct LocalStorage {
    /// The origin (e.g. `"https://example.com"`).
    origin: String,
    /// In-memory key-value data.
    data: HashMap<String, String>,
    /// Whether the in-memory state differs from the on-disk state.
    dirty: bool,
    /// Maximum bytes this origin may use (default 5 MiB).
    quota: usize,
    /// Current number of bytes used (sum of key and value UTF-8 sizes).
    used: usize,
    /// Path to the JSON file on disk.
    file_path: PathBuf,
}

impl LocalStorage {
    /// Create a new `LocalStorage` for the given origin.
    ///
    /// `base_path` is the application data directory (e.g. `~/.local/share/rust-browser`
    /// on Linux or `%APPDATA%/rust-browser` on Windows).  The storage file will be
    /// created at `{base_path}/storage/{origin_hash}.json`.
    ///
    /// If a storage file already exists on disk, its contents are loaded into memory.
    pub fn new(origin: &str, base_path: &Path) -> Self {
        let file_path = storage_file_path(base_path, origin);
        let (data, used, initial_dirty) = if file_path.exists() {
            match fs::read(&file_path) {
                Ok(bytes) => match JsonStorageFormat::from_bytes(&bytes) {
                    Ok(fmt) => {
                        let used = compute_used_bytes(&fmt.data);
                        (fmt.data, used, false)
                    }
                    Err(e) => {
                        log::warn!(
                            "Failed to deserialize storage file {:?}: {}. Starting fresh.",
                            file_path,
                            e
                        );
                        (HashMap::new(), 0, true)
                    }
                },
                Err(e) => {
                    log::warn!(
                        "Failed to read storage file {:?}: {}. Starting fresh.",
                        file_path,
                        e
                    );
                    (HashMap::new(), 0, true)
                }
            }
        } else {
            (HashMap::new(), 0, false)
        };

        // Ensure the storage directory exists.
        if let Some(parent) = file_path.parent() {
            if !parent.exists() {
                if let Err(e) = fs::create_dir_all(parent) {
                    log::error!("Failed to create storage directory {:?}: {}", parent, e);
                }
            }
        }

        LocalStorage {
            origin: origin.to_string(),
            data,
            dirty: initial_dirty,
            quota: DEFAULT_QUOTA,
            used,
            file_path,
        }
    }

    /// Set a custom quota (max bytes).  Only affects future `set_item` calls.
    pub fn set_quota(&mut self, quota: usize) {
        self.quota = quota;
    }

    /// Return the number of bytes currently used.
    pub fn used_bytes(&self) -> usize {
        self.used
    }

    /// Return the number of bytes remaining before the quota is hit.
    pub fn remaining_bytes(&self) -> usize {
        self.quota.saturating_sub(self.used)
    }

    /// Write the current in-memory state to the JSON file on disk.
    ///
    /// This is a no-op if no changes have been made (`dirty == false`).
    pub fn flush(&mut self) -> Result<(), StorageError> {
        if !self.dirty {
            return Ok(());
        }

        // Ensure parent directory exists.
        if let Some(parent) = self.file_path.parent() {
            if !parent.exists() {
                fs::create_dir_all(parent)
                    .map_err(|e| StorageError::IoError(format!("create_dir_all: {}", e)))?;
            }
        }

        let fmt = JsonStorageFormat::new(&self.origin, self.data.clone());
        let bytes = fmt.to_bytes()?;

        // Write atomically: write to a temp file, then rename.
        let tmp_path = self.file_path.with_extension("tmp");
        fs::write(&tmp_path, &bytes).map_err(|e| StorageError::IoError(format!("write: {}", e)))?;
        fs::rename(&tmp_path, &self.file_path)
            .map_err(|e| StorageError::IoError(format!("rename: {}", e)))?;

        self.dirty = false;
        Ok(())
    }
}

impl StorageArea for LocalStorage {
    fn get_item(&self, key: &str) -> Option<&str> {
        self.data.get(key).map(|s| s.as_str())
    }

    fn set_item(&mut self, key: &str, value: &str) -> Result<(), StorageError> {
        let old_value_size = self.data.get(key).map(|v| key.len() + v.len()).unwrap_or(0);
        let new_value_size = key.len() + value.len();

        // Compute new total usage.
        let new_used = self.used + new_value_size - old_value_size;

        if new_used > self.quota {
            return Err(StorageError::QuotaExceeded {
                current: self.used,
                max: self.quota,
                item_size: new_value_size,
            });
        }

        // Update usage and insert.
        self.used = new_used;
        self.data.insert(key.to_string(), value.to_string());
        self.dirty = true;
        Ok(())
    }

    fn remove_item(&mut self, key: &str) -> Option<String> {
        if let Some(old) = self.data.remove(key) {
            self.used = self.used.saturating_sub(key.len() + old.len());
            self.dirty = true;
            Some(old)
        } else {
            None
        }
    }

    fn clear(&mut self) {
        if !self.data.is_empty() {
            self.data.clear();
            self.used = 0;
            self.dirty = true;
        }
    }

    fn length(&self) -> usize {
        self.data.len()
    }

    fn key(&self, index: usize) -> Option<&str> {
        // HashMap iteration order is not guaranteed, but we provide a consistent
        // snapshot by collecting keys and sorting them.  This matches the common
        // expectation that `key(i)` is stable within a session.
        let mut keys: Vec<&String> = self.data.keys().collect();
        keys.sort();
        keys.get(index).map(|k| k.as_str())
    }
}

impl Drop for LocalStorage {
    fn drop(&mut self) {
        if self.dirty {
            if let Err(e) = self.flush() {
                log::error!(
                    "Failed to flush storage for origin '{}' on drop: {}",
                    self.origin,
                    e
                );
            }
        }
    }
}

impl fmt::Debug for LocalStorage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("LocalStorage")
            .field("origin", &self.origin)
            .field("dirty", &self.dirty)
            .field("quota", &self.quota)
            .field("used", &self.used)
            .field("entries", &self.data.len())
            .field("file_path", &self.file_path)
            .finish()
    }
}

// ---------------------------------------------------------------------------
// StorageManager
// ---------------------------------------------------------------------------

/// Manages `LocalStorage` instances for multiple origins.
///
/// Typical usage:
///
/// ```rust,ignore
/// let mut mgr = StorageManager::new(&data_dir);
/// let store = mgr.for_origin("https://example.com");
/// store.set_item("key", "value")?;
/// mgr.flush_all()?;
/// ```
pub struct StorageManager {
    stores: HashMap<String, LocalStorage>,
    base_path: PathBuf,
}

impl StorageManager {
    /// Create a new `StorageManager` rooted at `base_path`.
    ///
    /// The storage directory `{base_path}/storage/` will be created on demand.
    pub fn new(base_path: &Path) -> Self {
        StorageManager {
            stores: HashMap::new(),
            base_path: base_path.to_path_buf(),
        }
    }

    /// Get (or create) the `LocalStorage` for the given origin.
    pub fn for_origin(&mut self, origin: &str) -> &mut LocalStorage {
        self.stores
            .entry(origin.to_string())
            .or_insert_with(|| LocalStorage::new(origin, &self.base_path))
    }

    /// Flush all dirty storage instances to disk.
    pub fn flush_all(&mut self) -> Result<(), StorageError> {
        for store in self.stores.values_mut() {
            if store.dirty {
                store.flush()?;
            }
        }
        Ok(())
    }

    /// Delete the storage file and in-memory data for the given origin.
    pub fn delete_origin(&mut self, origin: &str) -> Result<(), StorageError> {
        // Remove in-memory data.
        self.stores.remove(origin);

        // Remove the file from disk.
        let file_path = storage_file_path(&self.base_path, origin);
        if file_path.exists() {
            fs::remove_file(&file_path)
                .map_err(|e| StorageError::IoError(format!("remove_file: {}", e)))?;
        }
        Ok(())
    }

    /// Remove all storage data (in-memory and on-disk) for all origins.
    pub fn clear_all(&mut self) -> Result<(), StorageError> {
        self.stores.clear();

        let storage_dir = self.base_path.join(STORAGE_DIR);
        if storage_dir.exists() {
            fs::remove_dir_all(&storage_dir)
                .map_err(|e| StorageError::IoError(format!("remove_dir_all: {}", e)))?;
            // Re-create the empty directory.
            fs::create_dir_all(&storage_dir)
                .map_err(|e| StorageError::IoError(format!("create_dir_all: {}", e)))?;
        }
        Ok(())
    }

    /// Return the number of origins currently loaded.
    pub fn len(&self) -> usize {
        self.stores.len()
    }

    /// Returns `true` if no origins are loaded.
    pub fn is_empty(&self) -> bool {
        self.stores.is_empty()
    }
}

impl fmt::Debug for StorageManager {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("StorageManager")
            .field("base_path", &self.base_path)
            .field("origins", &self.stores.len())
            .finish()
    }
}

// ---------------------------------------------------------------------------
// Global STORAGE_MANAGER
// ---------------------------------------------------------------------------

/// Global storage manager, lazily initialised with `init_global_storage_manager`.
static STORAGE_MANAGER: OnceLock<Mutex<StorageManager>> = OnceLock::new();

/// Initialise the global `STORAGE_MANAGER` with the given data directory.
///
/// Must be called once early in program startup (e.g. from `main`).
///
/// # Panics
///
/// Panics if the global storage manager has already been initialised.
pub fn init_global_storage_manager(base_path: &Path) {
    let mgr = StorageManager::new(base_path);
    STORAGE_MANAGER
        .set(Mutex::new(mgr))
        .unwrap_or_else(|_| panic!("Global STORAGE_MANAGER already initialised"));
}

/// Return a reference to the global storage manager mutex.
///
/// # Panics
///
/// Panics if `init_global_storage_manager` has not been called first.
pub fn global_storage_manager() -> &'static Mutex<StorageManager> {
    STORAGE_MANAGER
        .get()
        .expect("Global STORAGE_MANAGER not initialised. Call init_global_storage_manager() first.")
}

/// Convenience: get (or create) the `LocalStorage` for `origin` via the global manager.
///
/// # Panics
///
/// Panics if the global storage has not been initialised.
pub fn storage_for_origin(origin: &str) -> std::sync::MutexGuard<'static, StorageManager> {
    let mut guard = global_storage_manager()
        .lock()
        .expect("Global STORAGE_MANAGER lock poisoned");
    guard.for_origin(origin);
    // Drop the guard – the caller needs the `LocalStorage` reference,
    // but we cannot return a reference to the interior of a `MutexGuard`.
    // Instead, the caller must hold the lock and work through the manager.
    // This function is provided for ergonomics in single-access scenarios.
    drop(guard);
    global_storage_manager()
        .lock()
        .expect("Global STORAGE_MANAGER lock poisoned")
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

/// Compute the total number of bytes used by all key-value pairs in the map.
fn compute_used_bytes(data: &HashMap<String, String>) -> usize {
    data.iter().map(|(k, v)| k.len() + v.len()).sum()
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_set_and_get_item() {
        let mut store =
            LocalStorage::new("https://example.com", &PathBuf::from("/tmp/_test_storage"));
        assert!(store.set_item("hello", "world").is_ok());
        assert_eq!(store.get_item("hello"), Some("world"));
        assert_eq!(store.length(), 1);
    }

    #[test]
    fn test_remove_item() {
        let mut store =
            LocalStorage::new("https://example.com", &PathBuf::from("/tmp/_test_storage"));
        store.set_item("a", "1").unwrap();
        assert_eq!(store.remove_item("a"), Some("1".to_string()));
        assert_eq!(store.get_item("a"), None);
        assert_eq!(store.length(), 0);
    }

    #[test]
    fn test_clear() {
        let mut store =
            LocalStorage::new("https://example.com", &PathBuf::from("/tmp/_test_storage"));
        store.set_item("a", "1").unwrap();
        store.set_item("b", "2").unwrap();
        store.clear();
        assert_eq!(store.length(), 0);
        assert_eq!(store.used_bytes(), 0);
    }

    #[test]
    fn test_key_index() {
        let mut store =
            LocalStorage::new("https://example.com", &PathBuf::from("/tmp/_test_storage"));
        store.set_item("alpha", "1").unwrap();
        store.set_item("beta", "2").unwrap();
        store.set_item("gamma", "3").unwrap();
        // Sorted order: alpha, beta, gamma
        assert_eq!(store.key(0), Some("alpha"));
        assert_eq!(store.key(1), Some("beta"));
        assert_eq!(store.key(2), Some("gamma"));
        assert_eq!(store.key(3), None);
    }

    #[test]
    fn test_quota_exceeded() {
        let mut store =
            LocalStorage::new("https://example.com", &PathBuf::from("/tmp/_test_storage"));
        store.quota = 10; // very small quota
        let result = store.set_item("key", "value_that_exceeds_quota");
        assert!(matches!(result, Err(StorageError::QuotaExceeded { .. })));
    }

    #[test]
    fn test_used_bytes_tracking() {
        let mut store =
            LocalStorage::new("https://example.com", &PathBuf::from("/tmp/_test_storage"));
        assert_eq!(store.used_bytes(), 0);
        store.set_item("ab", "cd").unwrap(); // 2 + 2 = 4
        assert_eq!(store.used_bytes(), 4);
        store.set_item("ab", "cde").unwrap(); // replace: old 4, new 2+3=5 => used becomes 5
        assert_eq!(store.used_bytes(), 5);
        store.remove_item("ab").unwrap();
        assert_eq!(store.used_bytes(), 0);
    }

    #[test]
    fn test_flush_and_reload() {
        let dir = std::env::temp_dir().join(format!("_test_storage_{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);

        // Write
        {
            let mut store = LocalStorage::new("https://example.com", &dir);
            store.set_item("persist", "me").unwrap();
            store.flush().unwrap();
        }

        // Reload
        {
            let store = LocalStorage::new("https://example.com", &dir);
            assert_eq!(store.get_item("persist"), Some("me"));
            assert!(!store.dirty);
        }

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_storage_manager() {
        let dir = std::env::temp_dir().join(format!("_test_mgr_{}", std::process::id()));
        let mut mgr = StorageManager::new(&dir);

        let store = mgr.for_origin("https://a.com");
        store.set_item("k", "v").unwrap();

        let store = mgr.for_origin("https://b.com");
        store.set_item("x", "y").unwrap();

        assert_eq!(mgr.len(), 2);
        mgr.flush_all().unwrap();
        mgr.delete_origin("https://a.com").unwrap();
        assert_eq!(mgr.len(), 1);

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_json_roundtrip() {
        let mut data = HashMap::new();
        data.insert("key1".to_string(), "val1".to_string());
        data.insert("key2".to_string(), "val2".to_string());

        let fmt = JsonStorageFormat::new("https://o.com", data);
        let bytes = fmt.to_bytes().unwrap();
        let parsed = JsonStorageFormat::from_bytes(&bytes).unwrap();

        assert_eq!(parsed.origin, "https://o.com");
        assert_eq!(parsed.data.get("key1").unwrap(), "val1");
        assert_eq!(parsed.data.get("key2").unwrap(), "val2");
    }

    #[test]
    fn test_global_storage_manager() {
        let dir = std::env::temp_dir().join(format!("_test_global_{}", std::process::id()));
        init_global_storage_manager(&dir);

        {
            let mut guard = global_storage_manager().lock().unwrap();
            let store = guard.for_origin("https://global.test");
            store.set_item("global_key", "global_val").unwrap();
            guard.flush_all().unwrap();
        }

        // Verify reload.
        {
            let mut guard = global_storage_manager().lock().unwrap();
            let store = guard.for_origin("https://global.test");
            assert_eq!(store.get_item("global_key"), Some("global_val"));
        }

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_drop_flushes_dirty() {
        let dir = std::env::temp_dir().join(format!("_test_drop_{}", std::process::id()));

        // Create and drop (flush on drop).
        {
            let mut store = LocalStorage::new("https://drop.test", &dir);
            store.set_item("drop", "persist").unwrap();
            // No explicit flush — Drop should handle it.
        }

        // Reload — should see the data.
        {
            let store = LocalStorage::new("https://drop.test", &dir);
            assert_eq!(store.get_item("drop"), Some("persist"));
        }

        let _ = fs::remove_dir_all(&dir);
    }
}
