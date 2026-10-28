//! Connexions HTTP(S) : TLS natif de Windows (SChannel) sous Windows, rustls ailleurs.
use std::time::Duration;

pub fn agent(timeout: Duration, redirects: u32, user_agent: &str) -> ureq::Agent {
    let b = ureq::AgentBuilder::new().timeout(timeout).redirects(redirects).user_agent(user_agent);
    #[cfg(windows)]
    let b = match native_tls::TlsConnector::new() {
        Ok(c) => b.tls_connector(std::sync::Arc::new(c)),
        Err(_) => b,
    };
    b.build()
}

pub fn ua() -> String {
    format!("Rustudio/{}", crate::config::VERSION)
}
