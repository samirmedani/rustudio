//! Constantes UVC, Kernel Streaming (Windows) et Extension Units (XU) Logitech C920,
//! verifiees sur une vraie HD Pro Webcam C920 (046D:0892, rev 0.19, FW 7.59.1035, SoC AIT PYTHON_V2B).
//!
//! SECURITE MATERIELLE :
//! - Seul le selecteur LED1 (CS=9, 5 octets) sur `XU_LOGITECH_PERIPHERAL` est ecrit (Read-Modify-Write).
//! - Les selecteurs de flash AIT (`XU_LOGITECH_AIT_CUSTOM` CS 1/3/4/6) et de reboot (`XU_LOGITECH_PERIPHERAL` CS 17)
//!   ne sont JAMAIS ecrits ni exposes en ecriture.
use crate::{ControlId, LedMode, Result, XuControl};

/// Representation portable d'un GUID Windows (16 octets au format standard Microsoft).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(C)]
pub struct Guid {
    pub data1: u32,
    pub data2: u16,
    pub data3: u16,
    pub data4: [u8; 8],
}

impl Guid {
    pub const fn new(data1: u32, data2: u16, data3: u16, data4: [u8; 8]) -> Self {
        Self { data1, data2, data3, data4 }
    }

    pub fn format_hyphenated(&self) -> String {
        format!(
            "{:08x}-{:04x}-{:04x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
            self.data1,
            self.data2,
            self.data3,
            self.data4[0],
            self.data4[1],
            self.data4[2],
            self.data4[3],
            self.data4[4],
            self.data4[5],
            self.data4[6],
            self.data4[7],
        )
    }
}

/// Categories d'interfaces Windows pour les webcams UVC (KSCATEGORY_CAPTURE / KSCATEGORY_VIDEO_CAMERA).
pub const KSCATEGORY_CAPTURE: Guid =
    Guid::new(0x65E8773D, 0x8F56, 0x11D0, [0xA3, 0xB9, 0x00, 0xA0, 0xC9, 0x22, 0x31, 0x96]);
pub const KSCATEGORY_VIDEO_CAMERA: Guid =
    Guid::new(0xE5323777, 0xF976, 0x4F5B, [0x9B, 0x55, 0xB9, 0x46, 0x99, 0xC4, 0x6E, 0x44]);

/// Ensembles de proprietes Kernel Streaming UVC standards.
pub const PROPSETID_VIDCAP_VIDEOPROCAMP: Guid =
    Guid::new(0xC6E13360, 0x30AC, 0x11D0, [0xA1, 0x8C, 0x00, 0xA0, 0xC9, 0x11, 0x89, 0x56]);
pub const PROPSETID_VIDCAP_CAMERACONTROL: Guid =
    Guid::new(0xC6E13370, 0x30AC, 0x11D0, [0xA1, 0x8C, 0x00, 0xA0, 0xC9, 0x11, 0x89, 0x56]);
pub const KSPROPSETID_TOPOLOGY: Guid =
    Guid::new(0x720D4AC0, 0x7533, 0x11D0, [0xA5, 0xD6, 0x28, 0xDB, 0x04, 0xC1, 0x00, 0x00]);
pub const KSNODETYPE_DEV_SPECIFIC: Guid =
    Guid::new(0x941C7AC0, 0xC559, 0x11D0, [0x8A, 0x2B, 0x00, 0xA0, 0xC9, 0x25, 0x5A, 0xC1]);

/// Extension Units Logitech verifiees sur la vraie C920 (046D:0892) :
/// Peripheral Control (bUnitID=11) : CS=9 (5 octets RW) pour LED1. Ne JAMAIS ecrire CS=17 (reboot).
pub const XU_LOGITECH_PERIPHERAL: Guid =
    Guid::new(0xFFE52D21, 0x8030, 0x4E2C, [0x82, 0xD9, 0xF5, 0x87, 0xD0, 0x05, 0x40, 0xBD]);
/// Device Information (bUnitID=8, lecture seule) : CS=1 (4 octets FW), CS=6 (6 octets VID/PID/bcdDevice).
pub const XU_LOGITECH_DEV_INFO: Guid =
    Guid::new(0x69678EE4, 0x410F, 0x40DB, [0xA8, 0x50, 0x74, 0x20, 0xD7, 0xD8, 0x24, 0x0E]);
/// AIT Custom (bUnitID=6) : LECTURE SEULE CS=10 (16 octets plateforme DSP "PYTHON_V2B").
/// INTERDICTION ABSOLUE d'ecrire CS=1, 3, 4 ou 6 (flash firmware AIT).
pub const XU_LOGITECH_AIT_CUSTOM: Guid =
    Guid::new(0x23E49ED0, 0x1178, 0x4F31, [0xAE, 0x52, 0xD2, 0xFB, 0x8A, 0x8D, 0x3B, 0x48]);

/// Flags Kernel Streaming VideoProcAmp / CameraControl.
pub const KS_FLAG_NONE: u32 = 0x0000;
pub const KS_FLAG_AUTO: u32 = 0x0001;
pub const KS_FLAG_MANUAL: u32 = 0x0002;

/// Identifiants numeriques des proprietes VideoProcAmp (`IAMVideoProcAmp`, bUnitID=3).
pub const VPA_BRIGHTNESS: u32 = 0;
pub const VPA_CONTRAST: u32 = 1;
pub const VPA_SATURATION: u32 = 3;
pub const VPA_SHARPNESS: u32 = 4;
pub const VPA_WHITEBALANCE: u32 = 7;
pub const VPA_BACKLIGHT_COMPENSATION: u32 = 8;
pub const VPA_GAIN: u32 = 9;
pub const VPA_POWERLINE_FREQUENCY: u32 = 13;

/// Identifiants numeriques des proprietes CameraControl (`IAMCameraControl`, bTerminalID=1).
pub const CC_PAN: u32 = 0;
pub const CC_TILT: u32 = 1;
pub const CC_ZOOM: u32 = 3;
pub const CC_EXPOSURE: u32 = 4;
pub const CC_FOCUS: u32 = 6;
pub const CC_AUTO_EXPOSURE_PRIORITY: u32 = 19;

/// Selecteurs XU verifies.
pub const XU_CS_LED1: u32 = 9;
pub const XU_LEN_LED1: usize = 5;
pub const XU_CS_FW_VERSION: u32 = 1;
pub const XU_LEN_FW_VERSION: usize = 4;
pub const XU_CS_DEV_IDENTITY: u32 = 6;
pub const XU_LEN_DEV_IDENTITY: usize = 6;
pub const XU_CS_DSP_PLATFORM: u32 = 10;
pub const XU_LEN_DSP_PLATFORM: usize = 16;

/// Frequence de clignotement par defaut en mode Blink (20 * 0,05 Hz = 1 Hz).
pub const DEFAULT_LED_BLINK_RAW: u8 = 20;

/// Valeur usine 5 octets du selecteur LED1 (CS=9) relevee sur une C920 0892.
pub const FACTORY_LED1_BYTES: [u8; 5] = [0x01, 0x03, 0x01, 0x00, 0x03];

/// Groupe de propriete Windows KS d'un controle UVC standard.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KsPropertyGroup {
    VideoProcAmp,
    CameraControl,
}

/// Retourne le groupe KS et l'ID numerique pour un controle UVC standard.
pub const fn ks_property(control: ControlId) -> (KsPropertyGroup, u32) {
    match control {
        ControlId::Brightness => (KsPropertyGroup::VideoProcAmp, VPA_BRIGHTNESS),
        ControlId::Contrast => (KsPropertyGroup::VideoProcAmp, VPA_CONTRAST),
        ControlId::Saturation => (KsPropertyGroup::VideoProcAmp, VPA_SATURATION),
        ControlId::Sharpness => (KsPropertyGroup::VideoProcAmp, VPA_SHARPNESS),
        ControlId::WhiteBalance => (KsPropertyGroup::VideoProcAmp, VPA_WHITEBALANCE),
        ControlId::BacklightComp => (KsPropertyGroup::VideoProcAmp, VPA_BACKLIGHT_COMPENSATION),
        ControlId::Gain => (KsPropertyGroup::VideoProcAmp, VPA_GAIN),
        ControlId::PowerLineFreq => (KsPropertyGroup::VideoProcAmp, VPA_POWERLINE_FREQUENCY),
        ControlId::Pan => (KsPropertyGroup::CameraControl, CC_PAN),
        ControlId::Tilt => (KsPropertyGroup::CameraControl, CC_TILT),
        ControlId::Zoom => (KsPropertyGroup::CameraControl, CC_ZOOM),
        ControlId::Exposure => (KsPropertyGroup::CameraControl, CC_EXPOSURE),
        ControlId::Focus => (KsPropertyGroup::CameraControl, CC_FOCUS),
        ControlId::LowLightComp => (KsPropertyGroup::CameraControl, CC_AUTO_EXPOSURE_PRIORITY),
    }
}

/// Indique si le controle attend `Flags = 0` sous `usbvideo.sys` (PowerlineFrequency=13, AutoExposurePriority=19).
pub const fn uses_zero_flags(control: ControlId) -> bool {
    matches!(control, ControlId::PowerLineFreq | ControlId::LowLightComp)
}

/// Specification GUID / selecteur / taille d'un controle XU autorise.
pub const fn xu_spec(control: XuControl) -> (Guid, u32, usize, bool) {
    // Dernier booleen : ecriture autorisee. Seul Led1Mode (CS=9) est autorise en ecriture.
    match control {
        XuControl::Led1Mode => (XU_LOGITECH_PERIPHERAL, XU_CS_LED1, XU_LEN_LED1, true),
        XuControl::FirmwareVersion => (XU_LOGITECH_DEV_INFO, XU_CS_FW_VERSION, XU_LEN_FW_VERSION, false),
        XuControl::DeviceIdentity => (XU_LOGITECH_DEV_INFO, XU_CS_DEV_IDENTITY, XU_LEN_DEV_IDENTITY, false),
        XuControl::DspPlatform => (XU_LOGITECH_AIT_CUSTOM, XU_CS_DSP_PLATFORM, XU_LEN_DSP_PLATFORM, false),
    }
}

/// Decode la charge utile 5 octets du controle XU LED1 (`CS = 9`).
/// Retourne `(mode, blink_raw)` ou `blink_raw` vaut `0..=255` en pas de `0,05 Hz`.
pub fn decode_led1(bytes: &[u8]) -> Result<(LedMode, u8)> {
    if bytes.len() < XU_LEN_LED1 {
        return Err(format!("Réponse XU LED1 tronquée ({} octets, 5 attendus)", bytes.len()));
    }
    let mode = match bytes[1] {
        0x00 => LedMode::Off,
        0x01 => LedMode::On,
        0x02 => LedMode::Blink,
        0x03 => LedMode::Auto,
        other => return Err(format!("Mode LED XU inconnu : {other:#04x}")),
    };
    Ok((mode, bytes[3]))
}

/// Prepare la charge utile 5 octets par Read-Modify-Write pour le controle XU LED1 (`CS = 9`).
/// Preserve intact les octets 0, 2 et 4 relus sur la webcam.
pub fn patch_led1(existing: &[u8], mode: LedMode, blink_raw: Option<u8>) -> Result<[u8; 5]> {
    if existing.len() < XU_LEN_LED1 {
        return Err(format!("Tampon XU LED1 invalide ({} octets, 5 attendus)", existing.len()));
    }
    let mut out = [existing[0], existing[1], existing[2], existing[3], existing[4]];
    out[1] = match mode {
        LedMode::Off => 0x00,
        LedMode::On => 0x01,
        LedMode::Blink => 0x02,
        LedMode::Auto => 0x03,
    };
    out[3] = match mode {
        LedMode::Blink => blink_raw.unwrap_or(if existing[3] > 0 { existing[3] } else { DEFAULT_LED_BLINK_RAW }),
        _ => 0x00,
    };
    Ok(out)
}

/// Decode la version firmware Logitech depuis `XU_LOGITECH_DEV_INFO` `CS = 1` (4 octets RO).
/// Formule officielle Logitech : `major = b[1]`, `minor = b[0]`, `build = u16::from_le_bytes([b[2], b[3]])`.
pub fn decode_firmware_version(bytes: &[u8]) -> Result<String> {
    if bytes.len() < XU_LEN_FW_VERSION {
        return Err("Version firmware XU tronquée".into());
    }
    let minor = bytes[0];
    let major = bytes[1];
    let build = u16::from_le_bytes([bytes[2], bytes[3]]);
    Ok(format!("{major}.{minor}.{build}"))
}

/// Decode `(vid, pid, bcd_device)` depuis `XU_LOGITECH_DEV_INFO` `CS = 6` (6 octets RO).
pub fn decode_device_identity(bytes: &[u8]) -> Result<(u16, u16, u16)> {
    if bytes.len() < XU_LEN_DEV_IDENTITY {
        return Err("Identité USB XU tronquée".into());
    }
    let vid = u16::from_le_bytes([bytes[0], bytes[1]]);
    let pid = u16::from_le_bytes([bytes[2], bytes[3]]);
    let bcd = u16::from_le_bytes([bytes[4], bytes[5]]);
    Ok((vid, pid, bcd))
}

/// Decode le nom de plateforme DSP (ASCII termine par nul) depuis `XU_LOGITECH_AIT_CUSTOM` `CS = 10` (16 octets RO).
pub fn decode_dsp_platform(bytes: &[u8]) -> Option<String> {
    let end = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
    let s = std::str::from_utf8(&bytes[..end]).ok()?.trim();
    (!s.is_empty() && s.bytes().all(|b| (0x20..=0x7E).contains(&b))).then(|| s.to_owned())
}

/// Identifie la revision commerciale d'une C920 d'apres son PID USB.
pub fn revision_label(pid: u16) -> String {
    match pid {
        crate::PID_C920_2019 => "HD Pro C920 (révision 2019+, 046D:0892)".into(),
        crate::PID_C920_LEGACY => "HD Pro C920 (révision classique H.264, 046D:082D)".into(),
        crate::PID_C920_PRO => "C920 PRO HD (046D:08E5)".into(),
        other => format!("Logitech C920 (046D:{other:04X})"),
    }
}

/// Constantes KS pour le decodage binaire portable des reponses `IOCTL_KS_PROPERTY`.
pub const KSPROPERTY_MEMBER_RANGES: u32 = 1;
pub const KSPROPERTY_MEMBER_STEPPEDRANGES: u32 = 2;
pub const KSPROPERTY_MEMBER_VALUES: u32 = 3;
pub const KSPROPERTY_MEMBER_FLAG_DEFAULT: u32 = 0x8000_0000;

/// Resultat du decodage d'un tampon `KSPROPERTY_TYPE_BASICSUPPORT`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BasicSupportRange {
    pub min: i32,
    pub max: i32,
    pub step: i32,
    pub default: Option<i32>,
}

fn read_u32_le(buf: &[u8], off: usize) -> Option<u32> {
    let slice: [u8; 4] = buf.get(off..off + 4)?.try_into().ok()?;
    Some(u32::from_le_bytes(slice))
}

fn read_i32_le(buf: &[u8], off: usize) -> Option<i32> {
    let slice: [u8; 4] = buf.get(off..off + 4)?.try_into().ok()?;
    Some(i32::from_le_bytes(slice))
}

fn read_guid_le(buf: &[u8], off: usize) -> Option<Guid> {
    let b = buf.get(off..off + 16)?;
    let data1 = u32::from_le_bytes(b[0..4].try_into().ok()?);
    let data2 = u16::from_le_bytes(b[4..6].try_into().ok()?);
    let data3 = u16::from_le_bytes(b[6..8].try_into().ok()?);
    let data4: [u8; 8] = b[8..16].try_into().ok()?;
    Some(Guid::new(data1, data2, data3, data4))
}

/// Extrait les indices des noeuds `KSNODETYPE_DEV_SPECIFIC` depuis un tampon `KSPROPERTY_TOPOLOGY_NODES`
/// (`KSMULTIPLE_ITEM` de 8 octets suivi de `count` GUID de 16 octets), sans dereferencement non aligne.
pub fn parse_topology_xu_nodes(buf: &[u8]) -> Vec<u32> {
    let Some(count) = read_u32_le(buf, 4) else {
        return Vec::new();
    };
    let mut ids = Vec::new();
    for i in 0..count as usize {
        let Some(off) = 8usize.checked_add(i.saturating_mul(16)) else {
            break;
        };
        let Some(guid) = read_guid_le(buf, off) else {
            break;
        };
        if guid == KSNODETYPE_DEV_SPECIFIC {
            ids.push(i as u32);
        }
    }
    ids
}

/// Decode un tampon `KSPROPERTY_TYPE_BASICSUPPORT` (`KSPROPERTY_DESCRIPTION` de 40 octets + listes de membres).
/// Sous `usbvideo.sys`, `MembersFlags = 1` (`RANGES`) avec `MembersSize = 16` contient un `KSPROPERTY_STEPPING_LONG`
/// complet (`[stepping_delta, reserved, min, max]`) : il est donc traite comme `STEPPEDRANGES` des que `elem_size >= 16`.
/// Renvoie `None` si les bornes sont absentes ou incoherentes (`min > max`).
pub fn parse_basic_support(buf: &[u8]) -> Option<BasicSupportRange> {
    const DESC_SZ: usize = 40;
    const HDR_SZ: usize = 16;
    if buf.len() < DESC_SZ + HDR_SZ {
        return None;
    }
    let members_list_count = read_u32_le(buf, 32)? as usize;
    let mut min: Option<i32> = None;
    let mut max: Option<i32> = None;
    let mut step: i32 = 1;
    let mut default: Option<i32> = None;

    let mut cursor = DESC_SZ;
    for _ in 0..members_list_count {
        if cursor.checked_add(HDR_SZ).is_none_or(|end| end > buf.len()) {
            break;
        }
        let members_flags = read_u32_le(buf, cursor)?;
        let elem_size = read_u32_le(buf, cursor + 4)? as usize;
        let count = read_u32_le(buf, cursor + 8)? as usize;
        let flags = read_u32_le(buf, cursor + 12)?;
        cursor += HDR_SZ;

        let Some(total_len) = elem_size.checked_mul(count) else {
            break;
        };
        if count == 0 || cursor.checked_add(total_len).is_none_or(|end| end > buf.len()) {
            break;
        }
        match members_flags {
            KSPROPERTY_MEMBER_STEPPEDRANGES | KSPROPERTY_MEMBER_RANGES if elem_size >= 16 => {
                let delta = read_u32_le(buf, cursor)?;
                let lo = read_i32_le(buf, cursor + 8)?;
                let hi = read_i32_le(buf, cursor + 12)?;
                min = Some(lo);
                max = Some(hi);
                step = i32::try_from(delta).unwrap_or(1).max(1);
            }
            KSPROPERTY_MEMBER_RANGES if elem_size == 8 => {
                let lo = read_i32_le(buf, cursor)?;
                let hi = read_i32_le(buf, cursor + 4)?;
                min = Some(lo);
                max = Some(hi);
                step = 1;
            }
            KSPROPERTY_MEMBER_VALUES if (flags & KSPROPERTY_MEMBER_FLAG_DEFAULT) != 0 && elem_size >= 4 => {
                default = read_i32_le(buf, cursor);
            }
            _ => {}
        }
        cursor += total_len;
    }

    let (min, max) = (min?, max?);
    if min > max {
        return None;
    }
    Some(BasicSupportRange {
        min,
        max,
        step: step.max(1),
        default: default.map(|d| d.clamp(min, max)),
    })
}

/// Decode la valeur par defaut depuis un tampon `KSPROPERTY_TYPE_DEFAULTVALUES`, sans dereferencement non aligne.
pub fn parse_default_value(buf: &[u8]) -> Option<i32> {
    const DESC_SZ: usize = 40;
    const HDR_SZ: usize = 16;
    const VAL_OFF: usize = DESC_SZ + HDR_SZ;
    if buf.len() == 4 {
        return read_i32_le(buf, 0);
    }
    if buf.len() < VAL_OFF + 4 {
        return None;
    }
    let elem_size = read_u32_le(buf, DESC_SZ + 4)? as usize;
    if elem_size == 4 && VAL_OFF + 4 <= buf.len() {
        return read_i32_le(buf, VAL_OFF);
    }
    if elem_size >= 36 && VAL_OFF + 36 <= buf.len() {
        return read_i32_le(buf, VAL_OFF + 24);
    }
    None
}
