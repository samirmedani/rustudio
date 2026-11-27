//! Fausse Logitech C920 : reproduit exactement les plages UVC et les unites XU de la vraie 046D:0892
//! (FW 7.59.1035, SoC AIT PYTHON_V2B, LED1 5 octets CS=9, Powerline 1..2, AutoExposurePriority 0..1),
//! ainsi que les deconnexions et les reinitialisations externes (Windows, OBS, Discord).
use crate::uvc::{decode_led1, revision_label, FACTORY_LED1_BYTES};
use crate::*;
use std::collections::HashMap;
use std::path::PathBuf;

#[derive(Clone, Debug)]
pub struct Sim {
    pub info: DeviceInfo,
    pub ranges: Ranges,
    pub values: HashMap<ControlId, ControlValue>,
    pub led1_bytes: [u8; 5],
    pub firmware_bytes: [u8; 4],
    pub identity_bytes: [u8; 6],
    pub dsp_bytes: [u8; 16],
    pub writes: usize,
    pub disconnected: bool,
    /// Si ce fichier existe, la webcam simulee apparait debranchee (tests d'integration).
    pub disconnect_flag: Option<PathBuf>,
    /// Si ce fichier existe, la webcam simule une reinitialisation externe (ex. OBS/Discord) puis consomme le drapeau.
    pub reset_flag: Option<PathBuf>,
}

impl Default for Sim {
    fn default() -> Self {
        let ranges = Ranges::default();
        let mut values = HashMap::new();
        for control in ControlId::ALL {
            let r = ranges.get(control);
            values.insert(
                control,
                ControlValue {
                    value: r.default,
                    auto: r.supports_auto,
                },
            );
        }
        let info = DeviceInfo {
            name: "HD Pro Webcam C920".into(),
            vid: VID,
            pid: PID_C920_2019,
            revision: "0019".into(),
            firmware: Some("7.59.1035".into()),
            dsp_platform: Some("PYTHON_V2B".into()),
            revision_label: revision_label(PID_C920_2019),
            formats: vec![
                VideoFormat { codec: "MJPG".into(), width: 1920, height: 1080, max_fps: 30 },
                VideoFormat { codec: "MJPG".into(), width: 1280, height: 720, max_fps: 30 },
                VideoFormat { codec: "YUY2".into(), width: 2304, height: 1536, max_fps: 2 },
                VideoFormat { codec: "YUY2".into(), width: 1920, height: 1080, max_fps: 5 },
                VideoFormat { codec: "YUY2".into(), width: 1280, height: 720, max_fps: 10 },
                VideoFormat { codec: "YUY2".into(), width: 800, height: 448, max_fps: 30 },
            ],
            xu: XuInfo {
                led_supported: true,
                units: vec![
                    XuUnitDesc {
                        node_id: 2,
                        guid: uvc::XU_LOGITECH_AIT_CUSTOM.format_hyphenated(),
                        name: "Logitech AIT Platform (lecture seule)".into(),
                    },
                    XuUnitDesc {
                        node_id: 3,
                        guid: uvc::XU_LOGITECH_DEV_INFO.format_hyphenated(),
                        name: "Logitech Device Info (firmware)".into(),
                    },
                    XuUnitDesc {
                        node_id: 6,
                        guid: uvc::XU_LOGITECH_PERIPHERAL.format_hyphenated(),
                        name: "Logitech Peripheral Control (LED)".into(),
                    },
                ],
            },
        };
        let mut dsp_bytes = [0u8; 16];
        dsp_bytes[..10].copy_from_slice(b"PYTHON_V2B");
        Self {
            info,
            ranges,
            values,
            led1_bytes: FACTORY_LED1_BYTES,
            // 7.59.1035 -> [minor=59 (0x3B), major=7 (0x07), build=1035 (0x040B LE -> 0x0B, 0x04)]
            firmware_bytes: [0x3B, 0x07, 0x0B, 0x04],
            // VID=0x046D, PID=0x0892, bcdDevice=0x0019 (LE)
            identity_bytes: [0x6D, 0x04, 0x92, 0x08, 0x19, 0x00],
            dsp_bytes,
            writes: 0,
            disconnected: false,
            disconnect_flag: std::env::var_os("RUSTUDIO_SIM_C920_DISCONNECT_FILE").map(Into::into),
            reset_flag: std::env::var_os("RUSTUDIO_SIM_C920_RESET_FILE").map(Into::into),
        }
    }
}

impl Sim {
    /// Simule une reinitialisation externe (OBS, Discord, sortie de veille Windows).
    pub fn simulate_external_reset(&mut self) {
        for control in ControlId::ALL {
            let r = self.ranges.get(control);
            self.values.insert(
                control,
                ControlValue {
                    value: r.default,
                    auto: r.supports_auto,
                },
            );
        }
        self.led1_bytes = FACTORY_LED1_BYTES;
    }

    fn check_online(&mut self) -> Result<()> {
        if self.disconnected || self.disconnect_flag.as_ref().is_some_and(|p| p.exists()) {
            return Err("Webcam simulée débranchée".into());
        }
        if let Some(flag) = self.reset_flag.clone() {
            if flag.exists() {
                let _ = std::fs::remove_file(&flag);
                self.simulate_external_reset();
            }
        }
        Ok(())
    }
}

impl Transport for Sim {
    fn info(&mut self) -> Result<DeviceInfo> {
        self.check_online()?;
        Ok(self.info.clone())
    }

    fn range(&mut self, control: ControlId) -> Result<Option<ControlRange>> {
        self.check_online()?;
        // Reproduit le comportement reel de usbvideo.sys sur AutoExposurePriority (GetRange renvoie 0..0).
        if control == ControlId::LowLightComp {
            return Ok(Some(ControlRange::new(0, 0, 1, 0, false, true)));
        }
        Ok(Some(self.ranges.get(control)))
    }

    fn get(&mut self, control: ControlId) -> Result<ControlValue> {
        self.check_online()?;
        self.values
            .get(&control)
            .copied()
            .ok_or_else(|| format!("Contrôle simulé absent : {}", control.label()))
    }

    fn set(&mut self, control: ControlId, value: ControlValue) -> Result<()> {
        self.check_online()?;
        let r = self.ranges.get(control);
        if value.auto && !r.supports_auto {
            return Err(format!("{} ne prend pas en charge le mode automatique", control.label()));
        }
        if !value.auto && !r.supports_manual {
            return Err(format!("{} ne prend pas en charge le mode manuel", control.label()));
        }
        if !value.auto {
            r.validate(control.label(), value.value)?;
        }
        let next = if value.auto {
            let prev = self.values.get(&control).map(|v| v.value).unwrap_or(r.default);
            ControlValue { value: prev, auto: true }
        } else {
            ControlValue { value: value.value, auto: false }
        };
        self.values.insert(control, next);
        self.writes += 1;
        Ok(())
    }

    fn get_xu(&mut self, control: XuControl) -> Result<Option<Vec<u8>>> {
        self.check_online()?;
        match control {
            XuControl::Led1Mode => Ok(self.info.xu.led_supported.then(|| self.led1_bytes.to_vec())),
            XuControl::FirmwareVersion => Ok(Some(self.firmware_bytes.to_vec())),
            XuControl::DeviceIdentity => Ok(Some(self.identity_bytes.to_vec())),
            XuControl::DspPlatform => Ok(Some(self.dsp_bytes.to_vec())),
        }
    }

    fn set_xu(&mut self, control: XuControl, data: &[u8]) -> Result<()> {
        self.check_online()?;
        let (_, _, expected_len, writable) = uvc::xu_spec(control);
        if !writable {
            return Err("Ce sélecteur XU est en lecture seule".into());
        }
        match control {
            XuControl::Led1Mode => {
                if !self.info.xu.led_supported {
                    return Err("XU LED non pris en charge".into());
                }
                if data.len() != expected_len {
                    return Err(format!("Taille XU LED1 invalide ({} octets, 5 attendus)", data.len()));
                }
                decode_led1(data)?;
                self.led1_bytes.copy_from_slice(data);
                self.writes += 1;
                Ok(())
            }
            _ => Err("Écriture XU interdite".into()),
        }
    }
}
