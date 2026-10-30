//! Catalogue validé livré avec Rustudio et vérifications anti-écho.
use crate::controller::Ctl;
use rustudio_core::modules;
use serde_json::{json, Value};

pub const LOCAL_INDEX: &str = include_str!("../../../library/index.json");
pub const INDEX_URL: &str = "https://github.com/samirmedani/rustudio/blob/main/library/index.json";
pub const CONTRIBUTE_URL: &str = "https://github.com/samirmedani/rustudio/blob/main/docs/COMMUNITY.md";
const STATUSES: [&str; 3] = ["official", "community", "wanted"];

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
                    let module = modules::find(&id);
                    // Un JSON ne peut ni s'attribuer le label officiel, ni valider un module tiers.
                    if status != "wanted" {
                        let registered = module?;
                        if status != registered.origin.as_str() || m["author"] != registered.author
                            || m["source"] != registered.source
                            || m["review_pr"] != json!(registered.review_pr) {
                            return None;
                        }
                        if status == "community" && registered.review_pr.unwrap_or(0) == 0 { return None; }
                    } else if module.is_some() { return None; }
                    Some(json!({
                        "id": id, "name": s(m, "name", 80)?, "vendor": s(m, "vendor", 60).unwrap_or_default(),
                        "kind": s(m, "kind", 40).unwrap_or_else(|| "other".into()), "status": status,
                        "summary": s(m, "summary", 400).unwrap_or_default(),
                        "features": m.get("features").and_then(|f| f.as_array()).map(|f| f.iter().filter_map(|x| x.as_str()).take(12).map(|x| x.chars().take(40).collect::<String>()).collect::<Vec<_>>()).unwrap_or_default(),
                        "usb": m.get("usb").cloned().unwrap_or(json!([])),
                        "builtin": module.is_some(),
                        "author": module.map(|m| m.author), "review_pr": module.and_then(|m| m.review_pr),
                        "source": s(m, "source", 200).unwrap_or_default(),
                    }))
                })
                .collect()
        })
        .unwrap_or_default()
}

pub fn snapshot() -> Value {
    let local: Value = serde_json::from_str(LOCAL_INDEX).unwrap_or(json!({}));
    json!({"source": "release", "modules": clean(&local), "fetched_at": null, "error": null,
           "contribute_url": CONTRIBUTE_URL, "index_url": INDEX_URL})
}

/// Compatible avec les anciennes interfaces : aucune fiche non publiée n'est téléchargée.
pub fn refresh() -> Value { snapshot() }

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalogue_et_registre_compiles_ont_la_meme_provenance() {
        let local: Value = serde_json::from_str(LOCAL_INDEX).unwrap();
        let clean = clean(&local);
        assert_eq!(clean.len(), local["modules"].as_array().unwrap().len(), "aucune fiche livrée ne doit être rejetée");
        for module in modules::installed() {
            let entry = clean.iter().find(|m| m["id"] == module.id).expect("fiche du module livré");
            assert_eq!(entry["status"], module.origin.as_str());
            assert_eq!(entry["author"], module.author);
            assert_eq!(entry["source"], module.source);
            assert_eq!(entry["review_pr"], json!(module.review_pr));
            assert_eq!(entry["builtin"], true);
        }
        assert_eq!(snapshot()["source"], "release");
        assert_eq!(refresh()["modules"], snapshot()["modules"]);
    }

    #[test]
    fn une_fiche_ne_peut_pas_se_declarer_officielle_ou_validee() {
        let local: Value = serde_json::from_str(LOCAL_INDEX).unwrap();
        let original = local["modules"][0].clone();
        for (field, forged) in [("id", json!("module-inconnu")), ("status", json!("community")),
            ("author", json!("autre")), ("source", json!("crates/inconnu")), ("review_pr", json!(1))] {
            let mut entry = original.clone(); entry[field] = forged;
            assert!(clean(&json!({"modules":[entry]})).is_empty(), "fiche falsifiée : {field}");
        }
    }
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
