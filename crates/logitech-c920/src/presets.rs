//! Préréglages intelligents pour la Logitech C920 : un clic pour adapter l'image, l'exposition et le focus.
//! Chaque valeur est automatiquement recalée sur les bornes réelles de la webcam lors de l'application.
use crate::{ControlPatch, LedMode, PowerLineFreq};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Preset {
    pub id: &'static str,
    pub label: &'static str,
    pub summary: &'static str,
    pub badge: Option<&'static str>,
    pub patch: ControlPatch,
}

/// Liste des préréglages intégrés, dans l'ordre d'affichage.
pub fn all() -> Vec<Preset> {
    vec![
        Preset {
            id: "studio",
            label: "Studio / Stream",
            summary: "Exposition manuelle fluide (30 ips réels), basse lumière coupée, balance verrouillée et 50 Hz.",
            badge: Some("Conseillé"),
            patch: ControlPatch {
                brightness: Some(128),
                contrast: Some(134),
                saturation: Some(132),
                sharpness: Some(136),
                gain: Some(48),
                white_balance_auto: Some(false),
                white_balance: Some(4200),
                backlight_comp: Some(false),
                power_line: Some(PowerLineFreq::Hz50),
                exposure_auto: Some(false),
                exposure: Some(-6),
                low_light_comp: Some(false),
                focus_auto: Some(false),
                focus: Some(25),
                zoom: Some(100),
                pan: Some(0),
                tilt: Some(0),
                led_mode: None,
            },
        },
        Preset {
            id: "low-light",
            label: "Faible lumière",
            summary: "Plus de luminosité et de gain en soirée, tout en gardant un flux fluide sans flou de mouvement.",
            badge: None,
            patch: ControlPatch {
                brightness: Some(140),
                contrast: Some(126),
                saturation: Some(124),
                sharpness: Some(120),
                gain: Some(160),
                white_balance_auto: Some(false),
                white_balance: Some(4000),
                backlight_comp: Some(true),
                power_line: Some(PowerLineFreq::Hz50),
                exposure_auto: Some(false),
                exposure: Some(-5),
                low_light_comp: Some(false),
                focus_auto: Some(true),
                focus: None,
                zoom: None,
                pan: None,
                tilt: None,
                led_mode: None,
            },
        },
        Preset {
            id: "natural",
            label: "Naturel",
            summary: "Rendu neutre pour les appels quotidiens : exposition, balance et mise au point automatiques.",
            badge: None,
            patch: ControlPatch {
                brightness: Some(128),
                contrast: Some(128),
                saturation: Some(128),
                sharpness: Some(128),
                gain: Some(0),
                white_balance_auto: Some(true),
                white_balance: None,
                backlight_comp: Some(false),
                power_line: Some(PowerLineFreq::Hz50),
                exposure_auto: Some(true),
                exposure: None,
                low_light_comp: Some(false),
                focus_auto: Some(true),
                focus: None,
                zoom: Some(100),
                pan: Some(0),
                tilt: Some(0),
                led_mode: None,
            },
        },
        Preset {
            id: "cinema",
            label: "Cinéma",
            summary: "Contraste soutenu, netteté douce sans halo numérique et couleur verrouillée.",
            badge: None,
            patch: ControlPatch {
                brightness: Some(120),
                contrast: Some(148),
                saturation: Some(138),
                sharpness: Some(116),
                gain: Some(40),
                white_balance_auto: Some(false),
                white_balance: Some(4600),
                backlight_comp: Some(false),
                power_line: Some(PowerLineFreq::Hz50),
                exposure_auto: Some(false),
                exposure: Some(-6),
                low_light_comp: Some(false),
                focus_auto: Some(false),
                focus: Some(25),
                zoom: None,
                pan: None,
                tilt: None,
                led_mode: None,
            },
        },
    ]
}

pub fn find(id: &str) -> Option<Preset> {
    all().into_iter().find(|p| p.id == id)
}

/// Informations compactes pour l'API et l'interface.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PresetSummary {
    pub id: String,
    pub label: String,
    pub summary: String,
    pub badge: Option<String>,
}

pub fn summaries() -> Vec<PresetSummary> {
    all()
        .into_iter()
        .map(|p| PresetSummary {
            id: p.id.to_owned(),
            label: p.label.to_owned(),
            summary: p.summary.to_owned(),
            badge: p.badge.map(str::to_owned),
        })
        .collect()
}

/// Convertit un mode LED texte ou numérique en `LedMode`.
pub fn parse_led_mode(v: &serde_json::Value) -> Option<LedMode> {
    if let Some(s) = v.as_str() {
        return match s {
            "off" => Some(LedMode::Off),
            "on" => Some(LedMode::On),
            "blink" => Some(LedMode::Blink),
            "auto" => Some(LedMode::Auto),
            _ => None,
        };
    }
    match v.as_u64()? {
        0 => Some(LedMode::Off),
        1 => Some(LedMode::On),
        2 => Some(LedMode::Blink),
        3 => Some(LedMode::Auto),
        _ => None,
    }
}
