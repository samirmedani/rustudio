//! Peripheriques audio par defaut de Windows, lus et corriges depuis la session de l'utilisateur.
//! Le moteur tourne en compte systeme et ne voit pas ces reglages (propres a chaque compte) : c'est l'appli qui
//! les lit (IMMDeviceEnumerator) et les change (IPolicyConfig), comme le panneau Son de Windows.
//! Cause d'echo la plus frequente : Windows enregistre « Broadcast Stream Mix » au lieu de « Chat Mic ».
use serde_json::{json, Value};

pub const MIC: &str = "Chat Mic";
pub const SOUND: &str = "System";
pub const VOICE: &str = "Chat";

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
    use windows::core::{interface, IUnknown, IUnknown_Vtbl, GUID, HRESULT, PCWSTR};
    use windows::Win32::Media::Audio::{eCapture, eRender, EDataFlow, ERole, IMMDeviceEnumerator, MMDeviceEnumerator};
    use windows::Win32::System::Com::{CoCreateInstance, CoInitializeEx, CoTaskMemFree, CoUninitialize, CLSCTX_ALL, COINIT_MULTITHREADED};
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
            out.push(Endpoint { id: format!("{prefix}{guid}"), name: get(DESC), goxlr: get(IFACE).to_lowercase().contains("goxlr") });
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

    unsafe fn default_id(e: &IMMDeviceEnumerator, flow: EDataFlow, role: i32) -> Option<String> {
        let d = e.GetDefaultAudioEndpoint(flow, ERole(role)).ok()?;
        let p = d.GetId().ok()?;
        let s = p.to_string().ok();
        CoTaskMemFree(Some(p.0 as *const core::ffi::c_void));
        s
    }

    pub fn defaults() -> [Option<String>; 4] {
        com(|| unsafe {
            let Ok(e) = CoCreateInstance::<_, IMMDeviceEnumerator>(&MMDeviceEnumerator, None, CLSCTX_ALL) else { return [None, None, None, None] };
            [default_id(&e, eRender, 0), default_id(&e, eRender, 2), default_id(&e, eCapture, 0), default_id(&e, eCapture, 2)]
        })
        .unwrap_or([None, None, None, None])
    }

    /// Corrige seulement ce qui est faux : sortie hors GoXLR -> System (Chat pour les communications),
    /// entree autre que Chat Mic -> Chat Mic. Un choix deja sur la GoXLR (Jeu, Musique...) est garde.
    /// Renvoie le nombre de roles changes, -1 si la GoXLR n'expose pas ses voies.
    pub fn fix() -> i32 {
        let (render, capture) = (active(false), active(true));
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

pub fn status() -> Value {
    #[cfg(windows)]
    {
        status_from(&win::active(false), &win::active(true), win::defaults())
    }
    #[cfg(not(windows))]
    {
        json!({"type": "audio-defaults", "goxlr": false, "issues": [], "unsupported": true})
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
}
