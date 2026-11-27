//! Logitech C920 : controle UVC (VideoProcAmp, CameraControl) et Extension Units (XU),
//! validation stricte des bornes, relecture confirmee, detection de derive et simulateur realiste.
//! Aucun acces audio, aucun flash de firmware, aucune ecriture sur les selecteurs XU critiques.
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub mod presets;
pub mod sim;
pub mod uvc;
#[cfg(windows)]
pub mod win;

pub type Result<T> = std::result::Result<T, String>;

pub const VID: u16 = 0x046D;
pub const PID_C920_2019: u16 = 0x0892;
pub const PID_C920_LEGACY: u16 = 0x082D;
pub const PID_C920_PRO: u16 = 0x08E5;
pub const PIDS: [u16; 3] = [PID_C920_2019, PID_C920_LEGACY, PID_C920_PRO];

pub const NO_DEVICE: &str = "Logitech C920 absente : branche ta webcam en USB";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ControlId {
    Brightness,
    Contrast,
    Saturation,
    Sharpness,
    Gain,
    WhiteBalance,
    BacklightComp,
    PowerLineFreq,
    Exposure,
    LowLightComp,
    Focus,
    Zoom,
    Pan,
    Tilt,
}

impl ControlId {
    pub const ALL: [ControlId; 14] = [
        ControlId::Brightness,
        ControlId::Contrast,
        ControlId::Saturation,
        ControlId::Sharpness,
        ControlId::Gain,
        ControlId::WhiteBalance,
        ControlId::BacklightComp,
        ControlId::PowerLineFreq,
        ControlId::Exposure,
        ControlId::LowLightComp,
        ControlId::Focus,
        ControlId::Zoom,
        ControlId::Pan,
        ControlId::Tilt,
    ];

    pub const fn label(self) -> &'static str {
        match self {
            ControlId::Brightness => "Luminosité",
            ControlId::Contrast => "Contraste",
            ControlId::Saturation => "Saturation",
            ControlId::Sharpness => "Netteté",
            ControlId::Gain => "Gain",
            ControlId::WhiteBalance => "Balance des blancs",
            ControlId::BacklightComp => "Compensation contre-jour",
            ControlId::PowerLineFreq => "Anti-scintillement",
            ControlId::Exposure => "Exposition",
            ControlId::LowLightComp => "Compensation basse lumière",
            ControlId::Focus => "Mise au point",
            ControlId::Zoom => "Zoom",
            ControlId::Pan => "Panoramique",
            ControlId::Tilt => "Inclinaison",
        }
    }
}

/// Controles XU Logitech autorises. Seul `Led1Mode` est inscriptible (CS=9, Read-Modify-Write 5 octets).
/// Les selecteurs firmware/reboot ne sont jamais exposes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum XuControl {
    Led1Mode,
    FirmwareVersion,
    DeviceIdentity,
    DspPlatform,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PowerLineFreq {
    Off,
    Hz50,
    Hz60,
}

impl PowerLineFreq {
    pub const fn to_raw(self) -> i32 {
        match self {
            PowerLineFreq::Off => 0,
            PowerLineFreq::Hz50 => 1,
            PowerLineFreq::Hz60 => 2,
        }
    }

    pub fn from_raw(raw: i32) -> Result<Self> {
        match raw {
            0 => Ok(PowerLineFreq::Off),
            1 => Ok(PowerLineFreq::Hz50),
            2 => Ok(PowerLineFreq::Hz60),
            other => Err(format!("Fréquence anti-scintillement inconnue : {other}")),
        }
    }

    pub fn parse_json(v: &Value) -> Result<Self> {
        if let Some(s) = v.as_str() {
            return match s {
                "off" | "0" => Ok(PowerLineFreq::Off),
                "hz50" | "50" | "50hz" => Ok(PowerLineFreq::Hz50),
                "hz60" | "60" | "60hz" => Ok(PowerLineFreq::Hz60),
                _ => Err("Anti-scintillement attendu : off, hz50 ou hz60".into()),
            };
        }
        if let Some(n) = v.as_i64() {
            return match n {
                0 => Ok(PowerLineFreq::Off),
                1 | 50 => Ok(PowerLineFreq::Hz50),
                2 | 60 => Ok(PowerLineFreq::Hz60),
                _ => Err("Anti-scintillement attendu : 0, 50 ou 60 Hz".into()),
            };
        }
        Err("Anti-scintillement invalide".into())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LedMode {
    Off,
    On,
    Blink,
    Auto,
}

impl LedMode {
    pub const fn label(self) -> &'static str {
        match self {
            LedMode::Off => "Éteinte",
            LedMode::On => "Allumée",
            LedMode::Blink => "Clignotante",
            LedMode::Auto => "Automatique",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ControlRange {
    pub min: i32,
    pub max: i32,
    pub step: i32,
    pub default: i32,
    pub supports_auto: bool,
    pub supports_manual: bool,
}

impl ControlRange {
    pub const fn new(min: i32, max: i32, step: i32, default: i32, supports_auto: bool, supports_manual: bool) -> Self {
        Self { min, max, step, default, supports_auto, supports_manual }
    }

    /// Recale une valeur sur [min, max] et sur le pas le plus proche (utile pour les prereglages).
    pub fn snap(&self, value: i32) -> i32 {
        let clamped = value.clamp(self.min, self.max);
        let step = self.step.max(1);
        let offset = clamped - self.min;
        let snapped = self.min + ((offset + step / 2) / step) * step;
        snapped.clamp(self.min, self.max)
    }

    /// Valide strictement une valeur utilisateur.
    pub fn validate(&self, label: &str, value: i32) -> Result<i32> {
        if value < self.min || value > self.max {
            return Err(format!("{label} hors limites ({} à {})", self.min, self.max));
        }
        let step = self.step.max(1);
        if (value - self.min) % step != 0 {
            return Err(format!("{label} doit suivre un pas de {step} (entre {} et {})", self.min, self.max));
        }
        Ok(value)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ControlValue {
    pub value: i32,
    pub auto: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct VideoFormat {
    pub codec: String,
    pub width: u32,
    pub height: u32,
    pub max_fps: u32,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct XuUnitDesc {
    pub node_id: u32,
    pub guid: String,
    pub name: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct XuInfo {
    pub led_supported: bool,
    pub units: Vec<XuUnitDesc>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeviceInfo {
    pub name: String,
    pub vid: u16,
    pub pid: u16,
    pub revision: String,
    pub firmware: Option<String>,
    pub dsp_platform: Option<String>,
    pub revision_label: String,
    pub formats: Vec<VideoFormat>,
    pub xu: XuInfo,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Ranges {
    pub brightness: ControlRange,
    pub contrast: ControlRange,
    pub saturation: ControlRange,
    pub sharpness: ControlRange,
    pub gain: ControlRange,
    pub white_balance: ControlRange,
    pub backlight_comp: ControlRange,
    pub power_line: ControlRange,
    pub exposure: ControlRange,
    pub low_light_comp: ControlRange,
    pub focus: ControlRange,
    pub zoom: ControlRange,
    pub pan: ControlRange,
    pub tilt: ControlRange,
}

impl Default for Ranges {
    /// Bornes exactes relevees sur la Logitech C920 (046D:0892, FW 7.59.1035), affinees a la lecture reelle.
    fn default() -> Self {
        Self {
            brightness: ControlRange::new(0, 255, 1, 128, false, true),
            contrast: ControlRange::new(0, 255, 1, 128, false, true),
            saturation: ControlRange::new(0, 255, 1, 128, false, true),
            sharpness: ControlRange::new(0, 255, 1, 128, false, true),
            gain: ControlRange::new(0, 255, 1, 0, false, true),
            white_balance: ControlRange::new(2000, 6500, 1, 4000, true, true),
            backlight_comp: ControlRange::new(0, 1, 1, 0, false, true),
            // Sur 046D:0892 : 1 = 50 Hz, 2 = 60 Hz (defaut 2) ; 0 non supporte.
            power_line: ControlRange::new(1, 2, 1, 2, false, true),
            exposure: ControlRange::new(-11, -2, 1, -5, true, true),
            // AutoExposurePriority (prop 19) : 0 = OFF (30 fps constant), 1 = ON (defaut C920).
            low_light_comp: ControlRange::new(0, 1, 1, 1, false, true),
            focus: ControlRange::new(0, 250, 5, 0, true, true),
            zoom: ControlRange::new(100, 500, 1, 100, false, true),
            pan: ControlRange::new(-10, 10, 1, 0, false, true),
            tilt: ControlRange::new(-10, 10, 1, 0, false, true),
        }
    }
}

impl Ranges {
    pub fn get(&self, id: ControlId) -> ControlRange {
        match id {
            ControlId::Brightness => self.brightness,
            ControlId::Contrast => self.contrast,
            ControlId::Saturation => self.saturation,
            ControlId::Sharpness => self.sharpness,
            ControlId::Gain => self.gain,
            ControlId::WhiteBalance => self.white_balance,
            ControlId::BacklightComp => self.backlight_comp,
            ControlId::PowerLineFreq => self.power_line,
            ControlId::Exposure => self.exposure,
            ControlId::LowLightComp => self.low_light_comp,
            ControlId::Focus => self.focus,
            ControlId::Zoom => self.zoom,
            ControlId::Pan => self.pan,
            ControlId::Tilt => self.tilt,
        }
    }

    pub fn set(&mut self, id: ControlId, range: ControlRange) {
        match id {
            ControlId::Brightness => self.brightness = range,
            ControlId::Contrast => self.contrast = range,
            ControlId::Saturation => self.saturation = range,
            ControlId::Sharpness => self.sharpness = range,
            ControlId::Gain => self.gain = range,
            ControlId::WhiteBalance => self.white_balance = range,
            ControlId::BacklightComp => self.backlight_comp = range,
            ControlId::PowerLineFreq => self.power_line = range,
            ControlId::Exposure => self.exposure = range,
            ControlId::LowLightComp => self.low_light_comp = range,
            ControlId::Focus => self.focus = range,
            ControlId::Zoom => self.zoom = range,
            ControlId::Pan => self.pan = range,
            ControlId::Tilt => self.tilt = range,
        }
    }
}

/// Etat complet relu sur la webcam.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Settings {
    pub device: DeviceInfo,
    pub brightness: i32,
    pub contrast: i32,
    pub saturation: i32,
    pub sharpness: i32,
    pub gain: i32,
    pub white_balance_auto: bool,
    pub white_balance: i32,
    pub backlight_comp: bool,
    pub power_line: PowerLineFreq,
    pub exposure_auto: bool,
    pub exposure: i32,
    pub low_light_comp: bool,
    pub focus_auto: bool,
    pub focus: i32,
    pub zoom: i32,
    pub pan: i32,
    pub tilt: i32,
    pub led_mode: Option<LedMode>,
    pub ranges: Ranges,
}

/// Valeurs cibles retenues pour le maintien automatique (« Garder mes réglages »).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DesiredSettings {
    pub brightness: i32,
    pub contrast: i32,
    pub saturation: i32,
    pub sharpness: i32,
    pub gain: i32,
    pub white_balance_auto: bool,
    pub white_balance: i32,
    pub backlight_comp: bool,
    pub power_line: PowerLineFreq,
    pub exposure_auto: bool,
    pub exposure: i32,
    pub low_light_comp: bool,
    pub focus_auto: bool,
    pub focus: i32,
    pub zoom: i32,
    pub pan: i32,
    pub tilt: i32,
    #[serde(default)]
    pub led_mode: Option<LedMode>,
}

impl From<&Settings> for DesiredSettings {
    fn from(s: &Settings) -> Self {
        Self {
            brightness: s.brightness,
            contrast: s.contrast,
            saturation: s.saturation,
            sharpness: s.sharpness,
            gain: s.gain,
            white_balance_auto: s.white_balance_auto,
            white_balance: s.white_balance,
            backlight_comp: s.backlight_comp,
            power_line: s.power_line,
            exposure_auto: s.exposure_auto,
            exposure: s.exposure,
            low_light_comp: s.low_light_comp,
            focus_auto: s.focus_auto,
            focus: s.focus,
            zoom: s.zoom,
            pan: s.pan,
            tilt: s.tilt,
            led_mode: s.led_mode,
        }
    }
}

impl DesiredSettings {
    /// Calcule les seuls controles ayant derive entre l'etat courant et les reglages souhaites.
    /// Les valeurs manuelles (balance, exposition, focus) ne sont comparees que lorsque leur mode auto est coupe.
    pub fn diff_from(&self, current: &Settings) -> (ControlPatch, Vec<&'static str>) {
        let mut p = ControlPatch::default();
        let mut names = Vec::new();
        let r = &current.ranges;

        let target_brightness = r.brightness.snap(self.brightness);
        if current.brightness != target_brightness {
            p.brightness = Some(target_brightness);
            names.push("brightness");
        }
        let target_contrast = r.contrast.snap(self.contrast);
        if current.contrast != target_contrast {
            p.contrast = Some(target_contrast);
            names.push("contrast");
        }
        let target_saturation = r.saturation.snap(self.saturation);
        if current.saturation != target_saturation {
            p.saturation = Some(target_saturation);
            names.push("saturation");
        }
        let target_sharpness = r.sharpness.snap(self.sharpness);
        if current.sharpness != target_sharpness {
            p.sharpness = Some(target_sharpness);
            names.push("sharpness");
        }
        let target_gain = r.gain.snap(self.gain);
        if current.gain != target_gain {
            p.gain = Some(target_gain);
            names.push("gain");
        }
        if current.white_balance_auto != self.white_balance_auto {
            p.white_balance_auto = Some(self.white_balance_auto);
            names.push("white_balance_auto");
        }
        if !self.white_balance_auto {
            let target_wb = r.white_balance.snap(self.white_balance);
            if current.white_balance != target_wb || current.white_balance_auto {
                p.white_balance = Some(target_wb);
                names.push("white_balance");
            }
        }
        if current.backlight_comp != self.backlight_comp {
            p.backlight_comp = Some(self.backlight_comp);
            names.push("backlight_comp");
        }
        let target_pl = PowerLineFreq::from_raw(r.power_line.snap(self.power_line.to_raw())).unwrap_or(self.power_line);
        if current.power_line != target_pl {
            p.power_line = Some(target_pl);
            names.push("power_line");
        }
        if current.exposure_auto != self.exposure_auto {
            p.exposure_auto = Some(self.exposure_auto);
            names.push("exposure_auto");
        }
        if !self.exposure_auto {
            let target_exp = r.exposure.snap(self.exposure);
            if current.exposure != target_exp || current.exposure_auto {
                p.exposure = Some(target_exp);
                names.push("exposure");
            }
        }
        if current.low_light_comp != self.low_light_comp {
            p.low_light_comp = Some(self.low_light_comp);
            names.push("low_light_comp");
        }
        if current.focus_auto != self.focus_auto {
            p.focus_auto = Some(self.focus_auto);
            names.push("focus_auto");
        }
        if !self.focus_auto {
            let target_focus = r.focus.snap(self.focus);
            if current.focus != target_focus || current.focus_auto {
                p.focus = Some(target_focus);
                names.push("focus");
            }
        }
        let target_zoom = r.zoom.snap(self.zoom);
        if current.zoom != target_zoom {
            p.zoom = Some(target_zoom);
            names.push("zoom");
        }
        let target_pan = r.pan.snap(self.pan);
        if current.pan != target_pan {
            p.pan = Some(target_pan);
            names.push("pan");
        }
        let target_tilt = r.tilt.snap(self.tilt);
        if current.tilt != target_tilt {
            p.tilt = Some(target_tilt);
            names.push("tilt");
        }
        if let (Some(want), Some(cur)) = (self.led_mode, current.led_mode) {
            if want != cur {
                p.led_mode = Some(want);
                names.push("led_mode");
            }
        }
        (p, names)
    }
}

/// Ensemble partiel de modifications a appliquer a la webcam.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ControlPatch {
    pub brightness: Option<i32>,
    pub contrast: Option<i32>,
    pub saturation: Option<i32>,
    pub sharpness: Option<i32>,
    pub gain: Option<i32>,
    pub white_balance_auto: Option<bool>,
    pub white_balance: Option<i32>,
    pub backlight_comp: Option<bool>,
    pub power_line: Option<PowerLineFreq>,
    pub exposure_auto: Option<bool>,
    pub exposure: Option<i32>,
    pub low_light_comp: Option<bool>,
    pub focus_auto: Option<bool>,
    pub focus: Option<i32>,
    pub zoom: Option<i32>,
    pub pan: Option<i32>,
    pub tilt: Option<i32>,
    pub led_mode: Option<LedMode>,
}

impl ControlPatch {
    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }

    /// Construit un patch depuis un objet JSON recu par l'API (validation stricte des cles et types).
    pub fn from_json(body: &Value) -> Result<Self> {
        let obj = body.as_object().ok_or("Objet JSON de réglages attendu")?;
        if obj.is_empty() {
            return Err("Aucun réglage à appliquer".into());
        }
        let mut p = Self::default();
        let need_i32 = |v: &Value, name: &str| -> Result<i32> {
            v.as_i64()
                .and_then(|n| i32::try_from(n).ok())
                .ok_or_else(|| format!("Valeur entière attendue pour {name}"))
        };
        let need_bool = |v: &Value, name: &str| -> Result<bool> {
            v.as_bool()
                .ok_or_else(|| format!("Booléen attendu pour {name}"))
        };
        for (k, v) in obj {
            match k.as_str() {
                "brightness" => p.brightness = Some(need_i32(v, "luminosité")?),
                "contrast" => p.contrast = Some(need_i32(v, "contraste")?),
                "saturation" => p.saturation = Some(need_i32(v, "saturation")?),
                "sharpness" => p.sharpness = Some(need_i32(v, "netteté")?),
                "gain" => p.gain = Some(need_i32(v, "gain")?),
                "white_balance_auto" => p.white_balance_auto = Some(need_bool(v, "balance auto")?),
                "white_balance" => p.white_balance = Some(need_i32(v, "balance des blancs")?),
                "backlight_comp" => p.backlight_comp = Some(need_bool(v, "compensation contre-jour")?),
                "power_line" => p.power_line = Some(PowerLineFreq::parse_json(v)?),
                "exposure_auto" => p.exposure_auto = Some(need_bool(v, "exposition auto")?),
                "exposure" => p.exposure = Some(need_i32(v, "exposition")?),
                "low_light_comp" => p.low_light_comp = Some(need_bool(v, "compensation basse lumière")?),
                "focus_auto" => p.focus_auto = Some(need_bool(v, "mise au point auto")?),
                "focus" => p.focus = Some(need_i32(v, "mise au point")?),
                "zoom" => p.zoom = Some(need_i32(v, "zoom")?),
                "pan" => p.pan = Some(need_i32(v, "panoramique")?),
                "tilt" => p.tilt = Some(need_i32(v, "inclinaison")?),
                "led_mode" => {
                    p.led_mode = Some(presets::parse_led_mode(v).ok_or("Mode LED attendu : off, on, blink ou auto")?);
                }
                other => return Err(format!("Réglage webcam inconnu : {other}")),
            }
        }
        Ok(p)
    }

    /// Recale les valeurs d'un prereglage sur les bornes reelles de la webcam.
    pub fn snapped_to(&self, r: &Ranges) -> Self {
        Self {
            brightness: self.brightness.map(|v| r.brightness.snap(v)),
            contrast: self.contrast.map(|v| r.contrast.snap(v)),
            saturation: self.saturation.map(|v| r.saturation.snap(v)),
            sharpness: self.sharpness.map(|v| r.sharpness.snap(v)),
            gain: self.gain.map(|v| r.gain.snap(v)),
            white_balance_auto: self.white_balance_auto,
            white_balance: self.white_balance.map(|v| r.white_balance.snap(v)),
            backlight_comp: self.backlight_comp,
            power_line: self.power_line.and_then(|pl| PowerLineFreq::from_raw(r.power_line.snap(pl.to_raw())).ok()),
            exposure_auto: self.exposure_auto,
            exposure: self.exposure.map(|v| r.exposure.snap(v)),
            low_light_comp: self.low_light_comp,
            focus_auto: self.focus_auto,
            focus: self.focus.map(|v| r.focus.snap(v)),
            zoom: self.zoom.map(|v| r.zoom.snap(v)),
            pan: self.pan.map(|v| r.pan.snap(v)),
            tilt: self.tilt.map(|v| r.tilt.snap(v)),
            led_mode: self.led_mode,
        }
    }

    /// Supprime un champ du patch (utilise pour la mise en pause d'un champ en echec repete).
    pub fn clear_field(&mut self, name: &str) {
        match name {
            "brightness" => self.brightness = None,
            "contrast" => self.contrast = None,
            "saturation" => self.saturation = None,
            "sharpness" => self.sharpness = None,
            "gain" => self.gain = None,
            "white_balance_auto" => self.white_balance_auto = None,
            "white_balance" => self.white_balance = None,
            "backlight_comp" => self.backlight_comp = None,
            "power_line" => self.power_line = None,
            "exposure_auto" => self.exposure_auto = None,
            "exposure" => self.exposure = None,
            "low_light_comp" => self.low_light_comp = None,
            "focus_auto" => self.focus_auto = None,
            "focus" => self.focus = None,
            "zoom" => self.zoom = None,
            "pan" => self.pan = None,
            "tilt" => self.tilt = None,
            "led_mode" => self.led_mode = None,
            _ => {}
        }
    }
}

/// Transport materiel ou simule pour parler a une Logitech C920.
pub trait Transport: Send {
    fn info(&mut self) -> Result<DeviceInfo>;
    fn range(&mut self, control: ControlId) -> Result<Option<ControlRange>>;
    fn get(&mut self, control: ControlId) -> Result<ControlValue>;
    fn set(&mut self, control: ControlId, value: ControlValue) -> Result<()>;
    fn get_xu(&mut self, control: XuControl) -> Result<Option<Vec<u8>>>;
    fn set_xu(&mut self, control: XuControl, data: &[u8]) -> Result<()>;
}

impl<T: Transport> Transport for &mut T {
    fn info(&mut self) -> Result<DeviceInfo> { (**self).info() }
    fn range(&mut self, control: ControlId) -> Result<Option<ControlRange>> { (**self).range(control) }
    fn get(&mut self, control: ControlId) -> Result<ControlValue> { (**self).get(control) }
    fn set(&mut self, control: ControlId, value: ControlValue) -> Result<()> { (**self).set(control, value) }
    fn get_xu(&mut self, control: XuControl) -> Result<Option<Vec<u8>>> { (**self).get_xu(control) }
    fn set_xu(&mut self, control: XuControl, data: &[u8]) -> Result<()> { (**self).set_xu(control, data) }
}

impl<T: Transport + ?Sized> Transport for Box<T> {
    fn info(&mut self) -> Result<DeviceInfo> { (**self).info() }
    fn range(&mut self, control: ControlId) -> Result<Option<ControlRange>> { (**self).range(control) }
    fn get(&mut self, control: ControlId) -> Result<ControlValue> { (**self).get(control) }
    fn set(&mut self, control: ControlId, value: ControlValue) -> Result<()> { (**self).set(control, value) }
    fn get_xu(&mut self, control: XuControl) -> Result<Option<Vec<u8>>> { (**self).get_xu(control) }
    fn set_xu(&mut self, control: XuControl, data: &[u8]) -> Result<()> { (**self).set_xu(control, data) }
}

/// Pilote haut niveau d'une Logitech C920, au-dessus d'un `Transport`.
pub struct Camera<T: Transport> {
    transport: T,
    ranges: Option<Ranges>,
}

impl<T: Transport> Camera<T> {
    pub fn new(transport: T) -> Self {
        Self { transport, ranges: None }
    }

    pub fn into_inner(self) -> T {
        self.transport
    }

    pub fn transport_mut(&mut self) -> &mut T {
        &mut self.transport
    }

    fn load_ranges(&mut self) -> Result<Ranges> {
        if let Some(r) = &self.ranges {
            return Ok(r.clone());
        }
        let mut ranges = Ranges::default();
        for control in ControlId::ALL {
            if let Some(r) = self.transport.range(control)? {
                // Sous usbvideo.sys, GetRange(19) (AutoExposurePriority) renvoie 0..0 alors que 0 et 1 fonctionnent.
                if control == ControlId::LowLightComp && r.min == 0 && r.max == 0 {
                    continue;
                }
                ranges.set(control, r);
            }
        }
        self.ranges = Some(ranges.clone());
        Ok(ranges)
    }

    /// Lit tous les controles UVC et XU de la webcam.
    pub fn read(&mut self) -> Result<Settings> {
        let mut device = self.transport.info()?;
        let ranges = self.load_ranges()?;

        // Enrichit les metadonnees en lecture seule depuis les XU Logitech (firmware, identite, plateforme DSP).
        if let Ok(Some(fw_bytes)) = self.transport.get_xu(XuControl::FirmwareVersion) {
            if let Ok(fw) = uvc::decode_firmware_version(&fw_bytes) {
                device.firmware = Some(fw);
            }
        }
        if let Ok(Some(id_bytes)) = self.transport.get_xu(XuControl::DeviceIdentity) {
            if let Ok((vid, pid, bcd)) = uvc::decode_device_identity(&id_bytes) {
                if vid != 0 && pid != 0 {
                    device.vid = vid;
                    device.pid = pid;
                    device.revision = format!("{bcd:04X}");
                    device.revision_label = uvc::revision_label(pid);
                }
            }
        }
        if let Ok(Some(dsp_bytes)) = self.transport.get_xu(XuControl::DspPlatform) {
            if let Some(platform) = uvc::decode_dsp_platform(&dsp_bytes) {
                device.dsp_platform = Some(platform);
            }
        }

        let brightness = self.transport.get(ControlId::Brightness)?.value;
        let contrast = self.transport.get(ControlId::Contrast)?.value;
        let saturation = self.transport.get(ControlId::Saturation)?.value;
        let sharpness = self.transport.get(ControlId::Sharpness)?.value;
        let gain = self.transport.get(ControlId::Gain)?.value;
        let wb = self.transport.get(ControlId::WhiteBalance)?;
        let backlight_comp = self.transport.get(ControlId::BacklightComp)?.value != 0;
        let power_line = PowerLineFreq::from_raw(self.transport.get(ControlId::PowerLineFreq)?.value)?;

        let exp = self.transport.get(ControlId::Exposure)?;
        let low_light_comp = self.transport.get(ControlId::LowLightComp)?.value != 0;
        let focus = self.transport.get(ControlId::Focus)?;
        let zoom = self.transport.get(ControlId::Zoom)?.value;
        let pan = self.transport.get(ControlId::Pan)?.value;
        let tilt = self.transport.get(ControlId::Tilt)?.value;

        let led_mode = match self.transport.get_xu(XuControl::Led1Mode)? {
            Some(bytes) => Some(uvc::decode_led1(&bytes)?.0),
            None => None,
        };

        Ok(Settings {
            device,
            brightness,
            contrast,
            saturation,
            sharpness,
            gain,
            white_balance_auto: wb.auto,
            white_balance: wb.value,
            backlight_comp,
            power_line,
            exposure_auto: exp.auto,
            exposure: exp.value,
            low_light_comp,
            focus_auto: focus.auto,
            focus: focus.value,
            zoom,
            pan,
            tilt,
            led_mode,
            ranges,
        })
    }

    /// Valide puis applique un patch JSON (avec relecture complete pour confirmation).
    pub fn apply_json(&mut self, body: &Value) -> Result<Settings> {
        let patch = ControlPatch::from_json(body)?;
        self.apply_patch(&patch, true)
    }

    /// Applique un patch valide (si `strict`, rejette hors bornes ou mauvais pas ; sinon recale).
    pub fn apply_patch(&mut self, patch: &ControlPatch, strict: bool) -> Result<Settings> {
        if patch.is_empty() {
            return self.read();
        }
        let current = self.read()?;
        let ranges = &current.ranges;

        // 1. Validation complete avant toute ecriture materielle.
        let check = |id: ControlId, v: i32| -> Result<i32> {
            let r = ranges.get(id);
            if strict { r.validate(id.label(), v) } else { Ok(r.snap(v)) }
        };

        let mut ops: Vec<(ControlId, ControlValue)> = Vec::new();
        if let Some(v) = patch.brightness {
            ops.push((ControlId::Brightness, ControlValue { value: check(ControlId::Brightness, v)?, auto: false }));
        }
        if let Some(v) = patch.contrast {
            ops.push((ControlId::Contrast, ControlValue { value: check(ControlId::Contrast, v)?, auto: false }));
        }
        if let Some(v) = patch.saturation {
            ops.push((ControlId::Saturation, ControlValue { value: check(ControlId::Saturation, v)?, auto: false }));
        }
        if let Some(v) = patch.sharpness {
            ops.push((ControlId::Sharpness, ControlValue { value: check(ControlId::Sharpness, v)?, auto: false }));
        }
        if let Some(v) = patch.gain {
            ops.push((ControlId::Gain, ControlValue { value: check(ControlId::Gain, v)?, auto: false }));
        }
        if let Some(on) = patch.backlight_comp {
            ops.push((ControlId::BacklightComp, ControlValue { value: i32::from(on), auto: false }));
        }
        if let Some(pl) = patch.power_line {
            let raw = check(ControlId::PowerLineFreq, pl.to_raw())?;
            ops.push((ControlId::PowerLineFreq, ControlValue { value: raw, auto: false }));
        }

        // Balance des blancs : si une valeur manuelle est demandee, on passe en manuel (sauf auto: true explicite).
        match (patch.white_balance_auto, patch.white_balance) {
            (Some(true), _) => {
                ops.push((ControlId::WhiteBalance, ControlValue { value: current.white_balance, auto: true }));
            }
            (Some(false), Some(v)) => {
                ops.push((ControlId::WhiteBalance, ControlValue { value: check(ControlId::WhiteBalance, v)?, auto: false }));
            }
            (Some(false), None) => {
                ops.push((ControlId::WhiteBalance, ControlValue { value: current.white_balance, auto: false }));
            }
            (None, Some(v)) => {
                ops.push((ControlId::WhiteBalance, ControlValue { value: check(ControlId::WhiteBalance, v)?, auto: false }));
            }
            (None, None) => {}
        }

        // Low-Light Compensation (AutoExposurePriority) appliquee avant l'exposition pour garantir 30 ips en manuel.
        if let Some(on) = patch.low_light_comp {
            ops.push((ControlId::LowLightComp, ControlValue { value: i32::from(on), auto: false }));
        }

        // Exposition : meme regle auto/manuel.
        match (patch.exposure_auto, patch.exposure) {
            (Some(true), _) => {
                ops.push((ControlId::Exposure, ControlValue { value: current.exposure, auto: true }));
            }
            (Some(false), Some(v)) => {
                ops.push((ControlId::Exposure, ControlValue { value: check(ControlId::Exposure, v)?, auto: false }));
            }
            (Some(false), None) => {
                ops.push((ControlId::Exposure, ControlValue { value: current.exposure, auto: false }));
            }
            (None, Some(v)) => {
                ops.push((ControlId::Exposure, ControlValue { value: check(ControlId::Exposure, v)?, auto: false }));
            }
            (None, None) => {}
        }

        // Mise au point (Focus, pas de 5) : meme regle auto/manuel.
        match (patch.focus_auto, patch.focus) {
            (Some(true), _) => {
                ops.push((ControlId::Focus, ControlValue { value: current.focus, auto: true }));
            }
            (Some(false), Some(v)) => {
                ops.push((ControlId::Focus, ControlValue { value: check(ControlId::Focus, v)?, auto: false }));
            }
            (Some(false), None) => {
                ops.push((ControlId::Focus, ControlValue { value: current.focus, auto: false }));
            }
            (None, Some(v)) => {
                ops.push((ControlId::Focus, ControlValue { value: check(ControlId::Focus, v)?, auto: false }));
            }
            (None, None) => {}
        }

        if let Some(v) = patch.zoom {
            ops.push((ControlId::Zoom, ControlValue { value: check(ControlId::Zoom, v)?, auto: false }));
        }
        if let Some(v) = patch.pan {
            ops.push((ControlId::Pan, ControlValue { value: check(ControlId::Pan, v)?, auto: false }));
        }
        if let Some(v) = patch.tilt {
            ops.push((ControlId::Tilt, ControlValue { value: check(ControlId::Tilt, v)?, auto: false }));
        }

        if patch.led_mode.is_some() && !current.device.xu.led_supported {
            return Err("Le contrôle de la LED n'est pas pris en charge sur cette révision de C920".into());
        }

        // 2. Application des controles UVC valides.
        for (control, val) in ops {
            self.transport.set(control, val)?;
        }

        // 3. Application du controle XU LED1 (CS=9, 5 octets) par Read-Modify-Write strict.
        if let Some(mode) = patch.led_mode {
            let existing = self
                .transport
                .get_xu(XuControl::Led1Mode)?
                .ok_or_else(|| "Unité XU LED1 indisponible".to_owned())?;
            let next = uvc::patch_led1(&existing, mode, None)?;
            self.transport.set_xu(XuControl::Led1Mode, &next)?;
        }

        // 4. Relecture complete : seule la valeur relue fait autorite.
        self.read()
    }

    /// Applique un prereglage integre par identifiant (`studio`, `low-light`, `natural`, `cinema`).
    pub fn apply_preset(&mut self, id: &str) -> Result<Settings> {
        let preset = presets::find(id).ok_or_else(|| format!("Préréglage webcam inconnu : {id}"))?;
        let ranges = self.load_ranges()?;
        let snapped = preset.patch.snapped_to(&ranges);
        self.apply_patch(&snapped, false)
    }

    /// Reinitialise tous les reglages aux valeurs usine annoncees par la webcam.
    pub fn reset_defaults(&mut self) -> Result<Settings> {
        let current = self.read()?;
        let r = &current.ranges;
        let patch = ControlPatch {
            brightness: Some(r.brightness.default),
            contrast: Some(r.contrast.default),
            saturation: Some(r.saturation.default),
            sharpness: Some(r.sharpness.default),
            gain: Some(r.gain.default),
            white_balance_auto: Some(r.white_balance.supports_auto),
            white_balance: Some(r.white_balance.default),
            backlight_comp: Some(r.backlight_comp.default != 0),
            power_line: Some(PowerLineFreq::from_raw(r.power_line.default).unwrap_or(PowerLineFreq::Hz60)),
            exposure_auto: Some(r.exposure.supports_auto),
            exposure: Some(r.exposure.default),
            low_light_comp: Some(r.low_light_comp.default != 0),
            focus_auto: Some(r.focus.supports_auto),
            focus: Some(r.focus.default),
            zoom: Some(r.zoom.default),
            pan: Some(r.pan.default),
            tilt: Some(r.tilt.default),
            led_mode: current.device.xu.led_supported.then_some(LedMode::Auto),
        };
        self.apply_patch(&patch, false)
    }
}
