//! Intégration Windows partagée : aucune dépendance à la boîte à outils de l'interface.
pub mod app_routes;
pub mod audio;
pub mod badge;
pub mod hotkey;
pub mod keyboard;
pub mod session;
pub mod soundcheck;
pub mod spectrum;
pub mod usage;
pub mod webcam_preview;

#[cfg(windows)]
pub mod osd;

/// Aperçu sur Mac (développement) : la bulle native n'existe que sous Windows.
#[cfg(not(windows))]
pub mod osd {
    pub struct Notice {
        pub title: String,
        pub subtitle: String,
        pub rgb: u32,
        pub icon: &'static str,
    }
    pub fn show(_notice: Notice) {}
}
