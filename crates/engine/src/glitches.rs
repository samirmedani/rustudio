//! Journal Windows des coupures du son (Microsoft-Windows-Audio/GlitchDetection).
//! Windows y note certaines coupures de son, côté programme ou côté pilote, mais le journal est éteint d'usine :
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
fn wevtutil(args: &[&str]) -> Result<String, String> {
    let o = crate::win::run_hidden("wevtutil", args).ok_or("Outil du journal Windows indisponible")?;
    if !o.status.success() {
        let error = String::from_utf8_lossy(&o.stderr).trim().to_string();
        return Err(if error.is_empty() { format!("Journal Windows : {}", o.status) } else { error });
    }
    Ok(String::from_utf8_lossy(&o.stdout).to_string())
}

#[cfg(any(windows, test))]
fn enabled(config: &str) -> Result<bool, String> {
    // XML : les noms d'attributs restent les memes quelle que soit la langue de Windows.
    if let Some((_, rest)) = config.split_once("enabled=") {
        let quote = rest.chars().next().filter(|q| matches!(q, '\'' | '"')).ok_or("État du journal Windows illisible")?;
        let value = rest[1..].split(quote).next().unwrap_or("");
        return match value {
            "true" => Ok(true),
            "false" => Ok(false),
            _ => Err("État du journal Windows illisible".into()),
        };
    }
    for line in config.lines() {
        if let Some((key, value)) = line.trim().split_once(':') {
            if key.eq_ignore_ascii_case("enabled") {
                return match value.trim() {
                    v if v.eq_ignore_ascii_case("true") => Ok(true),
                    v if v.eq_ignore_ascii_case("false") => Ok(false),
                    _ => Err("État du journal Windows illisible".into()),
                };
            }
        }
    }
    Err("État du journal Windows introuvable".into())
}

/// Renvoie son etat initial, uniquement si le journal est effectivement allume a la fin.
#[cfg(any(windows, test))]
fn ensure_with(run: &mut impl FnMut(&[&str]) -> Result<String, String>) -> Result<bool, String> {
    let was_on = enabled(&run(&["gl", CHANNEL, "/f:xml"])?)?;
    if !was_on {
        run(&["sl", CHANNEL, "/e:true"])?;
        if !enabled(&run(&["gl", CHANNEL, "/f:xml"])?)? {
            return Err("Le journal Windows est resté désactivé".into());
        }
    }
    Ok(was_on)
}

#[cfg(any(windows, test))]
fn unreadable(since: f64, on: Option<bool>, was_on: Option<bool>, error: String) -> Value {
    let mut v = summarize("", on.unwrap_or(false), since);
    v["enabled"] = json!(on);
    v["total"] = Value::Null; // mesure absente, pas zero coupure
    v["readable"] = json!(false);
    v["was_enabled"] = json!(was_on);
    v["error"] = json!(error);
    v
}

#[cfg(any(windows, test))]
fn query_with(since: f64, now: f64, run: &mut impl FnMut(&[&str]) -> Result<String, String>) -> Value {
    let was_on = match ensure_with(run) {
        Ok(was_on) => was_on,
        Err(e) => return unreadable(since, None, None, e),
    };
    let age_ms = ((now - since).max(1.0) * 1000.0) as u64;
    let q = format!("/q:*[System[TimeCreated[timediff(@SystemTime) <= {age_ms}]]]");
    match run(&["qe", CHANNEL, &q, "/c:5000", "/f:xml"]) {
        Ok(xml) => {
            let mut v = summarize(&xml, true, since);
            let records = xml.matches("<Event ").count() as u64;
            if records != v["total"].as_u64().unwrap_or(0) || records == 0 && !xml.trim().is_empty() && !xml.contains("<Events") {
                return unreadable(since, Some(true), Some(was_on), "Contenu du journal Windows illisible".into());
            }
            v["readable"] = json!(true);
            v["was_enabled"] = json!(was_on);
            v
        }
        Err(e) => unreadable(since, Some(true), Some(was_on), e),
    }
}

/// Allume le journal s'il est éteint. Renvoie vrai s'il était déjà allumé.
pub fn ensure_enabled() -> bool {
    #[cfg(windows)]
    {
        match ensure_with(&mut wevtutil) {
            Ok(was_on) => {
                if !was_on {
                    crate::log::log("Journal Windows des coupures du son allumé (diagnostic des crépitements)");
                }
                was_on
            }
            Err(e) => {
                crate::logf!("Journal Windows des coupures indisponible : {e}");
                false
            }
        }
    }
    #[cfg(not(windows))]
    false
}

/// Coupures notées depuis \`since\` (secondes depuis 1970).
pub fn query(since: f64) -> Value {
    #[cfg(windows)]
    {
        query_with(since, crate::config::now_epoch(), &mut wevtutil)
    }
    #[cfg(not(windows))]
    {
        let mut v = summarize("", false, since);
        v["total"] = Value::Null;
        v["readable"] = json!(false);
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

    #[test]
    fn journal_refuse_ou_illisible_ne_devient_pas_zero_coupure() {
        let v = query_with(100.0, 200.0, &mut |_| Err("accès refusé".into()));
        assert_eq!(v["readable"], false);
        assert!(v["total"].is_null());
        assert_eq!(v["error"], "accès refusé");
        let v = query_with(100.0, 200.0, &mut |args| {
            if args[0] == "gl" { Ok("enabled: true".into()) } else { Err("lecture refusée".into()) }
        });
        assert_eq!(v["enabled"], true);
        assert_eq!(v["readable"], false);
        assert!(v["total"].is_null());
        assert_eq!(v["error"], "lecture refusée");
        let v = query_with(100.0, 200.0, &mut |args| Ok(if args[0] == "gl" { "enabled: true" } else { "<Event xmlns='x'><System>" }.into()));
        assert_eq!(v["readable"], false, "XML tronque : pas de faux zero");
        assert!(v["total"].is_null());
    }

    #[test]
    fn activation_confirmee_avant_de_lire_le_journal() {
        let mut calls = Vec::new();
        let v = query_with(100.0, 200.0, &mut |args| {
            calls.push(args[0].to_string());
            Ok(match calls.len() {
                1 => "enabled: false",
                3 => "enabled: true",
                _ => "",
            }.into())
        });
        assert_eq!(calls, ["gl", "sl", "gl", "qe"]);
        assert_eq!(v["readable"], true);
        assert_eq!(v["was_enabled"], false);
        assert_eq!(v["total"], 0);
        // Un outil qui accepte la commande mais laisse le journal eteint n'a pas active la mesure.
        let v = query_with(100.0, 200.0, &mut |args| {
            assert_ne!(args[0], "qe", "pas de lecture d'un journal reste eteint");
            Ok("enabled: false".into())
        });
        assert_eq!(v["readable"], false);
        assert!(v["total"].is_null());
        assert!(enabled("propriété inconnue: true").is_err());
        assert_eq!(enabled("<?xml version='1.0'?><channel enabled=\"true\"/>").unwrap(), true);
        assert_eq!(enabled("<channel enabled='false'/>").unwrap(), false);
        assert!(enabled("<channel enabled='inconnu'/>").is_err());
    }
}
