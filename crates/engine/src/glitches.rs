//! Journal Windows des coupures du son (Microsoft-Windows-Audio/GlitchDetection).
//! Windows y note chaque coupure de son, côté programme ou côté pilote, mais le journal est éteint d'usine :
//! le moteur l'allume (journal léger, quelques événements tout au plus) et le résume pour le diagnostic.
use serde_json::{json, Map, Value};

pub const CHANNEL: &str = "Microsoft-Windows-Audio/GlitchDetection";

/// Famille d'une coupure selon l'identifiant d'événement de Windows.
pub fn kind(id: u32) -> &'static str {
    match id {
        29..=35 => "app",     // un programme n'a pas fourni (ou lu) le son à temps
        36..=45 => "driver",  // le pilote ou le périphérique n'a pas suivi
        _ => "other",
    }
}

/// Résume la sortie XML de \`wevtutil qe … /f:xml\`.
pub fn summarize(xml: &str, enabled: bool, since: f64) -> Value {
    let mut ids: Map<String, Value> = Map::new();
    let (mut app, mut driver, mut other) = (0u64, 0u64, 0u64);
    let mut last: Option<String> = None;
    for ev in xml.split("<Event ").skip(1) {
        let id = ev
            .split_once("<EventID")
            .and_then(|(_, r)| r.split_once('>'))
            .and_then(|(_, r)| r.split_once('<'))
            .and_then(|(n, _)| n.trim().parse::<u32>().ok());
        let Some(id) = id else { continue };
        let t = ev
            .split_once("SystemTime=")
            .map(|(_, r)| r.trim_start_matches(['\'', '"']))
            .and_then(|r| r.split(['\'', '"']).next())
            .map(String::from);
        match kind(id) {
            "app" => app += 1,
            "driver" => driver += 1,
            _ => other += 1,
        }
        let n = ids.get(&id.to_string()).and_then(|v| v.as_u64()).unwrap_or(0);
        ids.insert(id.to_string(), json!(n + 1));
        if let Some(t) = t {
            if last.as_deref().map(|l| t.as_str() > l).unwrap_or(true) {
                last = Some(t);
            }
        }
    }
    json!({"enabled": enabled, "since": since, "total": app + driver + other,
           "kinds": {"app": app, "driver": driver, "other": other}, "ids": ids, "last": last})
}

#[cfg(windows)]
fn wevtutil(args: &[&str]) -> Option<String> {
    let o = crate::win::run_hidden("wevtutil", args)?;
    o.status.success().then(|| String::from_utf8_lossy(&o.stdout).to_string())
}

/// Allume le journal s'il est éteint. Renvoie vrai s'il était déjà allumé.
pub fn ensure_enabled() -> bool {
    #[cfg(windows)]
    {
        let on = wevtutil(&["gl", CHANNEL]).map(|t| t.lines().any(|l| l.trim().eq_ignore_ascii_case("enabled: true"))).unwrap_or(false);
        if !on {
            let _ = wevtutil(&["sl", CHANNEL, "/e:true"]);
            crate::log::log("Journal Windows des coupures du son allumé (diagnostic des crépitements)");
        }
        on
    }
    #[cfg(not(windows))]
    false
}

/// Coupures notées depuis \`since\` (secondes depuis 1970).
pub fn query(since: f64) -> Value {
    #[cfg(windows)]
    {
        let was_on = ensure_enabled();
        let age_ms = ((crate::config::now_epoch() - since).max(1.0) * 1000.0) as u64;
        let q = format!("/q:*[System[TimeCreated[timediff(@SystemTime) <= {age_ms}]]]");
        let xml = wevtutil(&["qe", CHANNEL, &q, "/c:5000", "/f:xml"]).unwrap_or_default();
        let mut v = summarize(&xml, true, since);
        v["was_enabled"] = json!(was_on);
        v
    }
    #[cfg(not(windows))]
    {
        let mut v = summarize("", false, since);
        v["unsupported"] = json!(true);
        v
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn resume_du_journal() {
        let xml = "<Event xmlns='x'><System><EventID>41</EventID><TimeCreated SystemTime='2026-10-29T14:10:00.1Z'/></System></Event>\
                   <Event xmlns='x'><System><EventID Qualifiers='0'>33</EventID><TimeCreated SystemTime='2026-10-29T14:12:00.5Z'/></System></Event>\
                   <Event xmlns='x'><System><EventID>41</EventID><TimeCreated SystemTime='2026-10-29T14:11:00Z'/></System></Event>";
        let v = summarize(xml, true, 0.0);
        assert_eq!(v["total"], 3);
        assert_eq!(v["kinds"]["driver"], 2);
        assert_eq!(v["kinds"]["app"], 1);
        assert_eq!(v["ids"]["41"], 2);
        assert_eq!(v["last"], "2026-10-29T14:12:00.5Z");
        assert_eq!(summarize("", true, 0.0)["total"], 0);
    }
}
