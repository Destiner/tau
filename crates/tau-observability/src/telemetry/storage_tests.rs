use std::fs;
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use super::store::{Clock, Signal, Store, StoreConfig, SystemClock};

struct TestClock(Mutex<SystemTime>);
impl Clock for TestClock {
    fn now(&self) -> SystemTime {
        *self.0.lock().unwrap()
    }
}

#[test]
fn production_bounds_and_cross_signal_retention_are_preserved() {
    let production = StoreConfig::production();
    assert_eq!(production.max_age, Duration::from_secs(7 * 24 * 60 * 60));
    assert_eq!(production.max_total_bytes, 256 * 1024 * 1024);
    let root = tempfile::tempdir().unwrap();
    let clock = Arc::new(TestClock(Mutex::new(UNIX_EPOCH + Duration::from_secs(10))));
    let store = Store::with_enabled(
        root.path().to_path_buf(),
        StoreConfig {
            max_age: Duration::from_secs(1),
            max_total_bytes: 512,
            segment_rotation_bytes: 1,
        },
        clock.clone(),
        true,
    );
    for _ in 0..20 {
        assert!(store.append(Signal::Trace, serde_json::json!({"bounded":true})));
        assert!(store.append(Signal::Log, serde_json::json!({"bounded":true})));
    }
    let total: u64 = fs::read_dir(root.path())
        .unwrap()
        .map(|e| e.unwrap().metadata().unwrap().len())
        .sum();
    assert!(total <= 512);
    *clock.0.lock().unwrap() = UNIX_EPOCH + Duration::from_secs(12);
    assert!(store.append(Signal::Metric, serde_json::json!({"bounded":true})));
    assert!(store.read_records(Signal::Trace).is_empty());
    assert!(store.read_records(Signal::Log).is_empty());
    assert_eq!(store.read_records(Signal::Metric).len(), 1);
}

#[test]
fn failed_writer_fallback_is_bounded_and_disabled_store_touches_nothing() {
    let root = tempfile::tempdir().unwrap();
    let dir = root.path().join("telemetry");
    let disabled = Store::with_enabled(
        dir.clone(),
        StoreConfig::production(),
        Arc::new(SystemClock),
        false,
    );
    assert!(disabled.append(Signal::Log, serde_json::json!({})));
    assert!(!dir.exists());
    fs::write(&dir, "block directory creation").unwrap();
    let failed = Store::with_enabled(dir, StoreConfig::production(), Arc::new(SystemClock), true);
    for _ in 0..300 {
        assert!(!failed.append(Signal::Log, serde_json::json!({})));
    }
    assert_eq!(failed.failed_writes(), 300);
    assert_eq!(failed.fallback_len(), 256);
    assert_eq!(failed.fallback_sequences().first(), Some(&44));
}
