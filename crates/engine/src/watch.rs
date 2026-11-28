//! Applis ouvertes : scenes automatiques et egaliseur du casque coupe pendant le live.
use crate::config::now_epoch;
use crate::api::ApiResult;
use crate::controller::{AutoActive, Ctl};
use crate::log::log;
use crate::logf;
use rustudio_core::consts::{known_app, SYSTEM_PROCESSES};
use rustudio_core::hpeq::{config_text_for, HpEq, LIVE_APPS};
use rustudio_core::state::norm_app;
use serde_json::{json, Value};
use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;

impl Ctl {
    pub fn running_apps(&self) -> HashSet<String> {
        if let Some(sim) = &self.sim {
            return sim.lock().apps.iter().map(|a| a.to_lowercase()).collect();
        }
        if self.cfg.simulate {
            return HashSet::new();
        }
        crate::win::processes()
    }

    pub fn user_apps(&self) -> Value {
        let mut names: Vec<String> = self.running_apps().into_iter().filter(|n| !SYSTEM_PROCESSES.contains(&n.as_str())).collect();
        names.sort();
        let known: Vec<Value> = names.iter().filter_map(|n| known_app(n).map(|l| json!({"app": n, "label": l}))).collect();
        let others: Vec<&String> = names.iter().filter(|n| known_app(n).is_none()).take(300).collect();
        json!({"known": known, "others": others})
    }

    /// Applique la scene de la premiere regle dont l'appli tourne (seulement quand ca change).
    pub fn auto_scene_tick(&self, running: &HashSet<String>) -> Option<String> {
        let (target, why) = {
            let mut g = self.lock();
            let cfg = g.state.auto_scenes.clone();
            if !cfg.enabled || cfg.rules.is_empty() {
                g.auto_active = AutoActive::Off;
                return None;
            }
            let names: HashSet<String> = running.iter().map(|n| norm_app(n)).collect();
            let m = cfg.rules.iter().find(|r| names.contains(&norm_app(&r.app))).cloned();
            let key = m.as_ref().map(|r| AutoActive::On(r.app.clone(), r.scene.clone())).unwrap_or(AutoActive::Off);
            if key == g.auto_active {
                g.auto_seen = true;
                return None;
            }
            let prev = std::mem::replace(&mut g.auto_active, key);
            if !g.auto_seen {
                // premiere evaluation apres le demarrage : l'etat sauvegarde fait foi
                g.auto_seen = true;
                if let Some(r) = &m {
                    g.auto_note = Some(json!({"t": now_epoch(), "scene": r.scene, "why": format!("{} déjà ouvert", r.app), "applied": false}));
                }
                return None;
            }
            let (target, why) = if let Some(r) = &m {
                (r.scene.clone(), format!("{} ouvert", r.app))
            } else if matches!(prev, AutoActive::On(..)) && cfg.fallback.is_some() {
                (cfg.fallback.clone().unwrap(), "plus aucune appli de la liste".to_string())
            } else {
                return None;
            };
            if g.state.scene.as_deref() == Some(&target) {
                g.auto_note = Some(json!({"t": now_epoch(), "scene": target, "why": why, "applied": false}));
                return None;
            }
            if g.apply_scene(&target).is_err() {
                return None;
            }
            g.auto_note = Some(json!({"t": now_epoch(), "scene": target, "why": why, "applied": true}));
            g.version += 1;
            g.save_dirty = true;
            (target, why)
        };
        logf!("Scène automatique : {target} ({why})");
        Some(target)
    }

    pub fn app_watch_loop(self: &Arc<Self>, interval: Duration) {
        self.hp_apply();
        self.ns_apply();
        self.winmic_apply();
        #[cfg(windows)] if !self.simulated() {
            let refresh = rustudio_windows_mic::registry::refresh_binary().map_err(|e|e.to_string())
                .and_then(|changed|if changed {crate::native_mic::restart_audio(false)} else {Ok(())});
            match refresh {
                Err(e) => logf!("Mise à jour de l’effet micro : {e}"),
                Ok(()) => if let Err(e) = rustudio_windows_mic::registry::cleanup_binary_cache() {
                    logf!("Anciennes copies de l’effet micro : {e}");
                },
            }
        }
        loop {
            std::thread::sleep(interval);
            let (auto, hp) = {
                let g = self.lock();
                (g.state.auto_scenes.enabled, g.state.hp_eq.enabled)
            };
            let running = if auto || hp { Some(self.running_apps()) } else { None };
            if auto {
                if let Some(r) = &running {
                    self.auto_scene_tick(r);
                }
            }
            self.hp_tick(running.as_ref());
            self.ns_apply(); // micro débranché, rebranché, ou autre micro par défaut
            self.winmic_apply();
            self.winmic_watchdog();
        }
    }

    /// Entrée visée par la suppression du bruit : celle choisie, sinon le micro de la GoXLR, sinon le micro par
    /// défaut de Windows (signalé par l'appli).
    pub fn ns_target(&self) -> Option<crate::eqapo::Capture> {
        let (chosen, default) = {
            let g = self.lock();
            (g.state.mic_ns.device.clone(), g.mic_default.clone())
        };
        let caps = self.eqapo.captures();
        let find = |g: &str| caps.iter().find(|c| c.guid.eq_ignore_ascii_case(g)).cloned();
        chosen.as_deref().and_then(find).or_else(|| caps.iter().find(|c| c.goxlr).cloned()).or_else(|| default.as_deref().and_then(find))
    }

    /// Écrit la configuration du micro (RNNoise sur l'entrée visée) si elle a changé.
    pub fn ns_apply(&self) {
        let ns = self.lock().state.mic_ns.clone();
        let target = self.ns_target();
        let ready = self.eqapo.installed() && self.eqapo.plugin_installed();
        let why = if !ns.enabled {
            "desactivee dans l'appli"
        } else if !ready {
            "module non installe"
        } else if target.as_ref().is_none_or(|t| !t.installed) {
            "micro non branche"
        } else {
            ""
        };
        let dir = self.eqapo.plugin_dir().to_string_lossy().to_string();
        let t = target.filter(|t| t.installed && ready && !self.winmic.native.attached(&t.guid.to_ascii_lowercase()));
        let text = rustudio_core::micns::config_text(&ns, t.as_ref().map(|t| (t.guid.as_str(), t.channels)), &dir, why);
        self.eqapo.write_mic(&text);
    }

    /// État de la suppression du bruit pour l'appli.
    pub fn ns_status(&self) -> Value {
        let target = self.ns_target();
        let setup = self.eqapo.setup_value().filter(|s| s["kind"] == "mic-ns");
        json!({
            "apo": self.eqapo.installed(), "plugin": self.eqapo.plugin_installed(),
            "captures": self.eqapo.captures().iter().map(|c| json!({"guid": c.guid, "name": c.name, "installed": c.installed, "goxlr": c.goxlr})).collect::<Vec<_>>(),
            "target": target.as_ref().map(|t| t.guid.clone()), "target_name": target.as_ref().map(|t| t.name.clone()),
            "ready": self.eqapo.installed() && self.eqapo.plugin_installed() && target.as_ref().is_some_and(|t| t.installed),
            "setup": setup,
        })
    }

    /// Installe ce qu'il faut (Equalizer APO, RNNoise), branche l'entrée visée et active la suppression ; ou la
    /// retire de cette entrée.
    pub fn ns_start(self: &Arc<Self>, install: bool) -> ApiResult {
        if self.ns_target().is_some_and(|c| self.winmic.native.managed(&c.guid.to_ascii_lowercase())) {
            return Err(crate::api::ApiError::bad("Ce micro utilise l’effet Rustudio. Règle son anti-bruit dans Mon micro."));
        }
        if self.eqapo.setup_running() {
            return Err(crate::api::ApiError::bad("Une opération sur Equalizer APO est déjà en cours."));
        }
        self.eqapo.begin_setup("mic-ns",install)?;
        let c = self.clone();
        std::thread::spawn(move || {
            let step = |t: &str| c.eqapo.set_setup(&[("step", json!(t))]);
            let r = (|| -> Result<(), String> {
                if install {
                    if !c.eqapo.installed() {
                        c.eqapo.download_and_install(&step)?;
                    }
                    c.eqapo.install_plugin(&step)?;
                    let target = c.ns_target().ok_or("Aucun micro trouvé dans Windows : branche-le puis recommence.")?;
                    step("Branchement sur ton micro…");
                    if !target.installed {
                        c.eqapo.install_capture(&target.guid)?;
                    }
                    {
                        let mut g = c.lock();
                        g.state.mic_ns.enabled = true;
                        g.save_dirty = true;
                        g.version += 1;
                    }
                    c.eqapo.take_config();
                    c.ns_apply();
                    step("Redémarrage du son de Windows (2 secondes)…");
                    c.eqapo.restart_audio();
                    logf!("Suppression du bruit active sur « {} »", target.name);
                } else {
                    if let Some(target) = c.ns_target().filter(|t| t.installed) {
                        step("Retrait du micro…");
                        c.eqapo.uninstall_capture(&target.guid)?;
                    }
                    {
                        let mut g = c.lock();
                        g.state.mic_ns.enabled = false;
                        g.save_dirty = true;
                        g.version += 1;
                    }
                    c.ns_apply();
                    step("Redémarrage du son de Windows (2 secondes)…");
                    c.eqapo.restart_audio();
                    log("Suppression du bruit retirée du micro");
                }
                Ok(())
            })();
            match r {
                Ok(()) => c.eqapo.set_setup(&[("running", json!(false)), ("done", json!(true)), ("step", json!("Terminé"))]),
                Err(e) => {
                    logf!("Suppression du bruit : {e}");
                    c.eqapo.set_setup(&[("running", json!(false)), ("error", json!(e))]);
                }
            }
            c.lock().version += 1;
        });
        Ok(json!({"ok": true}))
    }

    /// Ecrit la config d'Equalizer APO d'apres l'etat (coupee si desactive ou pendant le live).
    /// Réglage de l'égaliseur tel qu'appliqué : les voies choisies, plus la sortie Windows par défaut.
    fn hp_effective(&self) -> (HpEq, bool, Option<String>) {
        let g = self.lock();
        let mut eq = g.state.hp_eq.clone();
        let extra = g.hp_default.clone().filter(|c| !eq.channels.contains(c));
        if let Some(c) = &extra {
            eq.channels.push(c.clone());
        }
        (eq, g.hp_live, g.hp_default.clone())
    }

    pub fn hp_apply(&self) {
        let (eq, live, default) = self.hp_effective();
        // chaque sortie visée par son identifiant : jamais le micro qui porte le même nom
        let eps = self.eqapo.endpoints();
        let outputs: indexmap::IndexMap<String, String> = eps.iter().map(|(c, e)| (c.clone(), e.guid.clone())).collect();
        let text = if !eq.enabled {
            config_text_for(&eq, false, "desactive dans l'appli", &outputs)
        } else if eq.pause_live && live {
            config_text_for(&eq, false, "live en cours : le stream recoit le son sans egaliseur", &outputs)
        } else if eq.channels.is_empty() {
            config_text_for(&eq, false, "aucune voie choisie", &outputs)
        } else {
            config_text_for(&eq, true, "", &outputs)
        };
        if eq.enabled && self.eqapo.installed() {
            self.eqapo.take_config();
        }
        self.eqapo.write(&text);
        // nouvelle sortie par défaut sans Equalizer APO : on l'y branche tout de suite, une fois
        if eq.enabled && self.eqapo.installed() && !self.eqapo.setup_running() {
            if let Some(c) = default {
                if let Some(ep) = eps.get(&c).filter(|e| !e.installed) {
                    if self.eqapo.try_default(&c) {
                        self.eqapo.mark_auto();
                        match self.eqapo.install_endpoint(&ep.guid) {
                            Ok(()) => {
                                self.eqapo.restart_audio();
                                logf!("Égaliseur du casque branché sur la sortie Windows par défaut ({c})");
                                self.eqapo.bump();
                            }
                            Err(e) => logf!("Égaliseur du casque : branchement sur {c} impossible ({e})"),
                        }
                    }
                }
            }
        }
    }

    pub fn hp_tick(&self, running: Option<&HashSet<String>>) {
        let live = running.map(|r| r.iter().any(|a| LIVE_APPS.contains(&norm_app(a).as_str()))).unwrap_or(false);
        let changed = {
            let mut g = self.lock();
            let changed = live != g.hp_live;
            g.hp_live = live;
            changed
        };
        let (eq, _, _) = self.hp_effective();
        if changed {
            if eq.enabled && eq.pause_live {
                log(if live { "Égaliseur du casque coupé : live en cours" } else { "Égaliseur du casque réactivé : fin du live" });
            }
            self.hp_apply();
        }
        // GoXLR sur un autre port USB : nouvelles voies Windows, on y rebranche l'egaliseur (une fois par 10 min)
        if eq.enabled && self.eqapo.installed() && !self.eqapo.setup_running() && self.eqapo.auto_due() {
            let eps = self.eqapo.endpoints();
            let todo: Vec<String> = eq.channels.iter().filter_map(|c| eps.get(c)).filter(|e| !e.installed).map(|e| e.guid.clone()).collect();
            if !todo.is_empty() {
                self.eqapo.mark_auto();
                for g in &todo {
                    let _ = self.eqapo.install_endpoint(g);
                }
                self.eqapo.restart_audio();
                logf!("Égaliseur du casque branché sur {} voie(s) GoXLR", todo.len());
                self.eqapo.bump();
            }
        }
    }

    pub fn hp_start(self: &Arc<Self>, mode: HpSetup) -> ApiResult {
        if self.eqapo.setup_running() {
            return Err(crate::api::ApiError::bad("Une opération sur Equalizer APO est déjà en cours."));
        }
        let install = mode == HpSetup::Install;
        let kind = match mode {
            HpSetup::Install => "install",
            HpSetup::Remove => "remove",
            HpSetup::Uninstall => "uninstall",
        };
        self.eqapo.begin_setup(kind,install)?;
        let c = self.clone();
        std::thread::spawn(move || c.hp_setup_run(mode));
        Ok(json!({"ok": true}))
    }

    fn hp_setup_run(&self, mode: HpSetup) {
        let install = mode == HpSetup::Install;
        let step = |t: &str| self.eqapo.set_setup(&[("step", json!(t))]);
        let r = (|| -> Result<(), String> {
            let eq = self.lock().state.hp_eq.clone();
            if install {
                if !self.eqapo.installed() {
                    self.eqapo.download_and_install(&step)?;
                }
                step("Branchement sur les voies GoXLR…");
                let eps = self.eqapo.endpoints();
                if !eq.channels.iter().any(|c| eps.contains_key(c)) {
                    return Err("Aucune voie GoXLR trouvée dans Windows : branche la GoXLR puis recommence.".into());
                }
                for c in &eq.channels {
                    if let Some(e) = eps.get(c).filter(|e| !e.installed) {
                        self.eqapo.install_endpoint(&e.guid)?;
                    }
                }
                {
                    let mut g = self.lock();
                    g.state.hp_eq.enabled = true;
                    g.save_dirty = true;
                }
                self.eqapo.reset_written();
                self.hp_apply();
                step("Redémarrage du son de Windows (2 secondes)…");
                self.eqapo.restart_audio();
                let on: Vec<&String> = eq.channels.iter().filter(|c| eps.contains_key(*c)).collect();
                logf!("Égaliseur du casque installé sur les voies {}", on.iter().map(|s| s.as_str()).collect::<Vec<_>>().join(", "));
            } else {
                step("Retrait de l'égaliseur des voies GoXLR…");
                for (_, e) in self.eqapo.endpoints() {
                    if e.installed {
                        self.eqapo.uninstall_endpoint(&e.guid)?;
                    }
                }
                {
                    let mut g = self.lock();
                    g.state.hp_eq.enabled = false;
                    g.save_dirty = true;
                }
                self.hp_apply();
                step("Redémarrage du son de Windows (2 secondes)…");
                self.eqapo.restart_audio();
                log("Égaliseur du casque retiré des voies GoXLR");
                if mode == HpSetup::Uninstall && self.eqapo.installed() {
                    step("Désinstallation d'Equalizer APO…");
                    self.eqapo.uninstall_software()?;
                    log("Equalizer APO désinstallé de Windows");
                }
            }
            Ok(())
        })();
        match r {
            Ok(()) => self.eqapo.set_setup(&[("running", json!(false)), ("done", json!(true)), ("step", json!("Terminé"))]),
            Err(e) => {
                logf!("Égaliseur du casque : {e}");
                self.eqapo.set_setup(&[("running", json!(false)), ("error", json!(e))]);
            }
        }
        self.lock().version += 1;
    }
}

/// Opérations sur Equalizer APO lancées depuis la page Casque.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HpSetup {
    /// installe le logiciel s'il manque et le branche sur les voies GoXLR
    Install,
    /// le débranche des voies GoXLR, le logiciel reste sur Windows
    Remove,
    /// le débranche puis le désinstalle de Windows
    Uninstall,
}
