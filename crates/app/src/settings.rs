//! Réglages de l'appli, configuration complète, mises à jour et retours de version.
use crate::{
    desktop::{Page, panel},
    mixer::Mixer,
    ui::{self, Icon, alpha, rgb},
};
use gpui::{Context, Div, FontWeight, PathPromptOptions, SharedString, div, prelude::*, px};
use rustudio_windows_desktop::session;
use serde_json::{Value, json};
use std::{path::PathBuf, time::Duration};

const UPDATE_BUSY: [&str; 4] = ["downloading", "verifying", "extracting", "applying"];

impl Mixer {
    /// Lecture des données de la page Réglages ou Versions.
    pub fn settings_data(client: &crate::backend::Client, page: Page) -> Result<Value, String> {
        if page == Page::Settings {
            let state = client.get("/api/state")?;
            return Ok(json!({"prefs": state["state"]["prefs"], "startup": session::startup_enabled(), "connected": state["connected"]}));
        }
        if !client.is_live() {
            return Ok(json!({"update": {"current": "Démonstration", "release_build": false, "status": "dev"},
                "history": {"versions": [{"version": "Démonstration", "notes": "Données simulées : aucune mise à jour n’est installée.", "installed": true}]}}));
        }
        Ok(json!({"update": client.get("/api/update")?, "history": client.get("/api/update/history")?}))
    }

    pub fn after_settings_load(&mut self, page: Page, data: &Value, cx: &mut Context<Self>) {
        let busy = UPDATE_BUSY.iter().any(|p| data["update"]["phase"] == *p) || data["update"]["status"] == "checking";
        if page == Page::Versions && busy {
            cx.spawn(async move |this, cx| {
                cx.background_executor().timer(Duration::from_millis(900)).await;
                let _ = this.update(cx, |this, cx| {
                    if this.page == page {
                        this.library_load(page, cx);
                    }
                });
            })
            .detach();
        }
    }

    fn engine_action(&mut self, path: &'static str, action: &'static str, body: Value, done: &'static str, cx: &mut Context<Self>) {
        if self.library.busy {
            return;
        }
        self.library.busy = true;
        self.library.confirm = None;
        let client = self.client.clone();
        let page = self.page;
        let job = cx.background_executor().spawn(async move { client.post_module(path, action, body) });
        cx.spawn(async move |this, cx| {
            let result = job.await;
            let _ = this.update(cx, |this, cx| {
                this.library.busy = false;
                this.library.notice = Some(match result {
                    Ok(_) => (true, done.to_string()),
                    Err(error) => (false, error),
                });
                if this.page == page {
                    this.library_load(page, cx);
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    fn ask(&mut self, message: String, path: &'static str, action: &'static str, body: Value, done: &'static str, cx: &mut Context<Self>) {
        self.library.confirm = Some((message, path, action, body, done));
        cx.notify();
    }

    pub(crate) fn confirm_box(&self, cx: &mut Context<Self>) -> Option<Div> {
        let (message, ..) = self.library.confirm.as_ref()?;
        Some(
            panel()
                .flex_row()
                .items_center()
                .gap(px(14.))
                .border_color(alpha(ui::ORANGE, 0.35))
                .child(ui::chip(Icon::Alert, ui::ORANGE, 34.))
                .child(div().flex_1().text_size(px(13.5)).child(message.clone()))
                .child(ui::pill("lib-cancel", None, "Annuler", ui::Tone::Plain).on_click(cx.listener(|this, _, _, cx| {
                    this.library.confirm = None;
                    cx.notify();
                })))
                .child(ui::pill("lib-confirm", None, "Confirmer", ui::Tone::Primary).on_click(cx.listener(|this, _, _, cx| {
                    if let Some((_, path, action, body, done)) = this.library.confirm.take() {
                        this.engine_action(path, action, body, done, cx);
                    }
                }))),
        )
    }

    fn export_config(&mut self, cx: &mut Context<Self>) {
        let client = self.client.clone();
        let initial = std::env::var_os("USERPROFILE").map(PathBuf::from).map(|p| p.join("Documents")).filter(|p| p.is_dir()).unwrap_or_else(std::env::temp_dir);
        let picker = cx.prompt_for_new_path(&initial, Some("configuration-rustudio.json"));
        cx.spawn(async move |this, cx| {
            let result = match picker.await {
                Ok(Ok(Some(path))) => {
                    let path = if path.extension().is_none() { path.with_extension("json") } else { path };
                    cx.background_executor().spawn(async move {
                        let state = client.get("/api/modules/goxlr-mini/state")?;
                        let mut config = state["state"].clone();
                        if !config.is_object() {
                            return Err("Configuration indisponible.".to_string());
                        }
                        // L'empreinte vocale et la mesure du micro restent sur ce PC.
                        for key in ["voice_print", "mic_cal"] {
                            config.as_object_mut().map(|o| o.remove(key));
                        }
                        let text = serde_json::to_string_pretty(&config).map_err(|e| e.to_string())?;
                        std::fs::write(&path, text).map_err(|e| format!("Écriture impossible : {e}"))
                    }).await
                }
                Ok(Ok(None)) => return,
                _ => Err("Impossible d’ouvrir le dossier d’export.".into()),
            };
            let _ = this.update(cx, |this, cx| {
                this.library.notice = Some(match result {
                    Ok(()) => (true, "Configuration exportée.".into()),
                    Err(e) => (false, e),
                });
                cx.notify();
            });
        })
        .detach();
    }

    fn import_config(&mut self, cx: &mut Context<Self>) {
        let picker = cx.prompt_for_paths(PathPromptOptions { files: true, directories: false, multiple: false, prompt: None });
        cx.spawn(async move |this, cx| {
            let read = match picker.await {
                Ok(Ok(Some(paths))) if !paths.is_empty() => {
                    let path = paths[0].clone();
                    cx.background_executor().spawn(async move {
                        let meta = std::fs::metadata(&path).map_err(|e| e.to_string())?;
                        if meta.len() > 4_000_000 {
                            return Err("Fichier trop volumineux.".to_string());
                        }
                        let value: Value = serde_json::from_slice(&std::fs::read(&path).map_err(|e| e.to_string())?)
                            .map_err(|_| "Fichier illisible : JSON attendu.".to_string())?;
                        if !value.is_object() || value.get("volumes").is_none() {
                            return Err("Ce fichier n’est pas une configuration Rustudio.".into());
                        }
                        Ok(value)
                    }).await
                }
                Ok(Ok(_)) => return,
                _ => Err("Impossible d’ouvrir le sélecteur de fichiers.".into()),
            };
            let _ = this.update(cx, |this, cx| match read {
                Ok(config) => this.ask(
                    "Remplacer toute la configuration de la GoXLR Mini (mixeur, routage, éclairage, micro, scènes) par ce fichier ?".into(),
                    "/api/modules/goxlr-mini", "profile", json!({"profile": config}), "Configuration importée.", cx,
                ),
                Err(e) => {
                    this.library.notice = Some((false, e));
                    cx.notify();
                }
            });
        })
        .detach();
    }

    pub fn settings_view(&self, data: &Value, cx: &mut Context<Self>) -> Div {
        let prefs = &data["prefs"];
        let hold = prefs["hold_ms"].as_i64().unwrap_or(600);
        let osd = prefs["osd"] != false;
        let startup = data["startup"] == true;
        let step = |delta: i64| (hold + delta).clamp(250, 1500);
        let root = div().flex().flex_col().gap(px(16.));
        let (less, more) = (step(-50), step(50));
        let mut general = ui::section(Icon::Settings, ui::ACCENT, "Comportement", None)
            .child(ui::list_row(
                Some((Icon::Mic, ui::RED)),
                "Bulle à l’écran quand le micro change",
                Some("Une pastille discrète apparaît au centre de l’écran.".into()),
                ui::toggle("osd-toggle", osd, true).on_click(cx.listener(move |this, _, _, cx| {
                    this.engine_action("/api/modules/goxlr-mini", "settings", json!({"prefs": {"osd": !osd}}), "Préférence enregistrée.", cx)
                })),
                true,
            ))
            .child(ui::list_row(
                Some((Icon::Gauge, ui::ORANGE)),
                "Durée d’un appui long",
                Some("Pour les boutons de la GoXLR qui ont deux fonctions.".into()),
                ui::stepper(
                    ui::step_button("hold-less", Icon::Minus).on_click(cx.listener(move |this, _, _, cx| {
                        this.engine_action("/api/modules/goxlr-mini", "settings", json!({"prefs": {"hold_ms": less}}), "Durée enregistrée.", cx)
                    })),
                    format!("{:.2} s", hold as f64 / 1000.).replace('.', ","),
                    ui::step_button("hold-more", Icon::Plus).on_click(cx.listener(move |this, _, _, cx| {
                        this.engine_action("/api/modules/goxlr-mini", "settings", json!({"prefs": {"hold_ms": more}}), "Durée enregistrée.", cx)
                    })),
                ),
                false,
            ));
        if self.client.is_live() {
            general = general.child(ui::list_row(
                Some((Icon::Power, ui::GREEN)),
                "Démarrer avec Windows",
                Some("L’icône attend près de l’horloge.".into()),
                ui::toggle("startup-toggle", startup, true).on_click(cx.listener(move |this, _, _, cx| {
                    this.library.notice = Some(match session::set_startup(!startup) {
                        Ok(()) => (true, "Démarrage avec Windows mis à jour.".into()),
                        Err(e) => (false, e),
                    });
                    this.library_load(Page::Settings, cx);
                })),
                false,
            ));
        }
        let config = ui::section(Icon::Download, 0x5856d6, "Sauvegarde", None)
            .child(ui::list_row(
                None,
                "Exporter ma configuration",
                Some("Toute la GoXLR Mini dans un fichier, sans l’empreinte vocale.".into()),
                ui::pill("config-export", Some(Icon::Download), "Exporter", ui::Tone::Plain).on_click(cx.listener(|this, _, _, cx| this.export_config(cx))),
                true,
            ))
            .child(ui::list_row(
                None,
                "Importer une configuration",
                Some("Remplace la configuration actuelle, après confirmation.".into()),
                ui::pill("config-import", Some(Icon::Upload), "Importer", ui::Tone::Plain).on_click(cx.listener(|this, _, _, cx| this.import_config(cx))),
                false,
            ));
        root.child(general).child(config)
    }

    pub fn versions_view(&self, data: &Value, cx: &mut Context<Self>) -> Div {
        let u = &data["update"];
        let mut root = div().flex().flex_col().gap(px(16.));
        let current = u["current"].as_str().unwrap_or("?").to_string();
        let busy = UPDATE_BUSY.iter().any(|p| u["phase"] == *p);
        let available = u["status"] == "available";
        let latest = u["latest"]["version"].as_str().unwrap_or("").to_string();
        let (state, colour) = if busy {
            ("Installation en cours", ui::ACCENT)
        } else if u["phase"] == "failed" {
            ("Mise à jour interrompue", ui::ORANGE)
        } else if available {
            ("Nouvelle version disponible", ui::ACCENT)
        } else if u["release_build"] == false {
            ("Version de développement", ui::TEXT3)
        } else if u["status"] == "up-to-date" {
            ("À jour", ui::GREEN)
        } else {
            ("Pas encore vérifiée", ui::TEXT3)
        };
        let mut hero = ui::card().flex_row().items_center().gap(px(16.)).p(px(22.))
            .child(ui::logo(56.))
            .child(div().flex().flex_col().flex_1().gap(px(4.)).child(ui::title(format!("Rustudio {current}"), 20.)).child(div().flex().child(ui::badge(state, colour))));
        if self.client.is_live() && u["release_build"] != false {
            hero = hero.child(if busy {
                div().w(px(220.)).flex().flex_col().gap(px(6.))
                    .child(ui::progress(u["progress"].as_f64().unwrap_or(1.) as f32, ui::ACCENT))
                    .child(ui::caption(match u["phase"].as_str() {
                        Some("downloading") => format!("Téléchargement · {} %", (u["progress"].as_f64().unwrap_or(0.) * 100.).round()),
                        Some("verifying") => "Vérification de l’empreinte…".into(),
                        _ => "Rustudio redémarre et se rouvre tout seul.".into(),
                    }))
                    .into_any_element()
            } else if available && u["installable"] == true {
                ui::pill("update-install", Some(Icon::Download), format!("Mettre à jour vers la {latest}"), ui::Tone::Primary).h(px(40.)).px(px(18.))
                    .on_click(cx.listener(|this, _, _, cx| this.update_click(cx))).into_any_element()
            } else {
                ui::pill("update-check", Some(Icon::Refresh), "Rechercher une mise à jour", ui::Tone::Plain)
                    .on_click(cx.listener(|this, _, _, cx| this.update_click(cx))).into_any_element()
            });
        }
        root = root.child(hero);
        if available {
            root = root.child(
                ui::section(Icon::Sparkles, ui::ACCENT, format!("Nouveautés de la {latest}"), None)
                    .child(ui::rich_text(u["latest"]["notes"].as_str().unwrap_or(""))),
            );
        }
        if u["phase"] == "failed" {
            root = root.child(ui::notice(false, format!("Échec : {}", u["error"]["msg"].as_str().unwrap_or("mise à jour interrompue"))));
        }
        let mut history = ui::section(Icon::History, 0x8e8e93, "Historique", Some("Survole une ancienne version pour y revenir".into())).gap(px(4.));
        for (n, item) in data["history"]["versions"].as_array().into_iter().flatten().enumerate() {
            let version = item["version"].as_str().unwrap_or("?").to_string();
            let expanded = self.library.expanded.as_deref() == Some(&version);
            let key = format!("release-{version}");
            let t = ui::hover_t(&key);
            let revertible = item["revertible"] == true && !busy && self.client.is_live();
            let date = item["published"].as_str().map(short_date).unwrap_or_default();
            let id = version.clone();
            let target = version.clone();
            let mut badges = div().flex().gap(px(6.));
            if item["installed"] == true {
                badges = badges.child(ui::badge("Installée", ui::GREEN));
            } else if item["newer"] == true {
                badges = badges.child(ui::badge("Nouvelle", ui::ACCENT));
            }
            let revert = ui::round(SharedString::from(format!("revert-{version}")), Icon::History, 30., alpha(ui::ACCENT, 0.1), rgb(ui::ACCENT))
                .opacity(t)
                .on_click(cx.listener(move |this, _, _, cx| {
                    cx.stop_propagation();
                    this.ask(format!("Revenir à Rustudio {target} ? Tes réglages restent ; mêmes vérifications qu’une mise à jour."), "/api", "update-install", json!({"version": target}), "Retour lancé.", cx)
                }));
            let row = div()
                .id(SharedString::from(key.clone()))
                .flex()
                .items_center()
                .gap(px(12.))
                .px(px(10.))
                .h(px(48.))
                .rounded(px(12.))
                .cursor(gpui::CursorStyle::PointingHand)
                .when(n > 0, |d| d.mt(px(2.)))
                .bg(ui::lerp(alpha(ui::ACCENT, 0.), alpha(ui::ACCENT, 0.05), t.max(if expanded { 1. } else { 0. })))
                .child(div().w(px(70.)).text_size(px(14.)).font_weight(FontWeight::SEMIBOLD).child(version.clone()))
                .child(badges)
                .child(div().flex_1())
                .child(ui::caption(date))
                .when(revertible, |d| d.child(revert))
                .child(div().relative().top(px(if expanded { 0. } else { 1. * t })).child(ui::icon(if expanded { Icon::ChevronDown } else { Icon::ChevronRight }, 15., rgb(ui::TEXT3))))
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.library.expanded = if this.library.expanded.as_ref() == Some(&id) { None } else { Some(id.clone()) };
                    cx.notify();
                }));
            history = history.child(ui::hoverable(row, key));
            if expanded {
                history = history.child(div().px(px(14.)).pt(px(6.)).pb(px(12.)).child(ui::rich_text(item["notes"].as_str().unwrap_or("Aucune note."))));
            }
        }
        root.child(history)
    }
}

/// « 2026-11-08T15:34:05Z » → « 8 nov. 2026 ».
fn short_date(iso: &str) -> String {
    const MONTHS: [&str; 12] = ["janv.", "févr.", "mars", "avr.", "mai", "juin", "juil.", "août", "sept.", "oct.", "nov.", "déc."];
    let mut parts = iso.get(..10).unwrap_or("").split('-');
    match (parts.next(), parts.next().and_then(|m| m.parse::<usize>().ok()), parts.next().and_then(|d| d.parse::<u32>().ok())) {
        (Some(y), Some(m), Some(d)) if (1..=12).contains(&m) => format!("{d} {} {y}", MONTHS[m - 1]),
        _ => String::new(),
    }
}
