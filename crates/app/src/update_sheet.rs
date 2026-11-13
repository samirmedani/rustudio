//! Fenêtre de mise à jour, sur le modèle de Wispad : les notes de chaque version à rattraper en rubriques claires,
//! ce que fait le clic, puis l'installation en quatre étapes (Téléchargement, Vérification, Installation,
//! Redémarrage). Après le redémarrage, la nouvelle version rouvre la fenêtre : « Redémarrage » se coche, puis
//! « Rustudio X est installé. » avec ses nouveautés, même hors ligne (CHANGELOG embarqué).
use crate::mixer::Mixer;
use crate::ui::{self, Icon, Tone, alpha, rgb};
use gpui::{AnyElement, Context, Div, FontWeight, SharedString, div, prelude::*, px};
use rustudio_core::notes::{self, Kind};
use serde_json::{Value, json};
use std::time::{Duration, Instant};

/// Notes de chaque version, embarquées : la fenêtre « installé » s'affiche aussi hors ligne.
const CHANGELOG: &str = include_str!("../../../CHANGELOG.md");
/// Chaque étape reste affichée au moins ce temps : sans cette pause, Vérification et Installation passeraient sans
/// qu'on les voie avant la fermeture.
const STEP_MIN: Duration = Duration::from_millis(600);
const STEPS: [&str; 4] = ["Téléchargement", "Vérification", "Installation", "Redémarrage"];
/// Versions plus anciennes affichées d'emblée ; les suivantes derrière « Afficher les N versions précédentes ».
const OPEN_CARDS: usize = 3;
const MAX_CARDS: usize = 30;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SheetKind {
    /// nouvelle version : notes à lire, puis installation
    Update,
    /// « Nouveautés de cette version » : la version installée, sans installation
    Current,
    /// juste installée, rouverte par la nouvelle version après le redémarrage
    Done,
}

pub struct Sheet {
    pub kind: SheetKind,
    history: Option<Value>,
    show_older: bool,
    shown: usize,
    shown_at: Instant,
    from: Option<String>,
    downgrade: bool,
}

/// Une version à afficher : numéro, date, notes.
struct Card {
    version: String,
    date: Option<String>,
    notes: String,
}

/// Étape de l'installation (0 à 3) d'après la phase du moteur ; None hors installation.
fn step_of(phase: &str) -> Option<usize> {
    match phase {
        "downloading" => Some(0),
        "verifying" | "extracting" => Some(1),
        "applying" => Some(2),
        _ => None,
    }
}

/// « 2026-11-08T15:34:05Z » → « 8 novembre 2026 ».
fn long_date(iso: &str) -> Option<String> {
    const MONTHS: [&str; 12] = ["janvier", "février", "mars", "avril", "mai", "juin", "juillet", "août", "septembre", "octobre", "novembre", "décembre"];
    let mut parts = iso.get(..10)?.split('-');
    let (y, m, d) = (parts.next()?, parts.next()?.parse::<usize>().ok()?, parts.next()?.parse::<u32>().ok()?);
    (1..=12).contains(&m).then(|| format!("{}{} {} {y}", d, if d == 1 { "er" } else { "" }, MONTHS[m - 1]))
}

fn megabytes(bytes: u64) -> String {
    format!("{} Mo", crate::mixer::fmt_num(bytes as f64 / 1_000_000.))
}

fn section_style(kind: Kind) -> (Icon, u32) {
    match kind {
        Kind::New => (Icon::Sparkles, ui::ACCENT),
        Kind::Improved => (Icon::Activity, ui::GREEN),
        Kind::Fixed => (Icon::Check, ui::ORANGE),
        Kind::Other => (Icon::Lines, ui::TEXT3),
    }
}

impl Sheet {
    fn new(kind: SheetKind) -> Sheet {
        Sheet { kind, history: None, show_older: false, shown: if kind == SheetKind::Done { 3 } else { 0 }, shown_at: Instant::now(), from: None, downgrade: false }
    }
}

impl Mixer {
    pub(crate) fn open_update_sheet(&mut self, kind: SheetKind, cx: &mut Context<Self>) {
        self.sheet = Some(Sheet::new(kind));
        ui::reset_hover();
        if kind == SheetKind::Update && self.client.is_live() {
            // dates et notes de chaque version à rattraper ; sans réponse, les notes composées par le moteur suffisent
            let client = self.client.clone();
            let job = cx.background_executor().spawn(async move { client.get("/api/update/history") });
            cx.spawn(async move |this, cx| {
                let result = job.await;
                let _ = this.update(cx, |this, cx| {
                    if let (Some(sheet), Ok(history)) = (this.sheet.as_mut(), result) {
                        sheet.history = Some(history);
                        cx.notify();
                    }
                });
            })
            .detach();
        }
        cx.notify();
    }

    /// Version juste installée (résultat écrit par l'installation) : la fenêtre se rouvre une fois sur « installé ».
    pub(crate) fn note_installed(&mut self, update: &Value, cx: &mut Context<Self>) {
        let done = &update["installed"];
        if self.installed_seen || done["ok"] != true || done["version"].as_str() != Some(crate::VERSION) {
            return;
        }
        self.installed_seen = true;
        let mut sheet = Sheet::new(SheetKind::Done);
        sheet.from = done["from_version"].as_str().map(str::to_owned);
        sheet.downgrade = sheet.from.as_deref().is_some_and(|f| rustudio_core::semver::lt(crate::VERSION, f));
        self.sheet = Some(sheet);
        cx.notify();
    }

    pub(crate) fn close_update_sheet(&mut self, cx: &mut Context<Self>) {
        if self.sheet.as_ref().is_some_and(|s| s.kind == SheetKind::Done) && self.client.is_live() {
            // l'installation est lue : elle ne se rouvrira plus à la prochaine relance
            let client = self.client.clone();
            cx.background_executor().spawn(async move { client.post_module("/api", "update-ack", json!({})) }).detach();
        }
        self.sheet = None;
        ui::reset_hover();
        cx.notify();
    }

    fn sheet_install(&mut self, cx: &mut Context<Self>) {
        if let Some(sheet) = self.sheet.as_mut() {
            sheet.shown = 0;
            sheet.shown_at = Instant::now();
        }
        self.update_post("update-install", cx);
    }

    /// Fait avancer l'étape affichée d'une seule étape à la fois, chacune visible STEP_MIN au moins.
    pub(crate) fn sheet_tick(&mut self, cx: &mut Context<Self>) {
        if !self.sheet_preview_done {
            self.sheet_preview_done = true;
            self.sheet_preview();
        }
        let phase = self.update.as_ref().and_then(|u| u["phase"].as_str()).unwrap_or("").to_owned();
        let Some(sheet) = self.sheet.as_mut() else { return };
        let target = match sheet.kind {
            SheetKind::Done => 4,
            SheetKind::Update => match step_of(&phase) {
                Some(step) => step,
                None => return,
            },
            SheetKind::Current => return,
        };
        if sheet.shown >= target {
            return;
        }
        let waited = sheet.shown_at.elapsed();
        if waited >= STEP_MIN {
            sheet.shown += 1;
            sheet.shown_at = Instant::now();
        }
        if sheet.shown < target || waited < STEP_MIN {
            let wait = STEP_MIN.saturating_sub(sheet.shown_at.elapsed()) + Duration::from_millis(16);
            cx.spawn(async move |this, cx| {
                cx.background_executor().timer(wait).await;
                let _ = this.update(cx, |_, cx| cx.notify());
            })
            .detach();
        }
    }

    /// RUSTUDIO_PREVIEW_SHEET (développement, captures) : ouvre la fenêtre dans un état donné, sans moteur réel.
    /// update, downloading, failed, done, current.
    fn sheet_preview(&mut self) {
        let Ok(state) = std::env::var("RUSTUDIO_PREVIEW_SHEET") else { return };
        let notes = notes::changelog(CHANGELOG);
        let latest = notes.first().map(|e| e.version.clone()).unwrap_or_else(|| crate::VERSION.to_owned());
        let composed = notes.iter().take(4).enumerate().map(|(i, e)| if i == 0 { e.notes.clone() } else { format!("#### Aussi dans la {}\n\n{}", e.version, e.notes) }).collect::<Vec<_>>().join("\n\n");
        let previous = notes.get(4).map(|e| e.version.clone()).unwrap_or_else(|| "6.1.4".into());
        let phase = match state.as_str() {
            "downloading" => json!("downloading"),
            "failed" => json!("failed"),
            _ => Value::Null,
        };
        self.update = Some(json!({"current": previous, "release_build": true, "status": "available", "installable": true, "phase": phase,
            "progress": 0.42, "error": {"msg": "Fichier incomplet : la taille ne correspond pas à celle annoncée par GitHub."},
            "latest": {"version": latest, "notes": composed, "published": "2026-11-13T10:00:00Z", "url": "https://github.com/samirmedani/rustudio/releases",
                       "asset": {"size": 6_639_453}}}));
        let kind = match state.as_str() {
            "done" => SheetKind::Done,
            "current" => SheetKind::Current,
            _ => SheetKind::Update,
        };
        let mut sheet = Sheet::new(kind);
        if kind == SheetKind::Done {
            sheet.from = Some(previous);
        }
        self.sheet = Some(sheet);
    }

    /// Versions à présenter, de la plus récente à la plus ancienne.
    fn sheet_cards(&self, sheet: &Sheet, u: &Value) -> Vec<Card> {
        if sheet.kind != SheetKind::Update {
            let entry = notes::changelog(CHANGELOG).into_iter().find(|e| e.version == crate::VERSION);
            return vec![Card {
                version: crate::VERSION.to_owned(),
                date: entry.as_ref().and_then(|e| e.date.clone()),
                notes: entry.map(|e| e.notes).unwrap_or_default(),
            }];
        }
        let current = u["current"].as_str().unwrap_or(crate::VERSION);
        if let Some(rows) = sheet.history.as_ref().and_then(|h| h["versions"].as_array()) {
            let cards: Vec<Card> = rows
                .iter()
                .filter(|r| r["version"].as_str().is_some_and(|v| rustudio_core::semver::lt(current, v)))
                .take(MAX_CARDS)
                .map(|r| Card {
                    version: r["version"].as_str().unwrap_or("").to_owned(),
                    date: r["published"].as_str().and_then(long_date),
                    notes: r["notes"].as_str().unwrap_or("").to_owned(),
                })
                .collect();
            if !cards.is_empty() {
                return cards;
            }
        }
        let latest = &u["latest"];
        notes::split_composed(latest["notes"].as_str().unwrap_or(""))
            .into_iter()
            .take(MAX_CARDS)
            .map(|(v, n)| Card {
                date: if v.is_none() { latest["published"].as_str().and_then(long_date) } else { None },
                version: v.unwrap_or_else(|| latest["version"].as_str().unwrap_or("").to_owned()),
                notes: n,
            })
            .collect()
    }

    pub(crate) fn update_sheet_view(&self, cx: &mut Context<Self>) -> Option<Div> {
        let sheet = self.sheet.as_ref()?;
        let u = self.update.clone().unwrap_or(Value::Null);
        let cards = self.sheet_cards(sheet, &u);
        let version = match sheet.kind {
            SheetKind::Update => u["latest"]["version"].as_str().unwrap_or("").to_owned(),
            _ => crate::VERSION.to_owned(),
        };
        let phase = u["phase"].as_str().unwrap_or("");
        let installing = sheet.kind == SheetKind::Update && step_of(phase).is_some();
        let failed = sheet.kind == SheetKind::Update && phase == "failed";
        let current = u["current"].as_str().unwrap_or(crate::VERSION).to_owned();

        // en-tête : version, étiquette, ce qu'on a aujourd'hui
        let (badge, colour) = if sheet.kind == SheetKind::Update { ("Nouvelle version", ui::ACCENT) } else { ("Installée", ui::GREEN) };
        let sub = match sheet.kind {
            SheetKind::Update => {
                let more = cards.len().saturating_sub(1);
                if more > 0 { format!("Tu as la {current} · {} versions à rattraper", cards.len()) } else { format!("Tu as la {current}") }
            }
            SheetKind::Current => cards.first().and_then(|c| c.date.clone()).unwrap_or_default(),
            SheetKind::Done => match &sheet.from {
                Some(from) if sheet.downgrade => format!("Retour depuis la {from}"),
                Some(from) => format!("Mise à jour depuis la {from}"),
                None => String::new(),
            },
        };
        let header = div()
            .flex()
            .items_center()
            .gap(px(14.))
            .child(ui::logo(46.))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(5.))
                    .child(ui::title(format!("Rustudio {version}"), 19.))
                    .child(div().flex().items_center().gap(px(8.)).child(ui::badge(badge, colour)).child(ui::caption(sub))),
            );
        let lead = cards.first().and_then(|c| notes::parse(&c.notes).lead);

        // notes, une carte par version
        let mut list = div().flex().flex_col().gap(px(10.));
        let hidden = cards.len().saturating_sub(OPEN_CARDS);
        for (i, card) in cards.iter().enumerate() {
            if i >= OPEN_CARDS && !sheet.show_older {
                break;
            }
            list = list.child(release_card(card, i > 0, cards.len() > 1));
        }
        if hidden > 0 && !sheet.show_older {
            list = list.child(
                ui::pill("update-older", Some(Icon::ChevronDown), format!("Afficher les {hidden} versions précédentes"), Tone::Plain).on_click(cx.listener(|this, _, _, cx| {
                    if let Some(s) = this.sheet.as_mut() {
                        s.show_older = true;
                    }
                    cx.notify();
                })),
            );
        }
        if cards.iter().all(|c| c.notes.trim().is_empty()) {
            list = div().child(ui::hint("Nouveautés indisponibles pour l’instant : vérifie ta connexion."));
        }
        let scroll = div().id("update-notes").max_h(px(340.)).overflow_y_scroll().pr(px(6.)).child(list);

        let mut root = div().flex().flex_col().gap(px(16.)).p(px(24.)).rounded(px(22.)).child(header);
        if let Some(lead) = lead {
            root = root.child(div().text_size(px(15.)).line_height(px(21.)).font_weight(FontWeight::MEDIUM).child(lead));
        }
        root = root.child(scroll);

        // pied : ce que fait le clic, l'avancement, ou la raison d'un échec
        let close = |label: &'static str| ui::pill("update-close", None, label, Tone::Plain).on_click(cx.listener(|this, _, _, cx| this.close_update_sheet(cx)));
        let repo = u["repo_url"].as_str().unwrap_or("https://github.com/samirmedani/rustudio").to_owned();
        let release_page = match sheet.kind {
            SheetKind::Update => u["latest"]["url"].as_str().or(u["release_page"].as_str()).map(str::to_owned).unwrap_or_else(|| format!("{repo}/releases")),
            _ => format!("{repo}/releases/tag/v{}", crate::VERSION),
        };
        let github = {
            let url = release_page.clone();
            ui::pill("update-github", Some(Icon::External), "Voir sur GitHub", Tone::Plain).on_click(cx.listener(move |_, _, _, cx| cx.open_url(&url)))
        };
        let buttons = div().flex().items_center().gap(px(8.));
        let footer: AnyElement = if sheet.kind == SheetKind::Done || installing {
            let done = sheet.kind == SheetKind::Done && sheet.shown >= 4;
            let detail: SharedString = if done {
                if sheet.downgrade { format!("Retour à la {version} terminé.") } else { format!("Rustudio {version} est installé.") }.into()
            } else {
                match sheet.shown {
                    0 => {
                        let p = u["progress"].as_f64().unwrap_or(0.);
                        match u["latest"]["asset"]["size"].as_u64() {
                            Some(size) => format!("Téléchargement · {} % de {}", (p * 100.).round(), megabytes(size)).into(),
                            None => format!("Téléchargement · {} %", (p * 100.).round()).into(),
                        }
                    }
                    1 => "Vérification de la taille et de l’empreinte SHA-256…".into(),
                    2 => "Installation… Rustudio va se fermer, puis se rouvrir tout seul.".into(),
                    _ => "Redémarrage…".into(),
                }
            };
            let mut col = div().flex().flex_col().gap(px(10.)).child(steps(sheet.shown));
            col = col.child(div().text_size(px(13.5)).font_weight(if done { FontWeight::SEMIBOLD } else { FontWeight::NORMAL }).text_color(rgb(if done { ui::GREEN_TEXT } else { ui::TEXT2 })).child(detail));
            if sheet.shown == 0 && installing {
                col = col.child(ui::progress(u["progress"].as_f64().unwrap_or(0.) as f32, ui::ACCENT));
            }
            let cancellable = matches!(phase, "downloading" | "verifying" | "extracting");
            let mut row = buttons.child(div().flex_1());
            if installing {
                row = row.child(close("Masquer"));
                if cancellable {
                    row = row.child(ui::pill("update-cancel", Some(Icon::Close), "Annuler", Tone::Plain).on_click(cx.listener(|this, _, _, cx| this.update_post("update-cancel", cx))));
                }
            } else {
                row = row.child(ui::pill("update-close", None, "Fermer", Tone::Primary).on_click(cx.listener(|this, _, _, cx| this.close_update_sheet(cx))));
            }
            col.child(row).into_any_element()
        } else if failed {
            let reason = u["error"]["msg"].as_str().unwrap_or("Mise à jour interrompue.").to_owned();
            let url = release_page.clone();
            div()
                .flex()
                .flex_col()
                .gap(px(12.))
                .child(ui::notice(false, format!("Échec de la mise à jour. {reason} Rien n’a été remplacé.")))
                .child(
                    buttons
                        .child(ui::pill("update-page", Some(Icon::External), "Ouvrir la page de la release", Tone::Plain).on_click(cx.listener(move |_, _, _, cx| cx.open_url(&url))))
                        .child(div().flex_1())
                        .child(close("Fermer"))
                        .child(ui::pill("update-retry", Some(Icon::Refresh), "Réessayer", Tone::Primary).on_click(cx.listener(|this, _, _, cx| this.sheet_install(cx)))),
                )
                .into_any_element()
        } else if sheet.kind == SheetKind::Current {
            buttons.child(github).child(div().flex_1()).child(ui::pill("update-close", None, "Fermer", Tone::Primary).on_click(cx.listener(|this, _, _, cx| this.close_update_sheet(cx)))).into_any_element()
        } else if u["installable"] == true {
            let size = u["latest"]["asset"]["size"].as_u64().map(|s| format!(" ({})", megabytes(s))).unwrap_or_default();
            div()
                .flex()
                .flex_col()
                .gap(px(12.))
                .child(ui::hint(format!("Rustudio télécharge cette version{size} depuis GitHub, vérifie sa taille et son empreinte, puis redémarre tout seul. Tes réglages et tes profils restent.")).text_size(px(12.5)))
                .child(
                    buttons
                        .child(github)
                        .child(div().flex_1())
                        .child(close("Plus tard"))
                        .child(ui::pill("update-install", Some(Icon::Download), "Mettre à jour", Tone::Primary).on_click(cx.listener(|this, _, _, cx| this.sheet_install(cx)))),
                )
                .into_any_element()
        } else {
            let why = match u["why_not"].as_str() {
                Some("readonly") => "Rustudio ne peut pas se remplacer là où il est installé : la nouvelle version s’installe depuis la page de la release.",
                Some("dev") => "Version de développement : la nouvelle version s’installe depuis la page de la release.",
                _ => "Cette version s’installe depuis la page de la release.",
            };
            let url = release_page.clone();
            div()
                .flex()
                .flex_col()
                .gap(px(12.))
                .child(ui::hint(why).text_size(px(12.5)))
                .child(
                    buttons
                        .child(div().flex_1())
                        .child(close("Plus tard"))
                        .child(ui::pill("update-page", Some(Icon::External), "Ouvrir la page de la release", Tone::Primary).on_click(cx.listener(move |_, _, _, cx| cx.open_url(&url)))),
                )
                .into_any_element()
        };
        Some(root.child(div().pt(px(4.)).border_t_1().border_color(alpha(ui::INK, 0.07)).child(div().pt(px(14.)).child(footer))))
    }
}

/// Une version : numéro et date, accroche (sauf la première, déjà en titre), rubriques en couleur.
fn release_card(card: &Card, with_lead: bool, with_header: bool) -> Div {
    let parsed = notes::parse(&card.notes);
    let mut col = div().flex().flex_col().gap(px(10.)).p(px(16.)).rounded(px(16.)).bg(alpha(ui::INK, 0.035));
    if with_header {
        col = col.child(
            div()
                .flex()
                .items_center()
                .gap(px(8.))
                .child(div().text_size(px(14.)).font_weight(FontWeight::SEMIBOLD).child(card.version.clone()))
                .child(ui::caption(card.date.clone().unwrap_or_default())),
        );
    }
    if with_lead {
        if let Some(lead) = parsed.lead.as_ref() {
            col = col.child(ui::hint(lead.clone()));
        }
    }
    for section in parsed.sections {
        let (glyph, colour) = section_style(section.kind);
        if !section.label.is_empty() {
            col = col.child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .mt(px(2.))
                    .child(ui::chip(glyph, colour, 22.))
                    .child(div().text_size(px(13.)).font_weight(FontWeight::SEMIBOLD).text_color(rgb(if colour == ui::GREEN { ui::GREEN_TEXT } else if colour == ui::ORANGE { ui::ORANGE_TEXT } else { colour })).child(section.label.clone())),
            );
        }
        for item in section.items {
            let mut text = div().flex().flex_col().flex_1().min_w(px(0.)).gap(px(2.));
            if let Some(title) = item.title {
                text = text.child(div().text_size(px(13.)).font_weight(FontWeight::SEMIBOLD).child(title));
            }
            text = text.child(ui::rich_text(&item.detail).text_color(rgb(ui::TEXT2)));
            col = col.child(div().flex().gap(px(10.)).child(div().flex_none().mt(px(7.)).size(px(5.)).rounded_full().bg(rgb(colour))).child(text));
        }
    }
    col
}

/// Quatre étapes numérotées : faites cochées en vert, en cours en couleur, à venir en gris.
fn steps(shown: usize) -> Div {
    let mut row = div().flex().items_start();
    for (i, label) in STEPS.iter().enumerate() {
        let done = i < shown;
        let active = i == shown;
        let dot = div()
            .flex()
            .flex_none()
            .items_center()
            .justify_center()
            .size(px(26.))
            .rounded_full()
            .text_size(px(12.5))
            .font_weight(FontWeight::SEMIBOLD)
            .when(done, |d| d.bg(rgb(ui::GREEN)).child(ui::icon(Icon::Check, 14., rgb(ui::WHITE))))
            .when(active, |d| d.bg(rgb(ui::ACCENT)).text_color(rgb(ui::WHITE)).child((i + 1).to_string()))
            .when(!done && !active, |d| d.border_2().border_color(alpha(ui::INK, 0.14)).text_color(rgb(ui::TEXT3)).child((i + 1).to_string()));
        let colour = if done { ui::GREEN_TEXT } else if active { ui::TEXT } else { ui::TEXT3 };
        row = row.child(
            div()
                .flex()
                .flex_col()
                .flex_1()
                .items_center()
                .gap(px(6.))
                .child(dot)
                .child(div().text_size(px(12.)).font_weight(if active { FontWeight::SEMIBOLD } else { FontWeight::NORMAL }).text_color(rgb(colour)).child(*label)),
        );
    }
    row
}
