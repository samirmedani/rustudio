//! Effet système de capture. Le seul point d'entrée de traitement reçoit des buffers de l'hôte.
#[cfg(windows)]
mod aggregate;
#[cfg(windows)]
mod apo;
mod limiter;
mod noise;
pub mod stream;
