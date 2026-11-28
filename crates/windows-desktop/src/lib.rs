//! Intégration Windows partagée : aucune dépendance à la boîte à outils de l'interface.
pub mod app_identity;
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

/// Préférence Windows « Effets d’animation », lue lors de l'ouverture de la fenêtre.
pub fn reduced_motion() -> bool {
    #[cfg(windows)]
    {
        use windows::Win32::UI::WindowsAndMessaging::{
            SPI_GETCLIENTAREAANIMATION, SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS, SystemParametersInfoW,
        };
        let mut enabled = 1i32;
        let result = unsafe {
            SystemParametersInfoW(
                SPI_GETCLIENTAREAANIMATION, 0, Some((&raw mut enabled).cast()),
                SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
            )
        };
        return result.is_ok() && enabled == 0;
    }
    #[cfg(not(windows))]
    false
}

#[cfg(windows)]
pub mod osd;

/// Aperçu sur Mac (développement) : la bulle native n'existe que sous Windows.
#[cfg(not(windows))]
pub mod osd {
    pub struct Notice {
        pub title: &'static str,
        pub rgb: u32,
    }
    pub fn show(_notice: Notice) {}
}
