//! Coupures du son entre le PC et la GoXLR, comptees par le pilote lui-meme (voir rustudio_goxlr::stream).
//! La boucle USB relit ses compteurs chaque seconde : aucun son de test, rien d'envoye a l'appareil. A chaque
//! coupure, le moteur note l'heure, le son perdu et les programmes que Windows venait de lancer (une rafale de
//! lancements suffit a faire sauter le son USB : c'est ainsi qu'on a trouve la synchro de projets du 29/10).
use rustudio_goxlr::stream::{StreamCounters, StreamStats};
use serde_json::{json, Value};
use std::collections::{BTreeMap, VecDeque};

const KEEP: usize = 300;

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
    pub streaming: bool,
    pub events: VecDeque<DropEvent>,
    pub total: u64,
    launched: VecDeque<(f64, Vec<String>)>,
    last_log: f64,
    unlogged: u32,
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
    /// Nouveau releve (None : compteurs illisibles). Renvoie la coupure survenue depuis le releve precedent.
    pub fn update(&mut self, s: Option<StreamStats>, now: f64) -> Option<DropEvent> {
        let Some(s) = s else {
            if self.available.is_none() {
                self.available = Some(false);
            }
            self.reset();
            return None;
        };
        self.available = Some(true);
        self.since.get_or_insert(now);
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
        self.events.push_back(ev.clone());
        Some(ev)
    }

    /// GoXLR deconnectee : les compteurs repartent de zero a la reconnexion.
    pub fn reset(&mut self) {
        self.prev = None;
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
        }
    }

    pub fn json(&self, since: f64) -> Value {
        let evs: Vec<&DropEvent> = self.events.iter().filter(|e| e.t >= since).collect();
        let sum = |f: fn(&DropEvent) -> f64| (evs.iter().map(|e| f(e)).sum::<f64>() * 1000.0).round() / 1000.0 + 0.0; // + 0.0 : pas de -0
        json!({
            "available": self.available, "since": self.since, "streaming": self.streaming, "total": self.total,
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
        d.update(Some(st(true, 1, 0)), 2.0);
        d.update(None, 3.0);
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
    fn journal_sans_inondation() {
        let mut d = UsbDrops::default();
        let ev = |t| DropEvent { t, out_ms: 1.0, in_ms: 1.0, hint: None };
        assert!(d.log_line(&ev(100.0)).is_some());
        assert!(d.log_line(&ev(101.0)).is_none());
        assert!(d.log_line(&ev(105.0)).is_none());
        assert!(d.log_line(&ev(111.0)).unwrap().contains("2 autre(s)"));
    }
}
