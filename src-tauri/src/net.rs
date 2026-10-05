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
