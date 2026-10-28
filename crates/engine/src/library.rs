//! Bibliotheque d'appareils (liste publiee sur GitHub, copie locale en secours) et verifications anti-echo.
use crate::config::now_epoch;
use crate::controller::Ctl;
use serde_json::{json, Value};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

pub const LOCAL_INDEX: &str = include_str!("../../../library/index.json");
pub const INDEX_URL: &str = "https://raw.githubusercontent.com/samirmedani/rustudio/main/library/index.json";
pub const CONTRIBUTE_URL: &str = "https://github.com/samirmedani/rustudio/blob/main/docs/MODULES.md";
const STATUSES: [&str; 4] = ["official", "community", "generic", "wanted"];

struct Cache {
    remote: Option<Value>,
    fetched_at: Option<f64>,
    error: Option<String>,
}
fn cache() -> &'static Mutex<Cache> {
    static C: OnceLock<Mutex<Cache>> = OnceLock::new();
    C.get_or_init(|| Mutex::new(Cache { remote: None, fetched_at: None, error: None }))
}

/// Garde seulement les fiches valides (une fiche mal formee ne casse pas la liste).
pub fn clean(index: &Value) -> Vec<Value> {
    let s = |m: &Value, k: &str, n: usize| m.get(k).and_then(|v| v.as_str()).map(|x| x.chars().take(n).collect::<String>());
    index
        .get("modules")
        .and_then(|m| m.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|m| {
                    let id = s(m, "id", 64).filter(|i| !i.is_empty() && i.chars().all(|c| c.is_ascii_alphanumeric() || c == '-'))?;
                    let status = s(m, "status", 16).filter(|st| STATUSES.contains(&st.as_str()))?;
                    Some(json!({
                        "id": id, "name": s(m, "name", 80)?, "vendor": s(m, "vendor", 60).unwrap_or_default(),
                        "kind": s(m, "kind", 40).unwrap_or_else(|| "other".into()), "status": status,
                        "summary": s(m, "summary", 400).unwrap_or_default(),
                        "features": m.get("features").and_then(|f| f.as_array()).map(|f| f.iter().filter_map(|x| x.as_str()).take(12).map(|x| x.chars().take(40).collect::<String>()).collect::<Vec<_>>()).unwrap_or_default(),
                        "usb": m.get("usb").cloned().unwrap_or(json!([])),
                        "builtin": m.get("builtin").and_then(|b| b.as_bool()).unwrap_or(false),
                        "source": s(m, "source", 200).unwrap_or_default(),
                    }))
                })
                .collect()
        })
        .unwrap_or_default()
}

pub fn snapshot() -> Value {
    let c = cache().lock().unwrap();
    let local: Value = serde_json::from_str(LOCAL_INDEX).unwrap_or(json!({}));
    let (src, idx) = match &c.remote {
        Some(r) => ("github", r.clone()),
        None => ("local", local),
    };
    json!({"source": src, "modules": clean(&idx), "fetched_at": c.fetched_at, "error": c.error,
           "contribute_url": CONTRIBUTE_URL, "index_url": INDEX_URL})
}

pub fn refresh() -> Value {
    let agent = crate::net::agent(Duration::from_secs(8), 0, &crate::net::ua());
    let r = agent.get(INDEX_URL).call();
    {
        let mut c = cache().lock().unwrap();
        match r {
            Ok(resp) => {
                let mut rd = resp.into_reader();
                match crate::updater::read_limited(&mut rd, 2_000_000).ok().and_then(|b| serde_json::from_slice::<Value>(&b).ok()) {
                    Some(v) if v.get("modules").is_some() => {
                        c.remote = Some(v);
                        c.fetched_at = Some(now_epoch());
                        c.error = None;
                    }
                    _ => c.error = Some("Liste illisible".into()),
                }
            }
            Err(_) => c.error = Some("GitHub injoignable : liste locale".into()),
        }
    }
    snapshot()
}

/// Tout ce qui, dans le routage de la GoXLR, peut faire de l'echo (chez tes amis, dans ton stream ou chez toi).
pub fn echo_check(ctl: &Ctl) -> Value {
    let g = ctl.lock();
    let s = &g.state;
    let mon = (s.volume("MicMonitor") as f64 * 100.0 / 255.0).round() as i32;
    let route = |i: &str, o: &str| json!({"action": "route", "body": {"input": i, "output": o, "enabled": false}});
    let mut checks = vec![];
    let mut add = |id: &str, bad: bool, lvl: &str, t: &str, d: String, fix: Option<(&str, Value)>| {
        let mut c = json!({"id": id, "ok": !bad, "lvl": if bad { lvl } else { "ok" }, "t": t, "d": d});
        if bad {
            if let Some((label, f)) = fix {
                c["fix"] = json!({"label": label, "action": f["action"], "body": f["body"]});
            }
        }
        checks.push(c);
    };
    add("chat-loop", s.route("Chat", "ChatMic"), "warn", "Discord renvoyé vers Discord",
        if s.route("Chat", "ChatMic") { "Tes amis s’entendent eux-mêmes avec un temps de retard.".into() } else { "Tes amis n’entendent pas leur propre voix.".into() },
        Some(("Corriger", route("Chat", "ChatMic"))));
    add("mic-speakers", s.route("Microphone", "LineOut"), "warn", "Ton micro dans les enceintes",
        if s.route("Microphone", "LineOut") { "Avec des enceintes branchées sur la GoXLR, ta voix ressort et revient dans le micro : larsen et écho.".into() } else { "Ta voix ne sort pas sur les enceintes.".into() },
        Some(("Retirer", route("Microphone", "LineOut"))));
    let spk = s.route("Chat", "LineOut") && s.mic.kind == "condenser";
    add("chat-speakers", spk, "tip", "Discord dans les enceintes",
        if spk { "Un micro statique capte les enceintes : tes amis s’entendent. Garde le casque pendant les vocaux.".into() } else { "Rien ne risque de repasser par le micro depuis les enceintes.".into() },
        Some(("Retirer Discord des enceintes", route("Chat", "LineOut"))));
    let hp = s.route("Microphone", "Headphones") && mon > 25;
    add("monitor", hp, "tip", "Retour micro dans le casque",
        if hp { format!("Tu t’entends à {mon} % : avec un casque ouvert, au-delà de 25 % le micro peut le capter.") } else { "Le retour micro est coupé ou bas.".into() },
        Some(("Couper le retour", route("Microphone", "Headphones"))));
    json!({"checks": checks, "mic_type": s.mic.kind})
}
