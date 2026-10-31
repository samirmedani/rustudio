//! Aide et retours : le diagnostic a coller dans un ticket public et les adresses GitHub du projet.
//! Le diagnostic donne les versions, Windows, l'etat des appareils, de la connexion, des mises a jour et des coupures
//! USB, et les dernieres lignes du journal ; jamais de numero de serie, nom de PC, nom d'utilisateur, chemin
//! personnel ni adresse IP. Rustudio n'envoie rien lui-meme : l'utilisateur copie, relit et colle.
use crate::config::{now_epoch, VERSION};
use crate::controller::Ctl;
use serde_json::{json, Value};

/// Remplace ce qui identifierait la personne ou sa machine.
const MASK: &str = "[masqué]";

/// Windows de ce PC, comme dans Parametres > Systeme > Informations : « Windows 11 25H2 (build 26200.7019) ».
pub fn windows_version() -> String {
    #[cfg(windows)]
    {
        use winreg::enums::*;
        if let Ok(k) = winreg::RegKey::predef(HKEY_LOCAL_MACHINE).open_subkey(r"SOFTWARE\Microsoft\Windows NT\CurrentVersion") {
            let build: String = k.get_value("CurrentBuild").unwrap_or_default();
            let ubr: u32 = k.get_value("UBR").unwrap_or(0);
            let display: String = k.get_value("DisplayVersion").unwrap_or_default();
            return windows_label(&build, ubr, &display);
        }
    }
    format!("hors Windows ({})", std::env::consts::OS)
}

/// Le registre dit « Windows 10 » meme sous Windows 11 : le numero de build tranche (22000 et plus : Windows 11).
pub fn windows_label(build: &str, ubr: u32, display: &str) -> String {
    let n: u32 = build.trim().parse().unwrap_or(0);
    let name = if n >= 22000 { "Windows 11" } else if n > 0 { "Windows 10" } else { "Windows" };
    let display = display.trim();
    let mut s = name.to_string();
    if !display.is_empty() {
        s.push(' ');
        s.push_str(display);
    }
    if n > 0 {
        s.push_str(&format!(" (build {n}.{ubr})"));
    }
    s
}

fn replace_ci(text: &str, needle: &str, with: &str) -> String {
    if needle.len() < 3 {
        return text.to_string();
    }
    let (lower, low_needle) = (text.to_lowercase(), needle.to_lowercase());
    if lower.len() != text.len() {
        return text.replace(needle, with);
    }
    let mut out = String::new();
    let mut i = 0;
    while let Some(j) = lower[i..].find(&low_needle) {
        out.push_str(&text[i..i + j]);
        out.push_str(with);
        i += j + needle.len();
    }
    out.push_str(&text[i..]);
    out
}

fn is_delim(c: char) -> bool {
    c.is_whitespace() || matches!(c, '"' | '\'' | ',' | ';' | '|' | ')' | '(' | '[' | ']' | '{' | '}' | '<' | '>')
}

/// Masque le segment qui suit un prefixe (nom d'utilisateur apres « \Users\ », numero de serie apres « série »)
/// jusqu'au separateur suivant. spaces : un espace termine le segment (faux pour un nom d'utilisateur, qui peut
/// en contenir).
fn mask_after(text: &str, prefix: &str, stop: &[char], spaces: bool) -> String {
    let lower = text.to_lowercase();
    if lower.len() != text.len() {
        return text.to_string();
    }
    let low_prefix = prefix.to_lowercase();
    let mut out = String::new();
    let mut i = 0;
    while let Some(j) = lower[i..].find(&low_prefix) {
        let start = i + j + prefix.len();
        out.push_str(&text[i..start]);
        let rest = &text[start..];
        let end = rest
            .find(|c: char| stop.contains(&c) || if spaces { is_delim(c) } else { matches!(c, '"' | '\'' | ',' | ';' | '|' | '\n' | '\r') })
            .unwrap_or(rest.len());
        if end > 0 {
            out.push('…');
        }
        i = start + end;
    }
    out.push_str(&text[i..]);
    out
}

/// Adresses IPv4 hors boucle locale (127.x) remplacees. Quatre nombres de 0 a 255 dont le premier vaut au moins
/// 10 : « firmware 1.2.1.49 » ou « pilote 5.57.0.0 » restent lisibles.
fn mask_ipv4(text: &str) -> String {
    let b = text.as_bytes();
    let mut out = String::new();
    let mut i = 0;
    while i < b.len() {
        if b[i].is_ascii_digit() && (i == 0 || !(b[i - 1].is_ascii_digit() || b[i - 1] == b'.')) {
            let mut j = i;
            let mut groups = 0;
            let mut ok = true;
            loop {
                let s = j;
                while j < b.len() && b[j].is_ascii_digit() {
                    j += 1;
                }
                if j - s == 0 || j - s > 3 {
                    ok = false;
                    break;
                }
                groups += 1;
                if groups == 4 || j >= b.len() || b[j] != b'.' {
                    break;
                }
                j += 1;
            }
            let end_ok = j >= b.len() || !(b[j].is_ascii_digit() || b[j] == b'.' && b.get(j + 1).map(|c| c.is_ascii_digit()).unwrap_or(false));
            if ok && groups == 4 && end_ok {
                let ip = &text[i..j];
                let nums: Vec<u32> = ip.split('.').filter_map(|n| n.parse().ok()).collect();
                let address = nums.len() == 4 && nums.iter().all(|n| *n <= 255) && nums[0] >= 10 && nums[0] != 127;
                out.push_str(if address { "x.x.x.x" } else { ip });
                i = j;
                continue;
            }
            out.push_str(&text[i..j.max(i + 1)]);
            i = j.max(i + 1);
            continue;
        }
        let ch = text[i..].chars().next().unwrap();
        out.push(ch);
        i += ch.len_utf8();
    }
    out
}

/// Ce qui identifierait la personne ou le PC, retire d'une ligne de journal ou d'un message d'erreur.
pub fn anonymize(text: &str, secrets: &[String]) -> String {
    let mut s = text.to_string();
    for secret in secrets.iter().map(|x| x.trim()).filter(|x| x.len() >= 3) {
        s = replace_ci(&s, secret, MASK);
    }
    for prefix in [r"\Users\", "/Users/", "/home/"] {
        s = mask_after(&s, prefix, &['\\', '/'], false);
    }
    for word in ["série ", "serial ", "numéro de série "] {
        s = mask_after(&s, word, &[], true);
    }
    s = mask_vid_pid_instance(&s);
    mask_ipv4(&s)
}

/// « USB\VID_1220&PID_8FE4\8&1dc14b6f&0&10 » -> « USB\VID_1220&PID_8FE4\… » (le numero d'instance peut porter
/// un numero de serie ; le modele reste).
fn mask_vid_pid_instance(text: &str) -> String {
    let mut out = String::new();
    let mut rest = text;
    while let Some(j) = rest.to_ascii_uppercase().find("PID_") {
        let after = j + 4;
        let hex = rest[after..].chars().take(4).filter(|c| c.is_ascii_hexdigit()).count();
        let mut k = after + hex;
        while rest[k..].starts_with('&') {
            let seg = rest[k + 1..].find(|c: char| c == '&' || c == '\\' || is_delim(c)).map(|e| k + 1 + e).unwrap_or(rest.len());
            if !rest[k + 1..seg].to_ascii_uppercase().starts_with("MI_") && !rest[k + 1..seg].to_ascii_uppercase().starts_with("REV_") {
                break;
            }
            k = seg;
        }
        out.push_str(&rest[..k]);
        rest = &rest[k..];
        if hex == 4 && rest.starts_with('\\') {
            let end = rest[1..].find(is_delim).map(|e| e + 1).unwrap_or(rest.len());
            if end > 1 {
                out.push_str("\\…");
                rest = &rest[end..];
            }
        }
    }
    out.push_str(rest);
    out
}

/// Encodage d'un parametre d'adresse (lettres, chiffres et « -._~ » gardes).
pub fn url_param(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => (b as char).to_string(),
            _ => format!("%{b:02X}"),
        })
        .collect()
}

/// Formulaire de bug prerempli : version de Rustudio et de Windows (champs « version » et « windows » de bug.yml).
pub fn bug_url(repo: &str, version: &str, windows: &str) -> String {
    format!("https://github.com/{repo}/issues/new?template=bug.yml&version={}&windows={}", url_param(version), url_param(windows))
}

/// Ce qui identifie ce PC et ses appareils, a retirer de tout texte du diagnostic.
fn secrets(ctl: &Ctl) -> Vec<String> {
    let mut v: Vec<String> = vec![];
    if let Some(s) = ctl.dev_info.lock().unwrap().serial.clone() {
        v.push(s);
    }
    for k in ["COMPUTERNAME", "USERNAME", "USERDOMAIN"] {
        if let Ok(x) = std::env::var(k) {
            let x = x.trim_end_matches('$').to_string();
            if !matches!(x.to_uppercase().as_str(), "SYSTEM" | "NT AUTHORITY" | "AUTORITE NT" | "WORKGROUP") {
                v.push(x);
            }
        }
    }
    v.sort_by_key(|s| std::cmp::Reverse(s.len()));
    v
}

/// GET /api/support : le diagnostic anonyme et les adresses d'aide du projet.
pub fn support(ctl: &Ctl) -> Value {
    let secrets = secrets(ctl);
    let anon = |s: &str| anonymize(s, &secrets);
    let h = ctl.health();
    let windows = windows_version();
    let repo = ctl.updater.ucfg.lock().unwrap().repo.clone();
    let u = ctl.updater.brief();
    let devices: Vec<Value> = ctl.modules_snapshot()["devices"]
        .as_array()
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .map(|d| json!({"module": d["id"], "etat": d["status"], "connecte": d["connected"], "simule": d["simulated"],
            "firmware": d["firmware"], "erreur": d["error"].as_str().map(|e| anon(e))}))
        .collect();
    let drops = &h["usb_drops"];
    let stats = &h["stats"];
    let report = json!({
        "rustudio": VERSION,
        "compilation": if ctl.updater.release_build { "release" } else { "développement" },
        "windows": windows,
        "moteur": {"etat": h["status"], "lance_depuis_s": h["engine_uptime_s"], "relances": h["engine_restarts"],
                   "surveille": h["supervised"], "simule": h["simulated"]},
        "appareils": devices,
        "goxlr_mini": {"connectee": h["connected"], "presente": h["present"], "firmware": h["firmware"],
                        "connectee_depuis_s": h["device_uptime_s"], "reconnexions": h["reconnects"], "port_usb": h["usb"],
                        "commandes": stats["commands"], "resynchros": stats["resyncs"], "remises_a_zero": stats["index_resets"],
                        "boucle_usb_ms": h["loop_age_ms"], "derniere_erreur": h["last_error"].as_str().map(|e| anon(e))},
        "mise_a_jour": u,
        "coupures_usb_24h": {"nombre": drops["count"], "perdu_casque_ms": drops["out_ms"], "perdu_pc_ms": drops["in_ms"],
                             "flux_actif": drops["streaming"],
                             "programmes_frequents": drops["top"].as_array().map(|t| t.iter().map(|x| json!({"nom": x["name"], "coupures": x["count"]})).collect::<Vec<_>>())},
        "erreurs": h["errors"].as_array().map(|a| a.iter().rev().take(10).map(|e| anon(e["msg"].as_str().unwrap_or(""))).collect::<Vec<_>>()),
        "journal": h["log"].as_array().map(|a| a.iter().map(|l| anon(l.as_str().unwrap_or(""))).collect::<Vec<_>>()),
        "genere_le": chrono::Local::now().format("%Y-%m-%d %H:%M").to_string(),
    });
    let _ = now_epoch();
    json!({
        "report": report,
        "version": VERSION,
        "windows": windows,
        "repo": repo,
        "bug_url": bug_url(&repo, VERSION, &windows),
        "device_url": format!("https://github.com/{repo}/issues/new?template=appareil.yml"),
        "ideas_url": format!("https://github.com/{repo}/discussions/categories/ideas"),
        "questions_url": format!("https://github.com/{repo}/discussions/categories/q-a"),
        "issues_url": format!("https://github.com/{repo}/issues"),
        "releases_url": format!("https://github.com/{repo}/releases"),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rien_qui_identifie_le_pc_ou_l_appareil() {
        // Valeurs inventées ; chemins et adresses assemblés à l'exécution pour ne ressembler à aucune donnée réelle.
        let secrets = vec!["SNTEST0000XX1".to_string(), "PC-EXEMPLE".to_string()];
        let line = r"2026-10-31 14:39:35 GoXLR connectée : série SNTEST0000XX1, firmware 1.2.1.49 sur pc-exemple";
        let a = anonymize(line, &secrets);
        assert!(!a.contains("SNTEST0000XX1") && !a.to_lowercase().contains("pc-exemple"), "{a}");
        assert!(a.contains("firmware 1.2.1.49"), "le reste du message est garde : {a}");
        let users = "Users";
        let line = format!(r"Lecture C:\{users}\Alex Martin\AppData\x.json impossible ; /{users}/alex/Library ok");
        let p = anonymize(&line, &[]);
        assert_eq!(p, format!(r"Lecture C:\{users}\…\AppData\x.json impossible ; /{users}/…/Library ok"));
        let usb = anonymize(r"USB\VID_1220&PID_8FE4\7&aaaa1111&0&1 ; USB\VID_046D&PID_C52B&MI_00\6&bbbb2222&0&0000", &[]);
        assert_eq!(usb, r"USB\VID_1220&PID_8FE4\… ; USB\VID_046D&PID_C52B&MI_00\…");
        let lan = |a: u8, b: u8, c: u8, d: u8| format!("{a}.{b}.{c}.{d}");
        let ip = anonymize(&format!("moteur 127.0.0.1:14565, routeur {}, firmware 1.2.1.49 build 26200.7019", lan(192, 168, 0, 20)), &[]);
        assert_eq!(ip, "moteur 127.0.0.1:14565, routeur x.x.x.x, firmware 1.2.1.49 build 26200.7019");
        assert_eq!(anonymize(&format!("lan {}, pilote 5.57.0.0, Windows 10.0.26200.7019", lan(10, 1, 2, 3)), &[]), "lan x.x.x.x, pilote 5.57.0.0, Windows 10.0.26200.7019");
        assert_eq!(anonymize("série SNTEST inconnue", &[]), "série … inconnue");
    }

    #[test]
    fn windows_lisible() {
        assert_eq!(windows_label("26200", 7019, "25H2"), "Windows 11 25H2 (build 26200.7019)");
        assert_eq!(windows_label("19045", 6456, "22H2"), "Windows 10 22H2 (build 19045.6456)");
        assert_eq!(windows_label("", 0, ""), "Windows");
    }

    #[test]
    fn formulaire_de_bug_prerempli() {
        let u = bug_url("samirmedani/rustudio", "5.6.12", "Windows 11 25H2 (build 26200.7019)");
        assert_eq!(u, "https://github.com/samirmedani/rustudio/issues/new?template=bug.yml&version=5.6.12&windows=Windows%2011%2025H2%20%28build%2026200.7019%29");
    }
}
