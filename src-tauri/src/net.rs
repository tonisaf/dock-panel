use std::sync::OnceLock;
use std::time::Duration;

/// Shared HTTP client (Windows SChannel TLS).
pub fn client() -> &'static reqwest::Client {
    static CLIENT: OnceLock<reqwest::Client> = OnceLock::new();
    CLIENT.get_or_init(|| {
        reqwest::Client::builder()
            .timeout(Duration::from_secs(20))
            .user_agent(concat!("DockPanel/", env!("CARGO_PKG_VERSION")))
            .build()
            .expect("reqwest client")
    })
}

/// Async sleep without a direct tokio dependency: parks a blocking-pool thread.
pub async fn sleep(d: Duration) {
    if !d.is_zero() {
        let _ = tauri::async_runtime::spawn_blocking(move || std::thread::sleep(d)).await;
    }
}
