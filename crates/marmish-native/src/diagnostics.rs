//! Opt-in finite diagnostics. Never accepts provider text, keys or payloads.
use std::sync::atomic::{AtomicBool, Ordering};
static ENABLED: AtomicBool = AtomicBool::new(false);

pub fn enable() {
    ENABLED.store(true, Ordering::Relaxed);
    eprintln!(
        "{}",
        serde_json::json!({"schema":1,"event":"context","platform":std::env::consts::OS,"architecture":std::env::consts::ARCH,"version":env!("CARGO_PKG_VERSION")})
    );
}

pub fn event(stage: &'static str, category: &'static str) {
    if ENABLED.load(Ordering::Relaxed) {
        eprintln!(
            "{}",
            serde_json::json!({"schema":1,"event":"diagnostic","stage":stage,"category":category})
        );
    }
}

pub fn stage(current: &mut &'static str, value: &'static str) {
    *current = value;
    event(value, "begin");
}
