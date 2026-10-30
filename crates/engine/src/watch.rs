//! Applis ouvertes : scenes automatiques et egaliseur du casque coupe pendant le live.
use crate::config::now_epoch;
use crate::api::ApiResult;
use crate::controller::{AutoActive, Ctl};
use crate::log::log;
use crate::logf;
use rustudio_core::consts::{known_app, SYSTEM_PROCESSES};
use rustudio_core::hpeq::{config_text, LIVE_APPS};
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
        }
    }

    /// Ecrit la config d'Equalizer APO d'apres l'etat (coupee si desactive ou pendant le live).
    pub fn hp_apply(&self) {
        let (eq, live) = {
            let g = self.lock();
            (g.state.hp_eq.clone(), g.hp_live)
        };
        let text = if !eq.enabled {
            config_text(&eq, false, "desactive dans l'appli")
        } else if eq.pause_live && live {
            config_text(&eq, false, "live en cours : le stream recoit le son sans egaliseur")
        } else if eq.channels.is_empty() {
            config_text(&eq, false, "aucune voie choisie")
        } else {
            config_text(&eq, true, "")
        };
        if eq.enabled && self.eqapo.installed() {
            self.eqapo.take_config();
        }
        self.eqapo.write(&text);
    }

    pub fn hp_tick(&self, running: Option<&HashSet<String>>) {
        let live = running.map(|r| r.iter().any(|a| LIVE_APPS.contains(&norm_app(a).as_str()))).unwrap_or(false);
        let (eq, changed) = {
            let mut g = self.lock();
            let changed = live != g.hp_live;
            g.hp_live = live;
            (g.state.hp_eq.clone(), changed)
        };
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

    pub fn hp_start(self: &Arc<Self>, install: bool) -> ApiResult {
        if self.eqapo.setup_running() {
            return Err(crate::api::ApiError::bad("Installation déjà en cours."));
        }
        self.eqapo.set_setup(&[("running", json!(true)), ("step", json!("Préparation…")), ("error", Value::Null), ("done", json!(false)), ("install", json!(install))], true);
        let c = self.clone();
        std::thread::spawn(move || c.hp_setup_run(install));
        Ok(json!({"ok": true}))
    }

    fn hp_setup_run(&self, install: bool) {
        let step = |t: &str| self.eqapo.set_setup(&[("step", json!(t))], false);
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
            }
            Ok(())
        })();
        match r {
            Ok(()) => self.eqapo.set_setup(&[("running", json!(false)), ("done", json!(true)), ("step", json!("Terminé"))], false),
            Err(e) => {
                logf!("Égaliseur du casque : {e}");
                self.eqapo.set_setup(&[("running", json!(false)), ("error", json!(e))], false);
            }
        }
        self.lock().version += 1;
    }
}
