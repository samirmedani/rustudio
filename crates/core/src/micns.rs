//! Suppression du bruit du micro, sur le PC : RNNoise (le module libre qu'utilise OBS), chargé par Equalizer APO
//! sur l'entrée micro de Windows. Discord, OBS et les jeux reçoivent la voix nettoyée ; ça marche avec n'importe
//! quel micro, GoXLR ou non. Ici : les réglages et le fichier de configuration d'Equalizer APO.
use crate::value::{clamp, get, has, truthy};
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Fichier d'Equalizer APO réservé au micro (inclus par config.txt, à côté de celui du casque).
pub const CONFIG_FILE: &str = "rustudio-micro.txt";
/// Module RNNoise pour Windows (werman/noise-suppression-for-voice, GPL-3.0), téléchargé à la demande depuis sa
/// release GitHub, taille et SHA-256 vérifiées.
pub const PLUGIN_URL: &str = "https://github.com/werman/noise-suppression-for-voice/releases/download/v1.22/win-rnnoise.zip";
pub const PLUGIN_SHA256: &str = "7fc1a031438c27cb5731bdd4b627cc890387752c26436fd7e7b29a9c67cc708e";
pub const PLUGIN_SIZE: u64 = 14_047_985;
pub const PLUGIN_MONO: &str = "rnnoise_mono.dll";
pub const PLUGIN_STEREO: &str = "rnnoise_stereo.dll";

/// Intensité par défaut : la probabilité de voix sous laquelle RNNoise rend le silence (90 % conseillé par son
/// auteur entre 85 et 95).
pub const STRENGTH_DEFAULT: i32 = 90;
pub const GRACE_DEFAULT_MS: i32 = 200;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MicNs {
    pub enabled: bool,
    /// seuil de détection de la voix en % : plus haut, plus le bruit entre les mots disparaît franchement
    pub strength: i32,
    /// après ta dernière syllabe, le temps pendant lequel rien n'est coupé (fins de mots)
    pub grace_ms: i32,
    /// entrée Windows visée (identifiant de l'appareil) ; aucune : le micro de la GoXLR, sinon le micro par défaut
    pub device: Option<String>,
}

impl Default for MicNs {
    fn default() -> Self {
        MicNs { enabled: false, strength: STRENGTH_DEFAULT, grace_ms: GRACE_DEFAULT_MS, device: None }
    }
}

pub fn normalize(src: &Value, base: Option<&MicNs>) -> MicNs {
    let mut ns = base.cloned().unwrap_or_default();
    if !src.is_object() {
        return ns;
    }
    if has(src, "enabled") {
        ns.enabled = truthy(get(src, "enabled"));
    }
    if let Some(s) = clamp(get(src, "strength"), 0, 99) {
        ns.strength = s;
    }
    if let Some(g) = clamp(get(src, "grace_ms"), 0, 1000) {
        ns.grace_ms = g;
    }
    if has(src, "device") {
        ns.device = get(src, "device").as_str().map(str::trim).filter(|d| is_guid(d)).map(|d| d.to_lowercase());
    }
    ns
}

/// « {xxxxxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxxx} » : seul format accepté pour viser une entrée Windows.
pub fn is_guid(s: &str) -> bool {
    let b = s.as_bytes();
    b.len() == 38
        && b[0] == b'{'
        && b[37] == b'}'
        && s[1..37].char_indices().all(|(i, c)| if matches!(i, 8 | 13 | 18 | 23) { c == '-' } else { c.is_ascii_hexdigit() })
}

/// Contenu de rustudio-micro.txt : RNNoise sur la seule entrée visée (par son identifiant : jamais une sortie du
/// même nom), ou rien. `target` : identifiant de l'entrée et son nombre de voies.
pub fn config_text(ns: &MicNs, target: Option<(&str, u16)>, plugin_dir: &str, why_off: &str) -> String {
    let mut lines = vec!["# Rustudio : suppression du bruit du micro. Fichier genere : regle-la dans Rustudio (page Micro).".to_string()];
    let clean = |s: &str| s.chars().filter(|c| !matches!(c, '"' | '\r' | '\n')).collect::<String>();
    match target.filter(|_| ns.enabled) {
        Some((guid, channels)) => {
            let plugin = if channels >= 2 { PLUGIN_STEREO } else { PLUGIN_MONO };
            let dir = clean(plugin_dir);
            let dir = dir.trim_end_matches(['\\', '/']);
            lines.push(format!("Device: {}", clean(guid).to_uppercase()));
            lines.push(format!(
                "VSTPlugin: Library \"{dir}\\{plugin}\" \"VAD Threshold (%)\" {} \"VAD Grace Period (ms)\" {} \"Retroactive VAD Grace (ms)\" 0",
                ns.strength.clamp(0, 99),
                ns.grace_ms.clamp(0, 1000)
            ));
            lines.push("Device: all".into());
        }
        None => {
            let reason = clean(why_off);
            lines.push(format!("# Suppression coupee{}.", if reason.is_empty() { String::new() } else { format!(" ({reason})") }));
        }
    }
    let text = lines.join("\n") + "\n";
    text.chars().map(|c| if c.is_ascii() { c } else { '?' }).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn reglages_bornes_et_entree_valide() {
        let ns = normalize(&json!({"enabled": true, "strength": 250, "grace_ms": -5, "device": "{CC214E79-D5E6-45C0-B617-DBB4992E5A7E}"}), None);
        assert!(ns.enabled);
        assert_eq!((ns.strength, ns.grace_ms), (99, 0));
        assert_eq!(ns.device.as_deref(), Some("{cc214e79-d5e6-45c0-b617-dbb4992e5a7e}"));
        assert_eq!(normalize(&json!({"device": "Micro\" ; Device: all"}), Some(&ns)).device, None, "rien d'autre qu'un identifiant");
    }

    #[test]
    fn configuration_visant_la_seule_entree_du_micro() {
        let ns = MicNs { enabled: true, ..Default::default() };
        let t = config_text(&ns, Some(("{cc214e79-d5e6-45c0-b617-dbb4992e5a7e}", 1)), r"C:\ProgramData\Rustudio\rnnoise", "");
        assert!(t.contains("Device: {CC214E79-D5E6-45C0-B617-DBB4992E5A7E}\n"));
        assert!(t.contains(r#"Library "C:\ProgramData\Rustudio\rnnoise\rnnoise_mono.dll" "VAD Threshold (%)" 90 "VAD Grace Period (ms)" 200"#));
        assert!(t.trim_end().ends_with("Device: all"));
        assert!(config_text(&ns, Some(("{a}", 2)), "x", "").contains("rnnoise_stereo.dll"));
        let off = config_text(&MicNs::default(), Some(("{a}", 1)), "x", "désactivée");
        assert!(!off.contains("VSTPlugin") && !off.contains("Device:"));
    }

    #[test]
    fn config_text_borne_valeurs_et_neutralise_injections() {
        let ns = MicNs { enabled: true, strength: 150, grace_ms: -20, device: None };
        let t = config_text(&ns, Some(("{a}\nDevice: all", 1)), "C:\\dir\"/\\", "");
        assert!(t.contains("\"VAD Threshold (%)\" 99 \"VAD Grace Period (ms)\" 0"), "{t}");
        assert!(t.contains(r#"Library "C:\dir\rnnoise_mono.dll""#), "{t}");
        assert_eq!(t.lines().filter(|l| l.starts_with("Device:")).count(), 2, "{t}");
    }
}
