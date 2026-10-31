//! Coupures du son entre le PC et la GoXLR, comptees par le pilote lui-meme (voir rustudio_goxlr::stream).
//! La boucle USB relit ses compteurs chaque seconde : aucun son de test, rien d'envoye a l'appareil. A chaque
//! coupure, le moteur note l'heure, le son perdu et les programmes que Windows venait de lancer (une rafale de
//! lancements suffit a faire sauter le son USB : c'est ainsi qu'on a trouve la synchro de projets du 29/10).
use rustudio_goxlr::stream::{StreamCounters, StreamStats};
use serde_json::{json, Value};
use std::collections::{BTreeMap, VecDeque};
use std::path::PathBuf;

/// Coupures gardees (l'historique survit aux redemarrages du moteur et du PC) et duree presentee.
const KEEP: usize = 300;
const DAY: f64 = 86_400.0;

#[derive(Debug, Clone, PartialEq)]
pub struct DropEvent {
    pub t: f64,
    /// son perdu vers la GoXLR (casque, enceintes) et vers le PC (micro, Stream Mix), en millisecondes
    pub out_ms: f64,
    pub in_ms: f64,
    /// programmes lances juste avant ou pendant (None : pas encore regarde)
    pub hint: Option<Vec<(String, u32)>>,
}

#[derive(Default)]
pub struct UsbDrops {
    prev: Option<StreamStats>,
    /// None : jamais lu ; Some(false) : pilote sans ces compteurs
    pub available: Option<bool>,
    pub since: Option<f64>,
    pub reading: bool,
    pub streaming: bool,
    pub events: VecDeque<DropEvent>,
    pub total: u64,
    launched: VecDeque<(f64, Vec<String>)>,
    last_log: f64,
    unlogged: u32,
    pending_logs: VecDeque<DropEvent>,
    /// fichier de l'historique ; None : rien sur le disque (tests)
    path: Option<PathBuf>,
    revision: u64,
    saved_revision: u64,
}

/// Copie a ecrire par le fil de diagnostic, sans garder le verrou des compteurs USB.
pub struct HistoryWrite {
    path: PathBuf,
    revision: u64,
    since: Option<f64>,
    total: u64,
    events: VecDeque<DropEvent>,
}

impl HistoryWrite {
    pub fn write(&self) -> std::io::Result<()> {
        let events: Vec<Value> = self.events.iter().map(|e| json!({
            "t": e.t, "out_ms": e.out_ms, "in_ms": e.in_ms,
            "hint": e.hint.as_ref().map(|h| h.iter().map(|(n, c)| json!({"name": n, "n": c})).collect::<Vec<_>>()),
        })).collect();
        let v = json!({"since": self.since, "total": self.total, "events": events});
        let tmp = self.path.with_extension("json.tmp");
        std::fs::write(&tmp, v.to_string())?;
        std::fs::rename(&tmp, &self.path)
    }
}

fn lost(a: &StreamCounters, b: &StreamCounters) -> u32 {
    // flux en marche aux deux releves, sans redemarrage entre les deux
    if a.active && b.active && b.ms >= a.ms && b.lost >= a.lost {
        b.lost - a.lost
    } else {
        0
    }
}

impl UsbDrops {
    /// Historique enregistre (fichier absent ou illisible : on repart de zero).
    pub fn load(path: PathBuf, now: f64) -> UsbDrops {
        let mut d = UsbDrops { path: Some(path.clone()), ..Default::default() };
        let Ok(v) = std::fs::read_to_string(&path).map_err(|e| e.to_string()).and_then(|t| serde_json::from_str::<Value>(&t).map_err(|e| e.to_string())) else {
            return d;
        };
        d.since = v["since"].as_f64();
        d.total = v["total"].as_u64().unwrap_or(0);
        for e in v["events"].as_array().into_iter().flatten() {
            let t = e["t"].as_f64().unwrap_or(0.0);
            if t < now - DAY {
                d.revision += 1;
                continue;
            }
            let hint = e["hint"].as_array().map(|a| {
                // Les repères internes d'anciennes versions (« @… », veille des écrans) ne sont pas des programmes.
                a.iter()
                    .filter_map(|h| Some((h["name"].as_str()?.to_string(), u32::try_from(h["n"].as_u64()?).ok()?)))
                    .filter(|(n, _)| !n.starts_with('@'))
                    .collect::<Vec<_>>()
            });
            d.events.push_back(DropEvent { t, out_ms: e["out_ms"].as_f64().unwrap_or(0.0), in_ms: e["in_ms"].as_f64().unwrap_or(0.0), hint });
        }
        while d.events.len() > KEEP {
            d.events.pop_front();
            d.revision += 1;
        }
        d
    }

    /// Prepare une sauvegarde, sans acces disque. Un seul fil doit ecrire ces copies, dans l'ordre.
    pub fn pending_history(&mut self, now: f64) -> Option<HistoryWrite> {
        self.prune(now);
        if self.revision == self.saved_revision {
            return None;
        }
        Some(HistoryWrite { path: self.path.clone()?, revision: self.revision, since: self.since, total: self.total, events: self.events.clone() })
    }

    /// A appeler seulement apres une ecriture reussie. Une coupure arrivee entretemps reste a sauvegarder.
    pub fn history_saved(&mut self, saved: &HistoryWrite) {
        self.saved_revision = saved.revision;
    }

    fn prune(&mut self, now: f64) {
        while self.events.front().map(|e| e.t < now - DAY).unwrap_or(false) {
            self.events.pop_front();
            self.revision += 1;
        }
    }

    /// Nouveau releve (None : compteurs illisibles). Renvoie la coupure depuis le releve precedent.
    pub fn update(&mut self, s: Option<StreamStats>, now: f64) -> Option<DropEvent> {
        let Some(s) = s else {
            if self.available.is_none() {
                self.available = Some(false);
            }
            self.reset();
            return None;
        };
        self.available = Some(true);
        self.reading = true;
        if self.since.is_none() {
            self.since = Some(now);
            self.revision += 1;
        }
        self.streaming = s.active();
        let prev = self.prev.replace(s)?;
        let (o, i) = (lost(&prev.output, &s.output), lost(&prev.input, &s.input));
        if o == 0 && i == 0 {
            return None;
        }
        let ev = DropEvent { t: now, out_ms: o as f64 / 8.0, in_ms: i as f64 / 8.0, hint: None };
        self.total += 1;
        if self.events.len() >= KEEP {
            self.events.pop_front();
        }
        self.prune(now);
        self.events.push_back(ev.clone());
        if self.pending_logs.len() >= KEEP {
            self.pending_logs.pop_front();
        }
        self.pending_logs.push_back(ev.clone());
        self.revision += 1;
        Some(ev)
    }

    /// GoXLR deconnectee : les compteurs repartent de zero a la reconnexion.
    pub fn reset(&mut self) {
        self.prev = None;
        self.reading = false;
        self.streaming = false;
    }

    /// Une ligne de journal par coupure, au plus une toutes les 10 s (une rafale n'inonde pas le journal).
    pub fn log_line(&mut self, ev: &DropEvent) -> Option<String> {
        if ev.t - self.last_log < 10.0 {
            self.unlogged += 1;
            return None;
        }
        let more = std::mem::take(&mut self.unlogged);
        self.last_log = ev.t;
        Some(format!(
            "Coupure du son entre le PC et la GoXLR : {} ms perdues vers la GoXLR, {} ms vers le PC{}",
            ev.out_ms, ev.in_ms,
            if more > 0 { format!(" ({more} autre(s) juste avant)") } else { String::new() }
        ))
    }

    /// Prepare les messages pour le fil de diagnostic : le fil USB n'ecrit pas le journal non plus.
    pub fn take_log_lines(&mut self) -> Vec<String> {
        let mut lines = Vec::new();
        while let Some(ev) = self.pending_logs.pop_front() {
            if let Some(line) = self.log_line(&ev) {
                lines.push(line);
            }
        }
        lines
    }

    /// Programmes apparus depuis le coup d'oeil precedent ; complete les coupures assez anciennes pour avoir vu
    /// les lancements d'avant et d'apres.
    pub fn note_launched(&mut self, now: f64, names: Vec<String>) {
        if !names.is_empty() {
            self.launched.push_back((now, names));
        }
        while self.launched.front().map(|(t, _)| now - t > 15.0).unwrap_or(false) {
            self.launched.pop_front();
        }
        let launched = &self.launched;
        for ev in self.events.iter_mut().rev().take(30) {
            if ev.hint.is_some() || now - ev.t < 1.5 {
                continue;
            }
            let mut n: BTreeMap<&str, u32> = BTreeMap::new();
            for (t, names) in launched {
                if *t >= ev.t - 2.5 && *t <= ev.t + 1.5 {
                    for x in names {
                        *n.entry(x.as_str()).or_default() += 1;
                    }
                }
            }
            let mut v: Vec<(String, u32)> = n.into_iter().map(|(k, c)| (k.to_string(), c)).collect();
            v.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
            v.truncate(5);
            ev.hint = Some(v);
            self.revision += 1;
        }
    }

    pub fn json(&self, since: f64) -> Value {
        let evs: Vec<&DropEvent> = self.events.iter().filter(|e| e.t >= since).collect();
        let sum = |f: fn(&DropEvent) -> f64| (evs.iter().map(|e| f(e)).sum::<f64>() * 1000.0).round() / 1000.0 + 0.0; // + 0.0 : pas de -0
        // programmes revenus le plus souvent au moment des coupures (une fois par coupure)
        let mut top: BTreeMap<&str, u32> = BTreeMap::new();
        for e in &evs {
            for (n, _) in e.hint.iter().flatten() {
                *top.entry(n.as_str()).or_default() += 1;
            }
        }
        let mut top: Vec<(String, u32)> = top.into_iter().map(|(k, c)| (k.to_string(), c)).collect();
        top.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
        top.truncate(5);
        json!({
            "available": self.available, "reading": self.reading, "since": self.since, "streaming": self.streaming, "total": self.total,
            "top": top.iter().map(|(n, c)| json!({"name": n, "count": c})).collect::<Vec<_>>(),
            "count": evs.len(), "out_ms": sum(|e| e.out_ms), "in_ms": sum(|e| e.in_ms), "last": evs.last().map(|e| e.t),
            "events": evs.iter().rev().take(20).map(|e| json!({
                "t": e.t, "out_ms": e.out_ms, "in_ms": e.in_ms,
                "hint": e.hint.as_ref().map(|h| h.iter().map(|(n, c)| json!({"name": n, "n": c})).collect::<Vec<_>>()),
            })).collect::<Vec<_>>(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn save(d: &mut UsbDrops, now: f64) {
        let pending = d.pending_history(now).expect("sauvegarde attendue");
        pending.write().unwrap();
        d.history_saved(&pending);
    }

    fn st(active: bool, ms: u32, lost: u32) -> StreamStats {
        let c = StreamCounters { active, ms, microframes: ms * 8, lost };
        StreamStats { input: c, output: c }
    }

    #[test]
    fn coupures_comptees_seulement_flux_en_marche() {
        let mut d = UsbDrops::default();
        assert!(d.update(Some(st(true, 1000, 1640)), 100.0).is_none(), "premier releve : reference");
        assert!(d.update(Some(st(true, 2000, 1640)), 101.0).is_none());
        let ev = d.update(Some(st(true, 3000, 1648)), 102.0).unwrap();
        assert_eq!((ev.out_ms, ev.in_ms, ev.t), (1.0, 1.0, 102.0));
        // arret du flux : le pilote ajoute des micro-trames, ce n'est pas une coupure
        assert!(d.update(Some(st(false, 3100, 1690)), 103.0).is_none());
        // redemarrage : nouvelle reference
        assert!(d.update(Some(st(true, 10, 1690)), 104.0).is_none());
        assert!(d.update(Some(st(true, 1010, 1706)), 105.0).is_some());
        // deconnexion puis compteurs remis a zero par le pilote
        d.reset();
        assert!(d.update(Some(st(true, 50, 0)), 200.0).is_none());
        assert_eq!(d.total, 2);
        assert_eq!(d.available, Some(true));
        let j = d.json(0.0);
        assert_eq!(j["count"], 2);
        assert_eq!(j["out_ms"], 3.0);
        assert_eq!(j["events"][0]["t"], 105.0, "la plus recente d'abord");
        assert_eq!(d.json(104.5)["count"], 1);
    }

    #[test]
    fn pilote_sans_compteurs() {
        let mut d = UsbDrops::default();
        d.update(None, 1.0);
        assert_eq!(d.available, Some(false));
        assert!(!d.reading);
        d.update(Some(st(true, 1, 0)), 2.0);
        assert!(d.reading);
        d.update(None, 3.0);
        assert!(!d.reading);
        assert_eq!(d.available, Some(true), "une GoXLR debranchee ne rend pas le diagnostic indisponible");
    }

    #[test]
    fn programmes_lances_autour_de_la_coupure() {
        let mut d = UsbDrops::default();
        d.update(Some(st(true, 1000, 0)), 100.0);
        d.note_launched(99.0, vec!["git.exe".into(), "conhost.exe".into(), "git.exe".into()]);
        d.update(Some(st(true, 2000, 8)), 101.0);
        d.note_launched(101.2, vec!["git.exe".into()]);
        assert!(d.events[0].hint.is_none(), "trop tot : les lancements d'apres ne sont pas encore vus");
        d.note_launched(102.6, vec![]);
        let h = d.events[0].hint.clone().unwrap();
        assert_eq!(h[0], ("git.exe".to_string(), 3));
        assert_eq!(h[1], ("conhost.exe".to_string(), 1));
        d.note_launched(130.0, vec![]);
        assert!(d.launched.is_empty(), "les vieux lancements sont oublies");
    }

    #[test]
    fn anciens_reperes_de_veille_ignores_au_chargement() {
        let dir = std::env::temp_dir().join(format!("rustudio-drops-reperes-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("usb-drops.json");
        let old = json!({"since": 900.0, "total": 1, "events": [{"t": 1000.0, "out_ms": 2.0, "in_ms": 2.0,
            "hint": [{"name": "@ecrans-eteints", "n": 1}, {"name": "svchost.exe", "n": 1}]}]});
        std::fs::write(&path, old.to_string()).unwrap();
        let d = UsbDrops::load(path, 1001.0);
        assert_eq!(d.events[0].hint.as_ref().unwrap(), &vec![("svchost.exe".to_string(), 1)], "repère d'écran retiré");
        assert_eq!(d.json(0.0)["top"][0]["name"], "svchost.exe");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn historique_qui_survit_au_redemarrage() {
        let dir = std::env::temp_dir().join(format!("rustudio-drops-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("usb-drops.json");
        let mut d = UsbDrops::load(path.clone(), 1000.0);
        d.update(Some(st(true, 1000, 0)), 1000.0);
        d.update(Some(st(true, 2000, 8)), 1001.0);
        d.update(Some(st(true, 3000, 16)), 1300.0);
        d.note_launched(999.0, vec!["git.exe".into()]);
        d.note_launched(1002.6, vec![]);
        d.note_launched(1301.6, vec![]);
        save(&mut d, 1302.0);
        let r = UsbDrops::load(path.clone(), 1302.0);
        assert_eq!(r.total, 2);
        assert_eq!(r.events.len(), 2);
        assert_eq!(r.events[0].hint.as_ref().unwrap()[0].0, "git.exe", "les programmes restent attaches");
        let j = r.json(0.0);
        assert_eq!(j["count"], 2);
        assert_eq!(j["top"][0]["name"], "git.exe");
        assert_eq!(j["top"][0]["count"], 1, "une fois par coupure, meme si lance 1 fois");
        // plus de 24 h : oublie, et le fichier est reecrit sans ces coupures
        let mut vieux = UsbDrops::load(path.clone(), 1302.0 + DAY + 10.0);
        assert!(vieux.events.is_empty());
        vieux.update(Some(st(true, 10, 1690)), 1302.0 + DAY + 11.0);
        vieux.update(Some(st(true, 1010, 1698)), 1302.0 + DAY + 12.0);
        save(&mut vieux, 1302.0 + DAY + 12.0);
        assert_eq!(UsbDrops::load(path.clone(), 1302.0 + DAY + 13.0).events.len(), 1, "les coupures de la veille ont disparu du fichier");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn sauvegarde_lente_ne_perd_pas_une_nouvelle_coupure() {
        let dir = std::env::temp_dir().join(format!("rustudio-drops-concurrent-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("usb-drops.json");
        let mut d = UsbDrops::load(path.clone(), 1000.0);
        d.update(Some(st(true, 1000, 0)), 1000.0);
        d.update(Some(st(true, 2000, 8)), 1001.0);
        let pending = d.pending_history(1001.0).unwrap();
        // Une deuxieme coupure arrive pendant l'ecriture de la premiere copie, hors du verrou.
        d.update(Some(st(true, 3000, 16)), 1002.0);
        pending.write().unwrap();
        d.history_saved(&pending);
        save(&mut d, 1002.0);
        assert_eq!(UsbDrops::load(path.clone(), 1003.0).events.len(), 2);
        assert!(d.pending_history(1003.0).is_none(), "pas d'ecriture sans changement");

        // Pas besoin d'une nouvelle coupure pour effacer du disque les evenements expires.
        save(&mut d, 1003.0 + DAY);
        let stored: Value = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
        assert_eq!(stored["events"], json!([]));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn ecriture_ratee_reessayee_sans_bloquer_les_compteurs() {
        let dir = std::env::temp_dir().join(format!("rustudio-drops-retry-{}", std::process::id()));
        let path = dir.join("usb-drops.json");
        let mut d = UsbDrops::load(path.clone(), 1000.0);
        d.update(Some(st(true, 1000, 0)), 1000.0);
        // Le dossier n'existe pas : echec disque. Les releves continuent entre les tentatives.
        let _ = std::fs::remove_dir_all(&dir);
        assert!(d.pending_history(1000.0).unwrap().write().is_err());
        d.update(Some(st(true, 2000, 8)), 1001.0);
        std::fs::create_dir_all(&dir).unwrap();
        save(&mut d, 1002.0);
        assert_eq!(UsbDrops::load(path, 1003.0).events.len(), 1);
        let lines = d.take_log_lines();
        assert_eq!(lines.len(), 1);
        assert!(lines[0].contains("1 ms"));
        assert!(d.take_log_lines().is_empty(), "pas de doublon dans le journal");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn journal_sans_inondation() {
        let mut d = UsbDrops::default();
        let ev = |t| DropEvent { t, out_ms: 1.0, in_ms: 1.0, hint: None };
        assert!(d.log_line(&ev(100.0)).is_some());
        assert!(d.log_line(&ev(101.0)).is_none());
        assert!(d.log_line(&ev(105.0)).is_none());
        assert!(d.log_line(&ev(111.0)).unwrap().contains("2 autre(s)"));
    }
}
