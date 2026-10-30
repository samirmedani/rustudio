//! Types communs à toutes les routes de l'API et à tous les modules : erreurs, paramètres, réveil de boucle.
use serde_json::Value;
use std::sync::{Condvar, Mutex};
use std::time::Duration;

/// Evenement « il y a du travail » pour la boucle USB.
pub struct Event {
    flag: Mutex<bool>,
    cv: Condvar,
}
impl Event {
    pub fn new() -> Event {
        Event { flag: Mutex::new(false), cv: Condvar::new() }
    }
    pub fn set(&self) {
        *self.flag.lock().unwrap_or_else(|e| e.into_inner()) = true;
        self.cv.notify_all();
    }
    pub fn wait(&self, d: Duration) {
        let g = self.flag.lock().unwrap_or_else(|e| e.into_inner());
        let (mut g, _) = self.cv.wait_timeout_while(g, d, |f| !*f).unwrap_or_else(|e| e.into_inner());
        *g = false;
    }
}
impl Default for Event {
    fn default() -> Self {
        Event::new()
    }
}

#[derive(Debug)]
pub struct ApiError {
    pub status: u16,
    pub msg: String,
}
impl ApiError {
    pub fn bad(m: impl Into<String>) -> ApiError {
        ApiError { status: 400, msg: m.into() }
    }
    pub fn device(m: impl Into<String>) -> ApiError {
        ApiError { status: 409, msg: m.into() }
    }
    pub fn internal(m: impl Into<String>) -> ApiError {
        ApiError { status: 500, msg: m.into() }
    }
}
pub type ApiResult = Result<Value, ApiError>;

pub fn need<'a>(p: &'a Value, k: &str) -> Result<&'a Value, ApiError> {
    match p.get(k) {
        Some(v) if !v.is_null() => Ok(v),
        _ => Err(ApiError::bad(format!("paramètre manquant : {k}"))),
    }
}
pub fn need_str(p: &Value, k: &str) -> Result<String, ApiError> {
    let v = need(p, k)?;
    Ok(v.as_str().map(String::from).unwrap_or_else(|| v.to_string()))
}

pub fn panic_text(p: &Box<dyn std::any::Any + Send>) -> String {
    let m = p.downcast_ref::<&str>().map(|s| s.to_string()).or_else(|| p.downcast_ref::<String>().cloned()).unwrap_or_default();
    format!("erreur interne : {m}")
}
