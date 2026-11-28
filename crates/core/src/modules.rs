//! Registre des modules compiles : seule cette liste fait autorite sur ce que Rustudio peut piloter.
//! Le catalogue est livré avec la version validée ; il ne charge jamais de code distant.
use serde::Serialize;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ModuleOrigin {
    Official,
    Community,
}
impl ModuleOrigin {
    pub fn as_str(self) -> &'static str {
        match self { Self::Official => "official", Self::Community => "community" }
    }
}

#[derive(Clone, Copy, Debug, Serialize)]
pub struct ModulePage {
    pub id: &'static str,
    pub label: &'static str,
    pub icon: &'static str,
}

#[derive(Clone, Copy, Debug, Serialize)]
pub struct ModuleDescriptor {
    pub id: &'static str,
    pub name: &'static str,
    pub vendor: &'static str,
    pub kind: &'static str,
    pub origin: ModuleOrigin,
    /// Pseudo GitHub public du créateur ; jamais une identité privée ni une adresse e-mail.
    pub author: &'static str,
    pub source: &'static str,
    /// PR GitHub validée par le mainteneur, obligatoire pour un module communautaire.
    pub review_pr: Option<u32>,
    pub summary: &'static str,
    pub capabilities: &'static [&'static str],
    pub pages: &'static [ModulePage],
    pub actions: &'static [&'static str],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ModuleStatus {
    Ready,
    /// Audio USB disponible, commandes du mixeur indisponibles.
    Audio,
    Disconnected,
    /// Récepteur ou câble présent, appareil endormi : il revient seul dès qu'on l'utilise.
    Asleep,
    Starting,
    Error,
    Blocked,
}

/// Etat d'une instance pilotee par un module. Les donnees de configuration restent propres au module.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ModuleRuntime {
    pub id: &'static str,
    pub status: ModuleStatus,
    pub connected: bool,
    pub simulated: bool,
    pub firmware: Option<String>,
    /// Sorte de firmware, quand le module sait la reconnaître ; absente sinon.
    pub firmware_kind: Option<FirmwareKind>,
    pub error: Option<String>,
}

/// Firmware qui tourne sur l'appareil.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum FirmwareKind {
    /// Firmware du fabricant.
    Official,
    /// Firmware de secours de la GoXLR : il démarre quand le firmware principal ne se lance plus (retour arrière).
    Golden,
    /// Firmware de laboratoire Rustudio, en essai.
    Lab,
    /// Firmware Rustudio avec micro et sortie USB Audio.
    Audio,
}

pub const GOXLR_MINI: ModuleDescriptor = ModuleDescriptor {
    id: "goxlr-mini",
    name: "GoXLR Mini",
    vendor: "TC-Helicon",
    kind: "audio-interface",
    origin: ModuleOrigin::Official,
    author: "samirmedani",
    source: "crates/goxlr",
    review_pr: None,
    summary: "Mixeur, micro, routage, éclairage et scènes avec le pilote officiel GoXLR.",
    capabilities: &["mixer", "microphone", "headphone-eq", "routing", "lighting", "scenes"],
    pages: &[
        ModulePage { id: "mixer", label: "Mixeur", icon: "mixer" },
        ModulePage { id: "mic", label: "Micro", icon: "Mic" },
        ModulePage { id: "hp", label: "Casque", icon: "Headphones" },
        ModulePage { id: "routing", label: "Routage", icon: "routing" },
        ModulePage { id: "light", label: "Éclairage", icon: "light" },
        ModulePage { id: "scenes", label: "Scènes", icon: "scenes" },
    ],
    actions: &[
        "volume", "mute", "fader", "route", "colour", "mic", "raw", "profile", "dsp", "mic-preset", "scene",
        "scene-save", "scene-delete", "theme", "lighting", "detect-mic", "auto-gain", "voice-setup", "mic-listen",
        "job-cancel", "settings", "auto-scenes", "reconnect", "fx", "hp-eq", "hp-eq-save", "hp-eq-delete",
        "hp-eq-setup", "hp-eq-remove", "echo-check", "sound-test", "mic-model", "voice-print", "mic-ab", "voice-forget", "hp-default", "hp-eq-uninstall", "mic-ns", "mic-ns-setup", "mic-ns-remove", "mic-default",
    ],
};

pub const R5_ULTRA: ModuleDescriptor = ModuleDescriptor {
    id: "attack-shark-r5-ultra", name: "R5 Ultra", vendor: "Attack Shark", kind: "mouse",
    origin: ModuleOrigin::Official, author: "samirmedani", source: "crates/attack-shark", review_pr: None,
    summary: "DPI par palier, fréquence, batterie et réglages du capteur, en USB ou 2,4 GHz.",
    capabilities: &["dpi", "polling", "battery", "sensor"],
    pages: &[ModulePage { id: "mouse", label: "Ma souris", icon: "mouse" }],
    actions: &["setting", "settings", "refresh"],
};

pub const MK770: ModuleDescriptor = ModuleDescriptor {
    id: "cooler-master-mk770", name: "MK770", vendor: "Cooler Master", kind: "keyboard",
    origin: ModuleOrigin::Official, author: "samirmedani", source: "crates/cooler-master", review_pr: None,
    summary: "Effets lumineux, couleur par touche et macros embarquées, en USB.",
    capabilities: &["lighting-effects", "per-key", "macros"],
    pages: &[
        ModulePage { id: "keyboard", label: "Mon clavier", icon: "keyboard" },
        ModulePage { id: "keyboard-macros", label: "Macros", icon: "keyboard" },
    ],
    actions: &["settings", "refresh", "macros", "macro-assign", "macro-remove"],
};

pub const LOGITECH_C920: ModuleDescriptor = ModuleDescriptor {
    id: "logitech-c920", name: "C920", vendor: "Logitech", kind: "webcam",
    origin: ModuleOrigin::Official, author: "samirmedani", source: "crates/logitech-c920", review_pr: None,
    summary: "Image, exposition manuelle, focus, cadrage, préréglages et maintien anti-réinitialisation en USB.",
    capabilities: &["image", "optics", "presets", "lock", "led"],
    pages: &[
        ModulePage { id: "webcam", label: "Ma webcam", icon: "camera" },
    ],
    actions: &["settings", "preset", "reset", "lock", "refresh"],
};

pub const WINDOWS_MIC: ModuleDescriptor = ModuleDescriptor {
    id: "windows-mic", name: "Micro", vendor: "Windows", kind: "microphone",
    origin: ModuleOrigin::Official, author: "samirmedani", source: "crates/windows-mic", review_pr: None,
    summary: "Volume, suppression du bruit, égaliseur, noise gate, compresseur et limiteur pour ton micro Windows.",
    capabilities: &["mic-level", "noise-suppression", "voice-eq", "voice-dynamics"],
    pages: &[ModulePage { id: "windows-mic", label: "Mon micro", icon: "Mic" }],
    actions: &["settings", "setup", "remove", "noise", "attach", "detach", "native-noise"],
};

const INSTALLED: &[ModuleDescriptor] = &[GOXLR_MINI, R5_ULTRA, MK770, LOGITECH_C920, WINDOWS_MIC];

pub fn installed() -> &'static [ModuleDescriptor] {
    INSTALLED
}

pub fn find(id: &str) -> Option<&'static ModuleDescriptor> {
    INSTALLED.iter().find(|module| module.id == id)
}
