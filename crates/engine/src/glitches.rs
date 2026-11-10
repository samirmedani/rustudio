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

/// Résume les événements du journal, rendus en XML par Windows.
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

/// Ce que le diagnostic demande au journal Windows.
#[cfg(any(windows, test))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Ask {
    /// Le journal est-il allumé ? Réponse : « enabled: true » ou « enabled: false ».
    State,
    /// Allumer le journal.
    Enable,
    /// Événements des dernières millisecondes demandées, en XML.
    Events { age_ms: u64 },
}

/// Au plus autant d'événements par lecture : le journal n'en garde que quelques-uns en usage normal.
#[cfg(windows)]
const MAX_EVENTS: usize = 5000;

/// Lecture par l'API Windows du journal (wevtapi), dans le processus du moteur : aucun programme lancé.
/// L'ancien appel à wevtutil créait deux à quatre processus par ouverture du diagnostic.
#[cfg(windows)]
fn evtlog(ask: Ask) -> Result<String, String> {
    use windows_sys::Win32::Foundation::{GetLastError, ERROR_INSUFFICIENT_BUFFER, ERROR_NO_MORE_ITEMS};
    use windows_sys::Win32::System::EventLog::*;

    struct Handle(EVT_HANDLE);
    impl Drop for Handle {
        fn drop(&mut self) {
            unsafe { EvtClose(self.0) };
        }
    }
    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }
    fn failed(what: &str) -> String {
        format!("{what} (erreur Windows {})", unsafe { GetLastError() })
    }
    fn config() -> Result<Handle, String> {
        let h = unsafe { EvtOpenChannelConfig(0, wide(CHANNEL).as_ptr(), 0) };
        if h == 0 { Err(failed("Journal Windows des coupures inaccessible")) } else { Ok(Handle(h)) }
    }

    match ask {
        Ask::State => {
            let h = config()?;
            let mut value: EVT_VARIANT = unsafe { std::mem::zeroed() };
            let mut used = 0u32;
            let ok = unsafe {
                EvtGetChannelConfigProperty(h.0, EvtChannelConfigEnabled, 0, std::mem::size_of::<EVT_VARIANT>() as u32, &mut value, &mut used)
            };
            if ok == 0 {
                return Err(failed("État du journal Windows illisible"));
            }
            if value.Type != EvtVarTypeBoolean as u32 {
                return Err("État du journal Windows illisible".into());
            }
            Ok(format!("enabled: {}", unsafe { value.Anonymous.BooleanVal } != 0))
        }
        Ask::Enable => {
            let h = config()?;
            let mut value: EVT_VARIANT = unsafe { std::mem::zeroed() };
            value.Type = EvtVarTypeBoolean as u32;
            value.Anonymous.BooleanVal = 1;
            if unsafe { EvtSetChannelConfigProperty(h.0, EvtChannelConfigEnabled, 0, &value) } == 0 {
                return Err(failed("Activation du journal Windows refusée"));
            }
            if unsafe { EvtSaveChannelConfig(h.0, 0) } == 0 {
                return Err(failed("Activation du journal Windows refusée"));
            }
            Ok(String::new())
        }
        Ask::Events { age_ms } => {
            let query = wide(&format!("*[System[TimeCreated[timediff(@SystemTime) <= {age_ms}]]]"));
            let h = unsafe { EvtQuery(0, wide(CHANNEL).as_ptr(), query.as_ptr(), EvtQueryChannelPath | EvtQueryForwardDirection) };
            if h == 0 {
                return Err(failed("Lecture du journal Windows refusée"));
            }
            let results = Handle(h);
            let mut xml = String::from("<Events>");
            let mut buffer: Vec<u16> = vec![0; 4096];
            let mut count = 0usize;
            'read: loop {
                let mut batch = [0isize; 64];
                let mut returned = 0u32;
                if unsafe { EvtNext(results.0, batch.len() as u32, batch.as_mut_ptr(), 2000, 0, &mut returned) } == 0 {
                    if unsafe { GetLastError() } == ERROR_NO_MORE_ITEMS {
                        break;
                    }
                    return Err(failed("Lecture du journal Windows interrompue"));
                }
                // Chaque événement reçu est fermé, même après une erreur ou la limite.
                let events: Vec<Handle> = batch[..returned as usize].iter().map(|e| Handle(*e)).collect();
                for event in &events {
                    if count >= MAX_EVENTS {
                        break 'read;
                    }
                    let (mut used, mut props) = (0u32, 0u32);
                    let mut render = |buffer: &mut Vec<u16>, used: &mut u32| unsafe {
                        EvtRender(0, event.0, EvtRenderEventXml, (buffer.len() * 2) as u32, buffer.as_mut_ptr().cast(), used, &mut props)
                    };
                    if render(&mut buffer, &mut used) == 0 {
                        if unsafe { GetLastError() } != ERROR_INSUFFICIENT_BUFFER {
                            return Err(failed("Contenu du journal Windows illisible"));
                        }
                        buffer.resize(used as usize / 2 + 1, 0);
                        if render(&mut buffer, &mut used) == 0 {
                            return Err(failed("Contenu du journal Windows illisible"));
                        }
                    }
                    let text = &buffer[..(used as usize / 2).min(buffer.len())];
                    let end = text.iter().position(|c| *c == 0).unwrap_or(text.len());
                    xml.push_str(&String::from_utf16_lossy(&text[..end]));
                    count += 1;
                }
            }
            xml.push_str("</Events>");
            Ok(xml)
        }
    }
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
fn ensure_with(run: &mut impl FnMut(Ask) -> Result<String, String>) -> Result<bool, String> {
    let was_on = enabled(&run(Ask::State)?)?;
    if !was_on {
        run(Ask::Enable)?;
        if !enabled(&run(Ask::State)?)? {
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
fn query_with(since: f64, now: f64, run: &mut impl FnMut(Ask) -> Result<String, String>) -> Value {
    let was_on = match ensure_with(run) {
        Ok(was_on) => was_on,
        Err(e) => return unreadable(since, None, None, e),
    };
    let age_ms = ((now - since).max(1.0) * 1000.0) as u64;
    match run(Ask::Events { age_ms }) {
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
        match ensure_with(&mut evtlog) {
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

/// Au-delà, la mémoire noyau non paginée est anormale : un PC sain reste nettement sous 2 Gio.
/// Mesure du 4 novembre 2026 : à 6,1 Gio (fuite de Windows), le noyau compactait son tas pendant plusieurs
/// millisecondes et retardait le traitement USB, d'où des coupures du son sans défaut de la GoXLR.
pub const KERNEL_NONPAGED_HIGH: u64 = 3 * 1024 * 1024 * 1024;

/// État de la mémoire noyau de Windows pour le diagnostic des coupures.
pub fn kernel(nonpaged_bytes: u64, uptime_s: u64) -> Value {
    json!({"nonpaged_mb": nonpaged_bytes / (1024 * 1024), "uptime_h": uptime_s / 3600,
           "high": nonpaged_bytes >= KERNEL_NONPAGED_HIGH})
}

/// Mémoire noyau lue à l'instant (nul hors de Windows ou si Windows ne répond pas).
pub fn kernel_now() -> Value {
    crate::win::kernel_memory().map(|(bytes, uptime)| kernel(bytes, uptime)).unwrap_or(Value::Null)
}

/// Démonstration : une valeur saine, ou celle de RUSTUDIO_SIM_KERNEL_MB pour afficher l'alerte.
pub fn kernel_simulated() -> Value {
    let mb = std::env::var("RUSTUDIO_SIM_KERNEL_MB").ok().and_then(|v| v.parse::<u64>().ok()).unwrap_or(640);
    kernel(mb * 1024 * 1024, 62 * 3600)
}

pub fn unavailable(since: f64, simulated: bool) -> Value {
    let mut v = summarize("", false, since);
    v["total"] = Value::Null;
    v["readable"] = json!(false);
    v["unsupported"] = json!(true);
    if simulated {
        v["simulated"] = json!(true);
    }
    v
}

/// Coupures notées depuis \`since\` (secondes depuis 1970).
pub fn query(since: f64) -> Value {
    #[cfg(windows)]
    {
        query_with(since, crate::config::now_epoch(), &mut evtlog)
    }
    #[cfg(not(windows))]
    {
        unavailable(since, false)
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
    fn memoire_noyau_signalee_seulement_quand_elle_est_anormale() {
        let gib = 1024 * 1024 * 1024;
        let sain = kernel(700 * 1024 * 1024, 3 * 3600 + 59);
        assert_eq!(sain["nonpaged_mb"], 700);
        assert_eq!(sain["uptime_h"], 3);
        assert_eq!(sain["high"], false);
        assert_eq!(kernel(2 * gib, 0)["high"], false, "2 Gio reste plausible sur un gros PC");
        let fuite = kernel(6 * gib + gib / 10, 80 * 3600);
        assert_eq!(fuite["high"], true);
        assert_eq!(fuite["nonpaged_mb"], 6246);
    }

    #[test]
    fn journal_refuse_ou_illisible_ne_devient_pas_zero_coupure() {
        let v = query_with(100.0, 200.0, &mut |_| Err("accès refusé".into()));
        assert_eq!(v["readable"], false);
        assert!(v["total"].is_null());
        assert_eq!(v["error"], "accès refusé");
        let v = query_with(100.0, 200.0, &mut |args| {
            if args == Ask::State { Ok("enabled: true".into()) } else { Err("lecture refusée".into()) }
        });
        assert_eq!(v["enabled"], true);
        assert_eq!(v["readable"], false);
        assert!(v["total"].is_null());
        assert_eq!(v["error"], "lecture refusée");
        let v = query_with(100.0, 200.0, &mut |args| Ok(if args == Ask::State { "enabled: true" } else { "<Event xmlns='x'><System>" }.into()));
        assert_eq!(v["readable"], false, "XML tronque : pas de faux zero");
        assert!(v["total"].is_null());
    }

    #[test]
    fn activation_confirmee_avant_de_lire_le_journal() {
        let mut calls = Vec::new();
        let v = query_with(100.0, 200.0, &mut |args| {
            calls.push(args);
            Ok(match calls.len() {
                1 => "enabled: false",
                3 => "enabled: true",
                _ => "",
            }.into())
        });
        assert_eq!(calls, [Ask::State, Ask::Enable, Ask::State, Ask::Events { age_ms: 100_000 }]);
        assert_eq!(v["readable"], true);
        assert_eq!(v["was_enabled"], false);
        assert_eq!(v["total"], 0);
        // Un outil qui accepte la commande mais laisse le journal eteint n'a pas active la mesure.
        let v = query_with(100.0, 200.0, &mut |args| {
            assert!(!matches!(args, Ask::Events { .. }), "pas de lecture d'un journal reste eteint");
            Ok("enabled: false".into())
        });
        assert_eq!(v["readable"], false);
        assert!(v["total"].is_null());
        assert!(enabled("propriété inconnue: true").is_err());
        assert!(enabled("<?xml version='1.0'?><channel enabled=\"true\"/>").unwrap());
        assert!(!enabled("<channel enabled='false'/>").unwrap());
        assert!(enabled("<channel enabled='inconnu'/>").is_err());
    }
}
