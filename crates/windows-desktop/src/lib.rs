//! Intégration Windows partagée : aucune dépendance à la boîte à outils de l'interface.
pub mod audio;
pub mod badge;
pub mod session;
pub mod soundcheck;

#[cfg(windows)]
pub mod osd;
