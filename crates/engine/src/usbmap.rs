//! Arbre USB, pour conseiller un port quand le son coupe entre le PC et la GoXLR.
//! Sur les cartes mères AMD, certains contrôleurs USB sont dans le processeur, d'autres dans le chipset (partagé
//! avec le Wi-Fi, le Bluetooth, l'éclairage…). Une interface audio coupe moins sur un port du processeur, branchée
//! directement, sans hub. On nomme un appareil déjà branché sur le bon contrôleur pour que le port se retrouve
//! sans ouvrir le PC : « à la place de ta webcam ».
use serde_json::{json, Value};
use std::collections::HashMap;

#[derive(Clone, Debug)]
pub struct Node {
    pub id: String,
    pub parent: Option<String>,
    pub name: String,
}

/// Contrôleurs USB intégrés aux processeurs AMD (vrai) ou aux chipsets AMD (faux), d'après l'identifiant PCI.
pub fn cpu_attached(controller_id: &str) -> Option<bool> {
    let id = controller_id.to_uppercase();
    if !id.contains("VEN_1022&DEV_") {
        return None;
    }
    let dev = id.split("DEV_").nth(1).map(|s| &s[..4.min(s.len())]).unwrap_or("");
    match dev {
        "15B6" | "15B7" | "15B8" | "149C" | "1639" | "15E0" | "15E1" => Some(true), // Ryzen 7000/9000, 3000/5000, APU
        "43F7" | "43EE" | "43D5" | "43BB" | "43B9" | "43BA" => Some(false),           // chipsets 300 à 800
        _ => None,
    }
}

fn is_device(id: &str) -> bool {
    let u = id.to_uppercase();
    u.starts_with("USB\\VID_") && !u.contains("&MI_")
}

/// Résumé : contrôleur de la GoXLR, appareils qui le partagent, hub éventuel, port conseillé.
pub fn summarize(nodes: &[Node]) -> Value {
    let by_id: HashMap<String, &Node> = nodes.iter().map(|n| (n.id.to_uppercase(), n)).collect();
    let chain = |id: &str| -> Vec<String> {
        let mut out = vec![];
        let mut cur = by_id.get(&id.to_uppercase()).and_then(|n| n.parent.clone());
        while let Some(p) = cur {
            out.push(p.clone());
            if p.to_uppercase().starts_with("PCI\\") || out.len() > 10 {
                break;
            }
            cur = by_id.get(&p.to_uppercase()).and_then(|n| n.parent.clone());
        }
        out
    };
    let controller = |id: &str| chain(id).into_iter().find(|p| p.to_uppercase().starts_with("PCI\\"));
    let devices: Vec<&Node> = nodes.iter().filter(|n| is_device(&n.id)).collect();
    let Some(gox) = devices.iter().find(|n| n.id.to_uppercase().starts_with("USB\\VID_1220&")) else {
        return json!({"found": false});
    };
    let Some(gctl) = controller(&gox.id) else { return json!({"found": false}) };
    let via_hub = chain(&gox.id).iter().any(|p| is_device(p));
    let mut ctrls: Vec<(String, Vec<String>)> = vec![];
    for d in &devices {
        let Some(c) = controller(&d.id) else { continue };
        let hub = chain(&d.id).iter().any(|p| is_device(p)) || d.name.to_lowercase().contains("hub");
        let name = if hub && d.name.to_lowercase().contains("hub") { None } else { Some(d.name.clone()) };
        match ctrls.iter_mut().find(|(k, _)| k.eq_ignore_ascii_case(&c)) {
            Some((_, v)) => v.extend(name),
            None => ctrls.push((c, name.into_iter().collect())),
        }
    }
    let shared: Vec<String> = ctrls
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case(&gctl))
        .map(|(_, v)| v.iter().filter(|n| **n != gox.name).cloned().collect())
        .unwrap_or_default();
    let here_cpu = cpu_attached(&gctl);
    // port conseillé : un autre contrôleur, du processeur si on sait le reconnaître, le moins chargé, avec un
    // appareil branché dessus pour retrouver le port
    let mut cands: Vec<&(String, Vec<String>)> = ctrls
        .iter()
        .filter(|(k, v)| !k.eq_ignore_ascii_case(&gctl) && !v.is_empty())
        .filter(|(k, _)| here_cpu != Some(true) || cpu_attached(k) == Some(true))
        .collect();
    cands.sort_by_key(|(k, v)| (cpu_attached(k) != Some(true), v.len()));
    let pick = if here_cpu == Some(true) && !via_hub { None } else { cands.first() };
    // l'appareil a deplacer : celui qui supporte le mieux un port partage (webcam, imprimante…), sinon le premier
    let anchor = pick.map(|(_, v)| {
        let rank = |n: &String| {
            let l = n.to_lowercase();
            if l.contains("cam") { 0 } else if l.contains("print") || l.contains("imprim") { 1 } else if l.contains("mouse") || l.contains("souris") { 3 } else { 2 }
        };
        v.iter().min_by_key(|n| rank(n)).cloned().unwrap_or_default()
    });
    json!({
        "found": true, "name": gox.name, "via_hub": via_hub, "controller_cpu": here_cpu, "shared": shared,
        "suggest": pick.map(|(k, v)| json!({"cpu": cpu_attached(k), "anchor": anchor, "devices": v})),
    })
}

/// Lecture de l'arbre des périphériques présents (Windows) ; ailleurs, rien.
pub fn query() -> Value {
    #[cfg(windows)]
    {
        summarize(&win::nodes())
    }
    #[cfg(not(windows))]
    {
        json!({"found": false, "unsupported": true})
    }
}

#[cfg(windows)]
mod win {
    use super::Node;
    use windows_sys::Win32::Devices::DeviceAndDriverInstallation::*;
    use windows_sys::Win32::Devices::Properties::{DEVPKEY_Device_BusReportedDeviceDesc, DEVPKEY_Device_FriendlyName, DEVPKEY_NAME, DEVPROPTYPE};

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }

    unsafe fn prop(inst: u32, key: &windows_sys::Win32::Devices::Properties::DEVPROPKEY) -> Option<String> {
        let mut ty: DEVPROPTYPE = 0;
        let mut size: u32 = 0;
        CM_Get_DevNode_PropertyW(inst, key, &mut ty, std::ptr::null_mut(), &mut size, 0);
        if size < 4 {
            return None;
        }
        let mut buf = vec![0u8; size as usize];
        if CM_Get_DevNode_PropertyW(inst, key, &mut ty, buf.as_mut_ptr(), &mut size, 0) != CR_SUCCESS {
            return None;
        }
        let w: Vec<u16> = buf.chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).take_while(|c| *c != 0).collect();
        let s = String::from_utf16_lossy(&w).trim().to_string();
        (!s.is_empty()).then_some(s)
    }

    unsafe fn id_of(inst: u32) -> Option<String> {
        let mut buf = vec![0u16; 512];
        (CM_Get_Device_IDW(inst, buf.as_mut_ptr(), buf.len() as u32, 0) == CR_SUCCESS)
            .then(|| String::from_utf16_lossy(&buf[..buf.iter().position(|c| *c == 0).unwrap_or(0)]))
    }

    pub fn nodes() -> Vec<Node> {
        let mut out: Vec<Node> = vec![];
        unsafe {
            let usb = wide("USB");
            let flags = CM_GETIDLIST_FILTER_ENUMERATOR | CM_GETIDLIST_FILTER_PRESENT;
            let mut size = 0u32;
            if CM_Get_Device_ID_List_SizeW(&mut size, usb.as_ptr(), flags) != CR_SUCCESS || size == 0 {
                return out;
            }
            let mut buf = vec![0u16; size as usize];
            if CM_Get_Device_ID_ListW(usb.as_ptr(), buf.as_mut_ptr(), size, flags) != CR_SUCCESS {
                return out;
            }
            let ids: Vec<String> = String::from_utf16_lossy(&buf).split('\0').filter(|s| !s.is_empty()).map(String::from).collect();
            let mut todo = ids;
            while let Some(id) = todo.pop() {
                if out.iter().any(|n| n.id.eq_ignore_ascii_case(&id)) {
                    continue;
                }
                let w = wide(&id);
                let mut inst = 0u32;
                if CM_Locate_DevNodeW(&mut inst, w.as_ptr(), CM_LOCATE_DEVNODE_NORMAL) != CR_SUCCESS {
                    continue;
                }
                let mut p = 0u32;
                let parent = (CM_Get_Parent(&mut p, inst, 0) == CR_SUCCESS).then(|| id_of(p)).flatten();
                let name = prop(inst, &DEVPKEY_Device_BusReportedDeviceDesc)
                    .or_else(|| prop(inst, &DEVPKEY_Device_FriendlyName))
                    .or_else(|| prop(inst, &DEVPKEY_NAME))
                    .unwrap_or_default();
                if let Some(par) = &parent {
                    if !par.to_uppercase().starts_with("PCI\\") || !out.iter().any(|n| n.id.eq_ignore_ascii_case(par)) {
                        if !par.to_uppercase().starts_with("HTREE") && out.len() < 2000 {
                            todo.push(par.clone());
                        }
                    }
                }
                out.push(Node { id, parent, name });
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn n(id: &str, parent: &str, name: &str) -> Node {
        Node { id: id.into(), parent: (!parent.is_empty()).then(|| parent.into()), name: name.into() }
    }
    const CPU: &str = "PCI\\VEN_1022&DEV_15B7&SUBSYS_0&REV_00\\4&1";
    const CHIP: &str = "PCI\\VEN_1022&DEV_43F7&SUBSYS_0&REV_01\\6&2";

    fn pc() -> Vec<Node> {
        vec![
            n(CPU, "", "Contrôleur USB du processeur"),
            n(CHIP, "", "Contrôleur USB du chipset"),
            n("USB\\ROOT_HUB30\\A", CPU, "Concentrateur racine"),
            n("USB\\ROOT_HUB30\\B", CHIP, "Concentrateur racine"),
            n("USB\\VID_373E&PID_0047\\1", "USB\\ROOT_HUB30\\A", "R5 Ultra Mouse 2.4G"),
            n("USB\\VID_046D&PID_0892\\2", "USB\\ROOT_HUB30\\A", "HD Pro Webcam C920"),
            n("USB\\VID_1220&PID_8FE4\\3", "USB\\ROOT_HUB30\\B", "GoXLRMini"),
            n("USB\\VID_0B05&PID_19AF\\4", "USB\\ROOT_HUB30\\B", "AURA LED Controller"),
            n("USB\\VID_05E3&PID_0608\\5", "USB\\ROOT_HUB30\\B", "USB2.0 Hub"),
            n("USB\\VID_2516&PID_01D5\\6", "USB\\ROOT_HUB30\\B", "MK770 Hybrid Mechanical Keyboard"),
            n("USB\\VID_2516&PID_01D5&MI_00\\7", "USB\\VID_2516&PID_01D5\\6", "interface"),
        ]
    }

    #[test]
    fn goxlr_sur_le_chipset_vers_le_port_de_la_webcam() {
        let v = summarize(&pc());
        assert_eq!(v["found"], true);
        assert_eq!(v["controller_cpu"], false);
        assert_eq!(v["via_hub"], false);
        assert_eq!(v["shared"], json!(["AURA LED Controller", "MK770 Hybrid Mechanical Keyboard"]));
        assert_eq!(v["suggest"]["cpu"], true);
        assert_eq!(v["suggest"]["anchor"], "HD Pro Webcam C920");
    }

    #[test]
    fn goxlr_deja_sur_le_processeur() {
        let mut nodes = pc();
        nodes.iter_mut().find(|n| n.name == "GoXLRMini").unwrap().parent = Some("USB\\ROOT_HUB30\\A".into());
        let v = summarize(&nodes);
        assert_eq!(v["controller_cpu"], true);
        assert!(v["suggest"].is_null());
    }

    #[test]
    fn goxlr_derriere_un_hub() {
        let mut nodes = pc();
        nodes.iter_mut().find(|n| n.name == "GoXLRMini").unwrap().parent = Some("USB\\VID_05E3&PID_0608\\5".into());
        let v = summarize(&nodes);
        assert_eq!(v["via_hub"], true);
        assert_eq!(v["suggest"]["anchor"], "HD Pro Webcam C920");
    }

    #[test]
    fn sans_goxlr_ou_autre_marque() {
        assert_eq!(summarize(&[])["found"], false);
        assert_eq!(cpu_attached("PCI\\VEN_8086&DEV_7AE0"), None);
        assert_eq!(cpu_attached("PCI\\VEN_1022&DEV_15B6&SUBSYS_1"), Some(true));
    }
}
