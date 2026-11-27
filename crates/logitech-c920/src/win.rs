//! Transport Windows natif (Kernel Streaming / Configuration Manager) pour la Logitech C920.
//! Fonctionne aussi bien en session utilisateur qu'en service Windows (Session 0), sans COM ni ouverture
//! de flux video ou audio : n'interrompt pas OBS, Discord ou Teams et ne touche jamais au micro (MI_02 exclu).
//!
//! Alignement materiel (sonde reelle sur 046D:0892, FW 7.59.1035, SoC AIT PYTHON_V2B) :
//! - VideoProcAmp (`C6E13360`) : Brightness(0), Contrast(1), Saturation(3), Sharpness(4), WhiteBalance(7),
//!   Backlight(8), Gain(9), PowerlineFrequency(13, flags=0).
//! - CameraControl (`C6E13370`) : Pan(0), Tilt(1), Zoom(3), Exposure(4), Focus(6), AutoExposurePriority(19, flags=0).
//! - XU dynamiquement resolues via `KSNODETYPE_DEV_SPECIFIC` + `KSPROPERTY_TYPE_SETSUPPORT | TOPOLOGY` :
//!   * LED1 : `{ffe52d21-8030-4e2c-82d9-f587d00540bd}` CS=9 (5 octets RW, Read-Modify-Write)
//!   * Firmware / Identite : `{69678ee4-410f-40db-a850-7420d7d8240e}` CS=1 (4 octets RO), CS=6 (6 octets RO)
//!   * Plateforme DSP : `{23e49ed0-1178-4f31-ae52-d2fb8a8d3b48}` CS=10 (16 octets RO, JAMAIS ecrire)
use crate::uvc::{
    self, Guid, KsPropertyGroup, KSCATEGORY_CAPTURE, KSCATEGORY_VIDEO_CAMERA, KSPROPSETID_TOPOLOGY,
    KS_FLAG_AUTO, KS_FLAG_MANUAL, KS_FLAG_NONE, PROPSETID_VIDCAP_CAMERACONTROL,
    PROPSETID_VIDCAP_VIDEOPROCAMP,
};
use crate::{
    ControlId, ControlRange, ControlValue, DeviceInfo, Ranges, Result, Transport, VideoFormat, XuControl,
    XuInfo, XuUnitDesc, PIDS, VID,
};
use std::collections::HashMap;
use std::mem::size_of;
use std::ptr::{null, null_mut};
use windows_sys::Win32::Devices::DeviceAndDriverInstallation::{
    CM_Get_Device_Interface_ListW, CM_Get_Device_Interface_List_SizeW,
    CM_GET_DEVICE_INTERFACE_LIST_PRESENT, CR_BUFFER_SMALL, CR_SUCCESS,
};
use windows_sys::Win32::Foundation::{
    CloseHandle, GetLastError, GENERIC_READ, GENERIC_WRITE, HANDLE, INVALID_HANDLE_VALUE,
};
use windows_sys::Win32::Storage::FileSystem::{
    CreateFileW, FILE_ATTRIBUTE_NORMAL, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING,
};
use windows_sys::Win32::System::IO::DeviceIoControl;

const IOCTL_KS_PROPERTY: u32 = 0x002F_0003;
const KSPROPERTY_TYPE_GET: u32 = 0x0000_0001;
const KSPROPERTY_TYPE_SET: u32 = 0x0000_0002;
const KSPROPERTY_TYPE_SETSUPPORT: u32 = 0x0000_0100;
const KSPROPERTY_TYPE_BASICSUPPORT: u32 = 0x0000_0200;
const KSPROPERTY_TYPE_DEFAULTVALUES: u32 = 0x0000_0800;
const KSPROPERTY_TYPE_TOPOLOGY: u32 = 0x1000_0000;

const KSPROPERTY_TOPOLOGY_NODES: u32 = 1;

/// Dans le SDK Windows (`ks.h`), `KSIDENTIFIER` / `KSPROPERTY` contient `LONGLONG Alignment` (`align(8)`),
/// ce qui donne `sizeof(KSPROPERTY) == 24`, `sizeof(KSP_NODE) == 32` et `sizeof(KSPROPERTY_VIDEOPROCAMP_S) == 40`.
#[repr(C, align(8))]
#[derive(Clone, Copy)]
struct KsProperty {
    set: Guid,
    id: u32,
    flags: u32,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct KspNode {
    property: KsProperty,
    node_id: u32,
    reserved: u32,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct KsVidCapS {
    property: KsProperty,
    value: i32,
    flags: u32,
    capabilities: u32,
}

const _: () = assert!(size_of::<KsProperty>() == 24);
const _: () = assert!(size_of::<KspNode>() == 32);
const _: () = assert!(size_of::<KsVidCapS>() == 40);

/// Handle Kernel Streaming ouvert sur l'interface video (MI_00) de la Logitech C920.
pub struct KsCamera {
    handle: HANDLE,
    info: DeviceInfo,
    xu_nodes: HashMap<XuControl, u32>,
}

// Le handle Win32 est possede exclusivement par un fil unique (`c920-uvc`).
unsafe impl Send for KsCamera {}

impl Drop for KsCamera {
    fn drop(&mut self) {
        if self.handle != INVALID_HANDLE_VALUE && !self.handle.is_null() {
            unsafe {
                CloseHandle(self.handle);
            }
        }
    }
}

impl KsCamera {
    /// Ouvre la premiere Logitech C920 detectee sur les interfaces video Windows (MI_00 uniquement).
    pub fn open() -> Result<Self> {
        let (path, pid, revision) = find_c920_interface()?;
        let mut wide: Vec<u16> = path.encode_utf16().collect();
        wide.push(0);
        // Meme ouverture partagee que ksproxy.ax : fonctionne pendant qu'OBS/Discord capturent la video.
        let mut handle = unsafe {
            CreateFileW(
                wide.as_ptr(),
                GENERIC_READ | GENERIC_WRITE,
                FILE_SHARE_READ | FILE_SHARE_WRITE,
                null(),
                OPEN_EXISTING,
                FILE_ATTRIBUTE_NORMAL,
                null_mut(),
            )
        };
        if handle == INVALID_HANDLE_VALUE || handle.is_null() {
            handle = unsafe {
                CreateFileW(
                    wide.as_ptr(),
                    0,
                    FILE_SHARE_READ | FILE_SHARE_WRITE,
                    null(),
                    OPEN_EXISTING,
                    FILE_ATTRIBUTE_NORMAL,
                    null_mut(),
                )
            };
        }
        if handle == INVALID_HANDLE_VALUE || handle.is_null() {
            let err = unsafe { GetLastError() };
            return Err(format!("Impossible d'ouvrir la Logitech C920 (code Windows {err})"));
        }

        let mut cam = Self {
            handle,
            info: DeviceInfo {
                name: "HD Pro Webcam C920".into(),
                vid: VID,
                pid,
                revision: revision.clone(),
                firmware: parse_revision_firmware(&revision),
                dsp_platform: None,
                revision_label: uvc::revision_label(pid),
                formats: default_formats(pid),
                xu: XuInfo::default(),
            },
            xu_nodes: HashMap::new(),
        };
        cam.discover_xu();
        Ok(cam)
    }

    fn ioctl_ks(&self, input: &[u8], output: &mut [u8]) -> std::result::Result<usize, u32> {
        let mut returned: u32 = 0;
        let ok = unsafe {
            DeviceIoControl(
                self.handle,
                IOCTL_KS_PROPERTY,
                input.as_ptr().cast(),
                input.len() as u32,
                if output.is_empty() { null_mut() } else { output.as_mut_ptr().cast() },
                output.len() as u32,
                &mut returned,
                null_mut(),
            )
        };
        if ok == 0 {
            Err(unsafe { GetLastError() })
        } else {
            Ok(returned as usize)
        }
    }

    fn topology_xu_node_ids(&self) -> Vec<u32> {
        let prop = KsProperty {
            set: KSPROPSETID_TOPOLOGY,
            id: KSPROPERTY_TOPOLOGY_NODES,
            flags: KSPROPERTY_TYPE_GET,
        };
        let mut buf = vec![0u8; 4096];
        let Ok(n) = self.ioctl_ks(as_bytes(&prop), &mut buf) else {
            return Vec::new();
        };
        uvc::parse_topology_xu_nodes(&buf[..n.min(buf.len())])
    }

    fn node_supports_guid(&self, node_id: u32, guid: Guid) -> bool {
        let req = KspNode {
            property: KsProperty {
                set: guid,
                id: 0,
                flags: KSPROPERTY_TYPE_SETSUPPORT | KSPROPERTY_TYPE_TOPOLOGY,
            },
            node_id,
            reserved: 0,
        };
        let mut dummy = [0u8; 4];
        self.ioctl_ks(as_bytes(&req), &mut dummy).is_ok()
            || self.ioctl_ks(as_bytes(&req), &mut []).is_ok()
    }

    fn read_xu_node(&self, node_id: u32, guid: Guid, selector: u32, len: usize) -> Option<Vec<u8>> {
        let req = KspNode {
            property: KsProperty {
                set: guid,
                id: selector,
                flags: KSPROPERTY_TYPE_GET | KSPROPERTY_TYPE_TOPOLOGY,
            },
            node_id,
            reserved: 0,
        };
        let mut out = vec![0u8; len.max(1)];
        let n = self.ioctl_ks(as_bytes(&req), &mut out).ok()?;
        out.truncate(n);
        Some(out)
    }

    fn discover_xu(&mut self) {
        let dev_specific_nodes = self.topology_xu_node_ids();
        let mut units = Vec::new();
        let specs: &[(Guid, &str, &[XuControl])] = &[
            (
                uvc::XU_LOGITECH_AIT_CUSTOM,
                "Logitech AIT Platform (lecture seule)",
                &[XuControl::DspPlatform],
            ),
            (
                uvc::XU_LOGITECH_DEV_INFO,
                "Logitech Device Info (firmware)",
                &[XuControl::FirmwareVersion, XuControl::DeviceIdentity],
            ),
            (
                uvc::XU_LOGITECH_PERIPHERAL,
                "Logitech Peripheral Control (LED)",
                &[XuControl::Led1Mode],
            ),
        ];

        for &(guid, label, controls) in specs {
            for &node_id in &dev_specific_nodes {
                let matched = self.node_supports_guid(node_id, guid) || {
                    let (_, sel, len, _) = uvc::xu_spec(controls[0]);
                    self.read_xu_node(node_id, guid, sel, len).is_some()
                };
                if !matched {
                    continue;
                }
                let mut any_readable = false;
                for &ctrl in controls {
                    let (_, sel, len, _) = uvc::xu_spec(ctrl);
                    if let Some(bytes) = self.read_xu_node(node_id, guid, sel, len) {
                        let valid = match ctrl {
                            XuControl::Led1Mode => uvc::decode_led1(&bytes).is_ok(),
                            XuControl::FirmwareVersion => uvc::decode_firmware_version(&bytes).is_ok(),
                            XuControl::DeviceIdentity => uvc::decode_device_identity(&bytes).is_ok(),
                            XuControl::DspPlatform => uvc::decode_dsp_platform(&bytes).is_some(),
                        };
                        if valid {
                            self.xu_nodes.insert(ctrl, node_id);
                            any_readable = true;
                        }
                    }
                }
                if any_readable {
                    units.push(XuUnitDesc {
                        node_id,
                        guid: guid.format_hyphenated(),
                        name: label.into(),
                    });
                    break;
                }
            }
        }
        self.info.xu = XuInfo {
            led_supported: self.xu_nodes.contains_key(&XuControl::Led1Mode),
            units,
        };
    }

    fn query_default(&self, set: Guid, id: u32) -> Option<i32> {
        let prop = KsProperty { set, id, flags: KSPROPERTY_TYPE_DEFAULTVALUES };
        let mut buf = [0u8; 256];
        let n = self.ioctl_ks(as_bytes(&prop), &mut buf).ok()?;
        uvc::parse_default_value(&buf[..n.min(buf.len())])
    }
}

impl Transport for KsCamera {
    fn info(&mut self) -> Result<DeviceInfo> {
        Ok(self.info.clone())
    }

    fn range(&mut self, control: ControlId) -> Result<Option<ControlRange>> {
        let (group, id) = uvc::ks_property(control);
        let set = match group {
            KsPropertyGroup::VideoProcAmp => PROPSETID_VIDCAP_VIDEOPROCAMP,
            KsPropertyGroup::CameraControl => PROPSETID_VIDCAP_CAMERACONTROL,
        };
        let prop = KsProperty { set, id, flags: KSPROPERTY_TYPE_BASICSUPPORT };
        let mut buf = [0u8; 512];
        let Ok(n) = self.ioctl_ks(as_bytes(&prop), &mut buf) else {
            return Ok(None);
        };
        let Some(parsed) = uvc::parse_basic_support(&buf[..n.min(buf.len())]) else {
            return Ok(None);
        };
        if parsed.min > parsed.max {
            return Ok(None);
        }

        let mut caps_auto = false;
        let mut caps_manual = true;
        if !uvc::uses_zero_flags(control) {
            let input = KsVidCapS {
                property: KsProperty { set, id, flags: KSPROPERTY_TYPE_GET },
                value: 0,
                flags: 0,
                capabilities: 0,
            };
            let mut output = input;
            if self.ioctl_ks(as_bytes(&input), as_bytes_mut(&mut output)).is_ok() {
                caps_auto = (output.capabilities & KS_FLAG_AUTO) != 0;
                caps_manual = (output.capabilities & KS_FLAG_MANUAL) != 0 || !caps_auto;
            }
        }
        let fallback_default = Ranges::default().get(control).default;
        let default = parsed
            .default
            .or_else(|| self.query_default(set, id))
            .unwrap_or(fallback_default)
            .clamp(parsed.min, parsed.max);
        Ok(Some(ControlRange::new(
            parsed.min,
            parsed.max,
            parsed.step,
            default,
            caps_auto,
            caps_manual,
        )))
    }

    fn get(&mut self, control: ControlId) -> Result<ControlValue> {
        let (group, id) = uvc::ks_property(control);
        let set = match group {
            KsPropertyGroup::VideoProcAmp => PROPSETID_VIDCAP_VIDEOPROCAMP,
            KsPropertyGroup::CameraControl => PROPSETID_VIDCAP_CAMERACONTROL,
        };
        let input = KsVidCapS {
            property: KsProperty { set, id, flags: KSPROPERTY_TYPE_GET },
            value: 0,
            flags: 0,
            capabilities: 0,
        };
        let mut output = input;
        self.ioctl_ks(as_bytes(&input), as_bytes_mut(&mut output))
            .map_err(|code| ks_error("Lecture", control.label(), code))?;
        Ok(ControlValue {
            value: output.value,
            auto: !uvc::uses_zero_flags(control) && (output.flags & KS_FLAG_AUTO) != 0,
        })
    }

    fn set(&mut self, control: ControlId, value: ControlValue) -> Result<()> {
        let (group, id) = uvc::ks_property(control);
        let set = match group {
            KsPropertyGroup::VideoProcAmp => PROPSETID_VIDCAP_VIDEOPROCAMP,
            KsPropertyGroup::CameraControl => PROPSETID_VIDCAP_CAMERACONTROL,
        };
        let flags = if uvc::uses_zero_flags(control) {
            KS_FLAG_NONE
        } else if value.auto {
            KS_FLAG_AUTO
        } else {
            KS_FLAG_MANUAL
        };
        let input = KsVidCapS {
            property: KsProperty { set, id, flags: KSPROPERTY_TYPE_SET },
            value: value.value,
            flags,
            capabilities: 0,
        };
        let mut output = input;
        self.ioctl_ks(as_bytes(&input), as_bytes_mut(&mut output))
            .map_err(|code| ks_error("Écriture", control.label(), code))?;
        Ok(())
    }

    fn get_xu(&mut self, control: XuControl) -> Result<Option<Vec<u8>>> {
        let Some(&node_id) = self.xu_nodes.get(&control) else {
            return Ok(None);
        };
        let (guid, selector, len, _) = uvc::xu_spec(control);
        Ok(self.read_xu_node(node_id, guid, selector, len))
    }

    fn set_xu(&mut self, control: XuControl, data: &[u8]) -> Result<()> {
        let (guid, selector, expected_len, writable) = uvc::xu_spec(control);
        if !writable {
            return Err("Ce sélecteur XU est en lecture seule".into());
        }
        if data.len() != expected_len {
            return Err(format!("Taille XU invalide ({} octets, {expected_len} attendus)", data.len()));
        }
        let &node_id = self
            .xu_nodes
            .get(&control)
            .ok_or_else(|| "Unité XU Logitech non prise en charge".to_owned())?;
        let req = KspNode {
            property: KsProperty {
                set: guid,
                id: selector,
                flags: KSPROPERTY_TYPE_SET | KSPROPERTY_TYPE_TOPOLOGY,
            },
            node_id,
            reserved: 0,
        };
        let mut payload = data.to_vec();
        self.ioctl_ks(as_bytes(&req), &mut payload)
            .map_err(|code| ks_error("Écriture", "XU Logitech", code))?;
        Ok(())
    }
}

fn ks_error(op: &str, label: &str, code: u32) -> String {
    if matches!(code, 2 | 31 | 1167) {
        return "Logitech C920 débranchée".into();
    }
    format!("{op} {label} impossible (code Windows {code})")
}

fn find_c920_interface() -> Result<(String, u16, String)> {
    for category in [KSCATEGORY_VIDEO_CAMERA, KSCATEGORY_CAPTURE] {
        if let Some(found) = enumerate_category(category) {
            return Ok(found);
        }
    }
    Err(crate::NO_DEVICE.into())
}

fn enumerate_category(category: Guid) -> Option<(String, u16, String)> {
    let win_guid = windows_sys::core::GUID {
        data1: category.data1,
        data2: category.data2,
        data3: category.data3,
        data4: category.data4,
    };
    let paths = loop {
        let mut length = 0u32;
        let mut status = unsafe {
            CM_Get_Device_Interface_List_SizeW(
                &mut length,
                &win_guid,
                null(),
                CM_GET_DEVICE_INTERFACE_LIST_PRESENT,
            )
        };
        let mut list = vec![0u16; length as usize];
        if status == CR_SUCCESS && length > 0 {
            status = unsafe {
                CM_Get_Device_Interface_ListW(
                    &win_guid,
                    null(),
                    list.as_mut_ptr(),
                    length,
                    CM_GET_DEVICE_INTERFACE_LIST_PRESENT,
                )
            };
        }
        match status {
            CR_SUCCESS => {
                break list
                    .split(|&unit| unit == 0)
                    .filter(|path| !path.is_empty())
                    .map(String::from_utf16_lossy)
                    .collect::<Vec<_>>();
            }
            CR_BUFFER_SMALL => continue,
            _ => return None,
        }
    };
    for path in paths {
        let upper = path.to_ascii_uppercase();
        // Uniquement l'interface video (MI_00), jamais l'interface audio (MI_02).
        if !upper.contains("VID_046D") || !upper.contains("MI_00") || upper.contains("MI_02") {
            continue;
        }
        for pid in PIDS {
            let needle = format!("PID_{pid:04X}");
            if upper.contains(&needle) {
                let rev = extract_rev(&upper).unwrap_or_else(|| "0000".into());
                return Some((path, pid, rev));
            }
        }
    }
    None
}

fn extract_rev(upper: &str) -> Option<String> {
    let idx = upper.find("REV_")?;
    let rest = &upper[idx + 4..];
    let rev: String = rest.chars().take_while(|c| c.is_ascii_hexdigit()).collect();
    (!rev.is_empty()).then_some(rev)
}

fn parse_revision_firmware(rev: &str) -> Option<String> {
    if rev.len() == 4 && rev.chars().all(|c| c.is_ascii_hexdigit()) && rev != "0000" {
        let major = u8::from_str_radix(&rev[0..2], 16).ok()?;
        let minor = u8::from_str_radix(&rev[2..4], 16).ok()?;
        return Some(format!("{major}.{minor:02}"));
    }
    None
}

fn default_formats(pid: u16) -> Vec<VideoFormat> {
    let mut out = vec![
        VideoFormat { codec: "MJPG".into(), width: 1920, height: 1080, max_fps: 30 },
        VideoFormat { codec: "MJPG".into(), width: 1280, height: 720, max_fps: 30 },
        VideoFormat { codec: "YUY2".into(), width: 2304, height: 1536, max_fps: 2 },
        VideoFormat { codec: "YUY2".into(), width: 1920, height: 1080, max_fps: 5 },
        VideoFormat { codec: "YUY2".into(), width: 1280, height: 720, max_fps: 10 },
        VideoFormat { codec: "YUY2".into(), width: 800, height: 448, max_fps: 30 },
    ];
    if pid == crate::PID_C920_LEGACY {
        out.push(VideoFormat { codec: "H264".into(), width: 1920, height: 1080, max_fps: 30 });
    }
    out
}

fn as_bytes<T>(v: &T) -> &[u8] {
    unsafe { std::slice::from_raw_parts((v as *const T).cast::<u8>(), size_of::<T>()) }
}

fn as_bytes_mut<T>(v: &mut T) -> &mut [u8] {
    unsafe { std::slice::from_raw_parts_mut((v as *mut T).cast::<u8>(), size_of::<T>()) }
}
