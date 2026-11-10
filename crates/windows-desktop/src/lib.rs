//! Intégration Windows partagée : aucune dépendance à la boîte à outils de l'interface.
pub mod audio;
pub mod badge;
pub mod hotkey;
pub mod session;
pub mod soundcheck;

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
