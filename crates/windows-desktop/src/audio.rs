//! Peripheriques audio par defaut de Windows, lus et corriges depuis la session de l'utilisateur.
//! Le moteur tourne en compte systeme et ne voit pas ces reglages (propres a chaque compte) : c'est l'appli qui
//! les lit (IMMDeviceEnumerator) et les change (IPolicyConfig), comme le panneau Son de Windows.
//! Cause d'echo la plus frequente : Windows enregistre « Broadcast Stream Mix » au lieu de « Chat Mic ».
use serde_json::{json, Value};

pub const MIC: &str = "Chat Mic";
pub const SOUND: &str = "System";
pub const VOICE: &str = "Chat";
/// Nom Windows de la Mini sous firmware Rustudio : une sortie et un micro.
pub const RUSTUDIO_DEVICE: &str = "Rustudio Mini Audio";

/// Vrai si le peripherique Windows est une sortie audio exposee par le firmware Rustudio.
pub fn is_rustudio_output(device: &str) -> bool {
    device.eq_ignore_ascii_case(RUSTUDIO_DEVICE)
        || device
            .strip_prefix("Rustudio ")
            .is_some_and(|c| ["Game", "Chat", "Music", "Sample"].contains(&c))
}

/// Nom de voie GoXLR d'une entree ou sortie Windows, et si elle appartient a la GoXLR. Firmware Rustudio :
/// « Rustudio Mini Audio » porte System et le micro (Chat Mic), puis une sortie par voie a son nom.
pub fn goxlr_channel(device: &str, name: String, capture: bool) -> (String, bool) {
    if device.eq_ignore_ascii_case(RUSTUDIO_DEVICE) {
        ((if capture { MIC } else { SOUND }).to_owned(), true)
    } else if let (false, Some(channel)) = (capture, device.strip_prefix("Rustudio ").filter(|c| ["Game", "Chat", "Music", "Sample"].contains(c))) {
        (channel.to_owned(), true)
    } else {
        (name, device.to_lowercase().contains("goxlr"))
    }
}

/// Construit un WAVEFORMATEXTENSIBLE (40 octets) PCM 24 bits / 48 kHz stereo (format natif du firmware Rustudio).
#[cfg_attr(not(windows), allow(dead_code))]
pub(crate) fn wav_ext_pcm24_48k() -> [u8; 40] {
    let mut b = [0u8; 40];
    b[0..2].copy_from_slice(&0xFFFEu16.to_le_bytes()); // WAVE_FORMAT_EXTENSIBLE
    b[2..4].copy_from_slice(&2u16.to_le_bytes()); // nChannels
    b[4..8].copy_from_slice(&48_000u32.to_le_bytes()); // nSamplesPerSec
    b[8..12].copy_from_slice(&(48_000u32 * 6).to_le_bytes()); // nAvgBytesPerSec
    b[12..14].copy_from_slice(&6u16.to_le_bytes()); // nBlockAlign
    b[14..16].copy_from_slice(&24u16.to_le_bytes()); // wBitsPerSample
    b[16..18].copy_from_slice(&22u16.to_le_bytes()); // cbSize
    b[18..20].copy_from_slice(&24u16.to_le_bytes()); // wValidBitsPerSample
    b[20..24].copy_from_slice(&3u32.to_le_bytes()); // SPEAKER_FRONT_LEFT | SPEAKER_FRONT_RIGHT
    b[24..40].copy_from_slice(&[1, 0, 0, 0, 0, 0, 0x10, 0, 0x80, 0, 0, 0xaa, 0, 0x38, 0x9b, 0x71]); // PCM
    b
}

/// Construit un WAVEFORMATEXTENSIBLE (40 octets) float 32 bits / 48 kHz stereo (format de mixage Windows).
#[cfg_attr(not(windows), allow(dead_code))]
pub(crate) fn wav_ext_f32_48k() -> [u8; 40] {
    let mut b = [0u8; 40];
    b[0..2].copy_from_slice(&0xFFFEu16.to_le_bytes());
    b[2..4].copy_from_slice(&2u16.to_le_bytes());
    b[4..8].copy_from_slice(&48_000u32.to_le_bytes());
    b[8..12].copy_from_slice(&(48_000u32 * 8).to_le_bytes());
    b[12..14].copy_from_slice(&8u16.to_le_bytes());
    b[14..16].copy_from_slice(&32u16.to_le_bytes());
    b[16..18].copy_from_slice(&22u16.to_le_bytes());
    b[18..20].copy_from_slice(&32u16.to_le_bytes());
    b[20..24].copy_from_slice(&3u32.to_le_bytes());
    b[24..40].copy_from_slice(&[3, 0, 0, 0, 0, 0, 0x10, 0, 0x80, 0, 0, 0xaa, 0, 0x38, 0x9b, 0x71]); // IEEE_FLOAT
    b
}

/// Verifie qu'un en-tete WAVEFORMATEX (18 octets minimum) correspond a du stereo 24 bits / 48 kHz.
#[cfg_attr(not(windows), allow(dead_code))]
pub(crate) fn is_pcm24_48k_header(wf: &[u8]) -> bool {
    if wf.len() < 16 {
        return false;
    }
    let ch = u16::from_le_bytes([wf[2], wf[3]]);
    let rate = u32::from_le_bytes([wf[4], wf[5], wf[6], wf[7]]);
    let bits = u16::from_le_bytes([wf[14], wf[15]]);
    ch == 2 && rate == 48_000 && bits == 24
}

#[derive(Clone, Debug)]
pub struct Endpoint {
    pub id: String,
    pub name: String,
    pub goxlr: bool,
}

/// Etat envoye a l'interface ; issues : ce qui fait de l'echo ou contourne la GoXLR.
pub fn status_from(render: &[Endpoint], capture: &[Endpoint], defaults: [Option<String>; 4]) -> Value {
    let find = |all: &[Endpoint], id: &Option<String>| id.as_ref().and_then(|i| all.iter().find(|e| e.id.eq_ignore_ascii_case(i)).cloned());
    let goxlr = render.iter().any(|e| e.goxlr) && capture.iter().any(|e| e.goxlr && e.name == MIC);
    let (r0, r2) = (find(render, &defaults[0]), find(render, &defaults[1]));
    let (c0, c2) = (find(capture, &defaults[2]), find(capture, &defaults[3]));
    let mut issues = vec![];
    if goxlr {
        for (e, role) in [(&c0, "console"), (&c2, "communications")] {
            match e {
                None => issues.push(json!({"kind": "capture-none", "role": role, "name": null})),
                Some(e) if e.goxlr && e.name != MIC => issues.push(json!({"kind": "capture-loop", "role": role, "name": e.name})),
                Some(e) if !e.goxlr => issues.push(json!({"kind": "capture-other", "role": role, "name": e.name})),
                _ => {}
            }
        }
        for (e, role) in [(&r0, "console"), (&r2, "communications")] {
            match e {
                None => issues.push(json!({"kind": "render-none", "role": role, "name": null})),
                Some(e) if !e.goxlr => issues.push(json!({"kind": "render-other", "role": role, "name": e.name})),
                _ => {}
            }
        }
    }
    let side = |e: &Option<Endpoint>| e.as_ref().map(|e| json!({"name": e.name, "goxlr": e.goxlr})).unwrap_or(json!({"name": null, "goxlr": false}));
    json!({"type": "audio-defaults", "goxlr": goxlr,
           "render": {"console": side(&r0), "communications": side(&r2)},
           "capture": {"console": side(&c0), "communications": side(&c2)}, "issues": issues})
}

#[cfg(windows)]
#[allow(non_snake_case)]
mod win {
    use super::*;
    use windows::core::{interface, BSTR, IUnknown, IUnknown_Vtbl, GUID, HRESULT, PCWSTR};
    use windows::Win32::Devices::FunctionDiscovery::PKEY_Device_FriendlyName;
    use windows::Win32::Media::Audio::{
        eCapture, eCommunications, eMultimedia, eRender, EDataFlow, ERole,
        IMMDevice, IMMDeviceEnumerator, MMDeviceEnumerator, DEVICE_STATE_ACTIVE,
    };
    use windows::Win32::System::Com::{CoCreateInstance, CoInitializeEx, CoTaskMemFree, CoUninitialize, CLSCTX_ALL, COINIT_MULTITHREADED, STGM_READ};
    use winreg::enums::*;
    use winreg::RegKey;

    const BASE: &str = r"SOFTWARE\Microsoft\Windows\CurrentVersion\MMDevices\Audio";
    const DESC: &str = "{a45c254e-df1c-4efd-8020-67d146a850e0},2";
    const IFACE: &str = "{b3f8fa53-0004-438e-9003-51a46e139bfc},6";

    /// Interface non documentee mais stable depuis Windows 7, utilisee par tous les outils de bascule audio.
    #[interface("F8679F50-850A-41CF-9C72-430F290290C8")]
    unsafe trait IPolicyConfig: IUnknown {
        fn GetMixFormat(&self, a: PCWSTR, b: *mut *mut core::ffi::c_void) -> HRESULT;
        fn GetDeviceFormat(&self, a: PCWSTR, b: i32, c: *mut *mut core::ffi::c_void) -> HRESULT;
        fn ResetDeviceFormat(&self, a: PCWSTR) -> HRESULT;
        fn SetDeviceFormat(&self, a: PCWSTR, b: *mut core::ffi::c_void, c: *mut core::ffi::c_void) -> HRESULT;
        fn GetProcessingPeriod(&self, a: PCWSTR, b: i32, c: *mut i64, d: *mut i64) -> HRESULT;
        fn SetProcessingPeriod(&self, a: PCWSTR, b: *mut i64) -> HRESULT;
        fn GetShareMode(&self, a: PCWSTR, b: *mut core::ffi::c_void) -> HRESULT;
        fn SetShareMode(&self, a: PCWSTR, b: *mut core::ffi::c_void) -> HRESULT;
        fn GetPropertyValue(&self, a: PCWSTR, b: *const core::ffi::c_void, c: *mut core::ffi::c_void) -> HRESULT;
        fn SetPropertyValue(&self, a: PCWSTR, b: *const core::ffi::c_void, c: *mut core::ffi::c_void) -> HRESULT;
        fn SetDefaultEndpoint(&self, id: PCWSTR, role: ERole) -> HRESULT;
        fn SetEndpointVisibility(&self, id: PCWSTR, visible: i32) -> HRESULT;
    }
    const CLSID_POLICY_CONFIG: GUID = GUID::from_u128(0x870af99c_171d_4f9e_af0d_e63df40c2bc9);

    pub fn active(capture: bool) -> Vec<Endpoint> {
        let mut out = vec![];
        let hk = RegKey::predef(HKEY_LOCAL_MACHINE);
        let Ok(root) = hk.open_subkey_with_flags(format!(r"{BASE}\{}", if capture { "Capture" } else { "Render" }), KEY_READ | KEY_WOW64_64KEY) else { return out };
        for guid in root.enum_keys().flatten() {
            let Ok(k) = root.open_subkey_with_flags(&guid, KEY_READ | KEY_WOW64_64KEY) else { continue };
            if k.get_value::<u32, _>("DeviceState").unwrap_or(0) != 1 {
                continue;
            }
            let p = root.open_subkey_with_flags(format!(r"{guid}\Properties"), KEY_READ | KEY_WOW64_64KEY).ok();
            let get = |n: &str| p.as_ref().and_then(|p| p.get_value::<String, _>(n).ok()).unwrap_or_default();
            let prefix = if capture { "{0.0.1.00000000}." } else { "{0.0.0.00000000}." };
            let (name, goxlr) = goxlr_channel(&get(IFACE), get(DESC), capture);
            out.push(Endpoint { id: format!("{prefix}{guid}"), name, goxlr });
        }
        out
    }

    fn com<T>(f: impl FnOnce() -> T + Send + 'static) -> Option<T>
    where
        T: Send + 'static,
    {
        // chaque operation dans son fil, COM initialise en multi-thread : aucun conflit avec la fenetre
        std::thread::spawn(move || unsafe {
            let ok = CoInitializeEx(None, COINIT_MULTITHREADED).is_ok();
            let r = f();
            if ok {
                CoUninitialize();
            }
            r
        })
        .join()
        .ok()
    }

    unsafe fn endpoint_id(device: &IMMDevice) -> windows::core::Result<String> {
        let p = device.GetId()?;
        let s = p.to_string();
        CoTaskMemFree(Some(p.0 as *const core::ffi::c_void));
        Ok(s?)
    }

    unsafe fn default_id(e: &IMMDeviceEnumerator, flow: EDataFlow, role: i32) -> Option<String> {
        let d = e.GetDefaultAudioEndpoint(flow, ERole(role)).ok()?;
        endpoint_id(&d).ok()
    }

    unsafe fn inventory_side(e: &IMMDeviceEnumerator, flow: EDataFlow) -> windows::core::Result<Vec<Value>> {
        const ENDPOINT_NOT_FOUND: HRESULT = HRESULT(0x80070490_u32 as i32);
        let default = |role| match e.GetDefaultAudioEndpoint(flow, role) {
            Ok(device) => endpoint_id(&device).map(Some),
            Err(err) if err.code() == ENDPOINT_NOT_FOUND => Ok(None),
            Err(err) => Err(err),
        };
        let multimedia = default(eMultimedia)?;
        let communications = default(eCommunications)?;
        let endpoints = e.EnumAudioEndpoints(flow, DEVICE_STATE_ACTIVE)?;
        let mut rows = Vec::new();
        for i in 0..endpoints.GetCount()? {
            let device = endpoints.Item(i)?;
            let id = endpoint_id(&device)?;
            let name = device.OpenPropertyStore(STGM_READ).ok()
                .and_then(|store| store.GetValue(&PKEY_Device_FriendlyName).ok())
                .and_then(|name| BSTR::try_from(&name).ok())
                .map(|name| name.to_string().trim().to_owned())
                .filter(|name| !name.is_empty());
            rows.push((
                name,
                id.clone(),
                multimedia.as_ref().is_some_and(|default| default.eq_ignore_ascii_case(&id)),
                communications.as_ref().is_some_and(|default| default.eq_ignore_ascii_case(&id)),
            ));
        }
        rows.sort_by_cached_key(|(name, _, multimedia, calls)| (!*multimedia, !*calls, name.as_deref().unwrap_or("").to_lowercase()));
        Ok(rows.into_iter().map(|(name, id, multimedia, calls)| {
            json!({"id":id,"name": name, "default": multimedia, "communications": calls})
        }).collect())
    }

    pub fn inventory() -> windows::core::Result<Value> {
        com(|| unsafe {
            let e: IMMDeviceEnumerator = CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)?;
            Ok(json!({
                "status": "ok",
                "outputs": inventory_side(&e, eRender)?,
                "inputs": inventory_side(&e, eCapture)?,
            }))
        })
        .unwrap_or_else(|| Err(windows::core::Error::empty()))
    }

    pub fn defaults() -> [Option<String>; 4] {
        com(|| unsafe {
            let Ok(e) = CoCreateInstance::<_, IMMDeviceEnumerator>(&MMDeviceEnumerator, None, CLSCTX_ALL) else { return [None, None, None, None] };
            [default_id(&e, eRender, 0), default_id(&e, eRender, 2), default_id(&e, eCapture, 0), default_id(&e, eCapture, 2)]
        })
        .unwrap_or([None, None, None, None])
    }

    /// Sorties actives et identifiants des sorties exposees par le firmware Rustudio en un seul parcours du registre.
    pub fn active_render_with_rustudio() -> (Vec<Endpoint>, Vec<String>) {
        let mut out = vec![];
        let mut rustudio_ids = vec![];
        let hk = RegKey::predef(HKEY_LOCAL_MACHINE);
        let Ok(root) = hk.open_subkey_with_flags(format!(r"{BASE}\Render"), KEY_READ | KEY_WOW64_64KEY) else { return (out, rustudio_ids) };
        for guid in root.enum_keys().flatten() {
            let Ok(k) = root.open_subkey_with_flags(&guid, KEY_READ | KEY_WOW64_64KEY) else { continue };
            if k.get_value::<u32, _>("DeviceState").unwrap_or(0) != 1 {
                continue;
            }
            let p = root.open_subkey_with_flags(format!(r"{guid}\Properties"), KEY_READ | KEY_WOW64_64KEY).ok();
            let get = |n: &str| p.as_ref().and_then(|p| p.get_value::<String, _>(n).ok()).unwrap_or_default();
            let iface = get(IFACE);
            let id = format!("{{0.0.0.00000000}}.{guid}");
            if is_rustudio_output(&iface) {
                rustudio_ids.push(id.clone());
            }
            let (name, goxlr) = goxlr_channel(&iface, get(DESC), false);
            out.push(Endpoint { id, name, goxlr });
        }
        (out, rustudio_ids)
    }

    /// Bascule en 24 bits / 48 kHz toute sortie du firmware Rustudio restee en cache 16 bits dans Windows.
    /// Sans effet si la sortie est deja en 24 bits / 48 kHz.
    pub fn ensure_rustudio_24bit_ids(ids: Vec<String>, force: bool) -> i32 {
        use std::sync::Mutex;
        use std::time::{Duration, Instant};
        static LAST: Mutex<Option<(Instant, Vec<String>)>> = Mutex::new(None);
        if ids.is_empty() {
            return 0;
        }
        if !force {
            let last = LAST.lock().unwrap_or_else(|e| e.into_inner());
            if let Some((at, prev)) = last.as_ref() {
                if *prev == ids && at.elapsed() < Duration::from_secs(30) {
                    return 0;
                }
            }
        }
        let ids_copy = ids.clone();
        let changed = com(move || unsafe {
            let Ok(p) = CoCreateInstance::<_, IPolicyConfig>(&CLSID_POLICY_CONFIG, None, CLSCTX_ALL) else { return 0 };
            let mut dev_fmt = wav_ext_pcm24_48k();
            let mut mix_fmt = wav_ext_f32_48k();
            let mut changed = 0;
            for id in ids {
                let w: Vec<u16> = id.encode_utf16().chain(std::iter::once(0)).collect();
                let mut cur: *mut core::ffi::c_void = core::ptr::null_mut();
                let already_24 = if p.GetDeviceFormat(PCWSTR(w.as_ptr()), 0, &mut cur).is_ok() && !cur.is_null() {
                    let ok = is_pcm24_48k_header(core::slice::from_raw_parts(cur as *const u8, 16));
                    CoTaskMemFree(Some(cur));
                    ok
                } else {
                    false
                };
                if !already_24
                    && p.SetDeviceFormat(
                        PCWSTR(w.as_ptr()),
                        dev_fmt.as_mut_ptr() as *mut core::ffi::c_void,
                        mix_fmt.as_mut_ptr() as *mut core::ffi::c_void,
                    )
                    .is_ok()
                {
                    changed += 1;
                }
            }
            changed
        })
        .unwrap_or(0);
        *LAST.lock().unwrap_or_else(|e| e.into_inner()) = Some((Instant::now(), ids_copy));
        changed
    }

    /// Corrige seulement ce qui est faux : sortie hors GoXLR -> System (Chat pour les communications),
    /// entree autre que Chat Mic -> Chat Mic. Un choix deja sur la GoXLR (Jeu, Musique...) est garde.
    /// Renvoie le nombre de roles changes, -1 si la GoXLR n'expose pas ses voies.
    pub fn fix() -> i32 {
        let (render, rustudio_ids) = active_render_with_rustudio();
        let _ = ensure_rustudio_24bit_ids(rustudio_ids, true);
        let capture = active(true);
        let sound = render.iter().find(|x| x.goxlr && x.name == SOUND).cloned();
        let voice = render.iter().find(|x| x.goxlr && x.name == VOICE).cloned().or(sound.clone());
        let mic = capture.iter().find(|x| x.goxlr && x.name == MIC).cloned();
        let (Some(sound), Some(voice), Some(mic)) = (sound, voice, mic) else { return -1 };
        com(move || unsafe {
            let Ok(e) = CoCreateInstance::<_, IMMDeviceEnumerator>(&MMDeviceEnumerator, None, CLSCTX_ALL) else { return -1 };
            let Ok(p) = CoCreateInstance::<_, IPolicyConfig>(&CLSID_POLICY_CONFIG, None, CLSCTX_ALL) else { return -1 };
            let set = |id: &str, role: i32| -> bool {
                let w: Vec<u16> = id.encode_utf16().chain(std::iter::once(0)).collect();
                p.SetDefaultEndpoint(PCWSTR(w.as_ptr()), ERole(role)).is_ok()
            };
            let mut changed = 0;
            for role in [0, 1, 2] {
                let cur = default_id(&e, eRender, role).and_then(|i| render.iter().find(|x| x.id.eq_ignore_ascii_case(&i)).cloned());
                if !cur.map(|c| c.goxlr).unwrap_or(false) && set(if role == 2 { &voice.id } else { &sound.id }, role) {
                    changed += 1;
                }
                let cur = default_id(&e, eCapture, role).and_then(|i| capture.iter().find(|x| x.id.eq_ignore_ascii_case(&i)).cloned());
                if !cur.map(|c| c.goxlr && c.name == MIC).unwrap_or(false) && set(&mic.id, role) {
                    changed += 1;
                }
            }
            changed
        })
        .unwrap_or(-1)
    }
}

pub fn inventory() -> Value {
    #[cfg(windows)]
    {
        win::inventory().unwrap_or_else(|_| json!({"status": "unavailable"}))
    }
    #[cfg(not(windows))]
    {
        json!({"status": "unsupported"})
    }
}

pub fn status() -> Value {
    #[cfg(windows)]
    {
        let (render, rustudio_ids) = win::active_render_with_rustudio();
        let _ = win::ensure_rustudio_24bit_ids(rustudio_ids, false);
        status_from(&render, &win::active(true), win::defaults())
    }
    #[cfg(not(windows))]
    {
        json!({"type": "audio-defaults", "goxlr": false, "issues": [], "unsupported": true})
    }
}

/// Voies actives (entrees si capture), lues dans le registre de Windows.
pub fn active_endpoints(capture: bool) -> Vec<Endpoint> {
    #[cfg(windows)]
    {
        win::active(capture)
    }
    #[cfg(not(windows))]
    {
        let _ = capture;
        vec![]
    }
}

/// Voie GoXLR où Windows envoie le son par défaut (Game, Sample…), si la sortie par défaut est la GoXLR.
pub fn default_output_channel() -> Option<String> {
    #[cfg(windows)]
    {
        let id = win::defaults()[0].clone()?;
        win::active(false).into_iter().find(|e| e.goxlr && e.id.eq_ignore_ascii_case(&id)).map(|e| e.name)
    }
    #[cfg(not(windows))]
    {
        None
    }
}

/// Identifiant (« {…} ») de l'entrée micro par défaut de Windows, quelle qu'elle soit.
pub fn default_input_guid() -> Option<String> {
    #[cfg(windows)]
    {
        let id = win::defaults()[2].clone()?;
        id.rsplit('.').next().map(|g| g.to_lowercase())
    }
    #[cfg(not(windows))]
    {
        None
    }
}

pub fn fix() -> i32 {
    #[cfg(windows)]
    {
        win::fix()
    }
    #[cfg(not(windows))]
    {
        -1
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn ep(id: &str, name: &str, goxlr: bool) -> Endpoint {
        Endpoint { id: id.into(), name: name.into(), goxlr }
    }
    #[test]
    fn boucle_d_echo_detectee() {
        let render = vec![ep("r1", "System", true), ep("r2", "Chat", true), ep("r3", "Haut-parleurs", false)];
        let capture = vec![ep("c1", "Chat Mic", true), ep("c2", "Broadcast Stream Mix", true)];
        let ok = status_from(&render, &capture, [Some("r1".into()), Some("r2".into()), Some("c1".into()), Some("c1".into())]);
        assert_eq!(ok["issues"], json!([]));
        let bad = status_from(&render, &capture, [Some("r3".into()), Some("r2".into()), Some("c2".into()), Some("c1".into())]);
        let kinds: Vec<&str> = bad["issues"].as_array().unwrap().iter().map(|i| i["kind"].as_str().unwrap()).collect();
        assert_eq!(kinds, vec!["capture-loop", "render-other"]);
    }
    #[test]
    fn voies_du_firmware_rustudio() {
        assert_eq!(goxlr_channel("Rustudio Mini Audio", "Microphone".into(), true), (MIC.to_owned(), true));
        assert_eq!(goxlr_channel("Rustudio Mini Audio", "Haut-parleurs".into(), false), (SOUND.to_owned(), true));
        assert_eq!(goxlr_channel("TC-Helicon GoXLR Mini", "Chat".into(), false), ("Chat".to_owned(), true));
        assert_eq!(goxlr_channel("Realtek USB2.0 Audio", "Haut-parleurs".into(), false), ("Haut-parleurs".to_owned(), false));
        assert_eq!(goxlr_channel("Rustudio Chat", "Haut-parleurs".into(), false), (VOICE.to_owned(), true));
        assert_eq!(goxlr_channel("Rustudio Game", "Haut-parleurs".into(), false), ("Game".to_owned(), true));
        // Une sortie et un micro suffisent : pas de probleme signale.
        let render = vec![ep("r1", SOUND, true)];
        let capture = vec![ep("c1", MIC, true)];
        let ok = status_from(&render, &capture, [Some("r1".into()), Some("r1".into()), Some("c1".into()), Some("c1".into())]);
        assert_eq!(ok["goxlr"], true);
        assert_eq!(ok["issues"], json!([]));
    }
    #[test]
    fn format_24_bits_48khz_detecte() {
        let fmt24 = wav_ext_pcm24_48k();
        assert!(is_pcm24_48k_header(&fmt24));
        let mut fmt16 = fmt24;
        fmt16[14..16].copy_from_slice(&16u16.to_le_bytes());
        assert!(!is_pcm24_48k_header(&fmt16));
        assert!(!is_pcm24_48k_header(&wav_ext_f32_48k()));
        assert!(is_rustudio_output("Rustudio Mini Audio"));
        assert!(is_rustudio_output("Rustudio Sample"));
        assert!(!is_rustudio_output("TC-Helicon GoXLR Mini"));
    }
}
