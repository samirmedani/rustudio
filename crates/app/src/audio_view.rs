//! Son Windows : noms expliqués, choix conseillés et visualisation de la source choisie.
use crate::{
    mixer::Mixer,
    ui::{self, Icon, Tone, alpha, rgb},
};
use gpui::{Context, Div, SharedString, canvas, div, prelude::*, px};
use rustudio_windows_desktop::spectrum::{FLOOR, Frame, Source};
use serde_json::Value;

/// Les voies virtuelles GoXLR sont des destinations des applis, pas des prises supplémentaires.
fn explanation(name: &str, output: bool) -> (&str, &'static str) {
    if name.to_lowercase().contains("goxlr") {
        let channel = name.split('(').next().unwrap_or(name).trim().to_lowercase();
        match (output, channel.as_str()) {
            (true, "system") => {
                return (
                    "Son du PC",
                    "Le choix simple pour le son de Windows, du navigateur et des applis courantes.",
                );
            }
            (true, "chat") => {
                return (
                    "Voix de tes amis",
                    "À choisir comme sortie dans Discord ou les applis d’appel. Tu règles leurs voix séparément.",
                );
            }
            (true, "game") => {
                return (
                    "Son des jeux",
                    "À choisir dans ton jeu pour lui réserver son propre réglage de volume.",
                );
            }
            (true, "music") => {
                return (
                    "Musique",
                    "À choisir pour ton lecteur de musique, si tu veux régler son volume à part.",
                );
            }
            (true, "sample" | "samples") => {
                return (
                    "Sons et effets",
                    "Pour les bruitages, jingles et sons courts envoyés par tes applis.",
                );
            }
            (false, "chat mic") => {
                return (
                    "Ton micro",
                    "Le bon choix comme micro dans Windows et Discord. Tes amis reçoivent la voix que tu y envoies.",
                );
            }
            (false, "broadcast stream mix" | "stream mix" | "stream mix 1") => {
                return (
                    "Mix pour le stream",
                    "À choisir dans OBS : ce mix reçoit les sources cochées vers le stream dans le routage Rustudio. À éviter comme micro Discord.",
                );
            }
            (false, "vod" | "stream mix 2") => {
                return (
                    "Mix pour les rediffusions",
                    "Un second mix pour l’enregistrement ou la piste VOD d’OBS. Vérifie son routage : le nom ne garantit pas que la musique est exclue.",
                );
            }
            _ => {}
        }
    }
    let lower = name.to_lowercase();
    if !output && (lower.contains("webcam") || lower.contains("c920")) {
        return (
            name,
            "Le micro intégré à la webcam. Choisis-le seulement si tu veux parler avec ce micro.",
        );
    }
    if output && lower.contains("spdif") {
        return (
            name,
            "Sortie numérique vers un appareil relié par câble optique ou coaxial. Choisis-la seulement si c’est cette connexion que tu utilises.",
        );
    }
    (
        name,
        if output {
            "Cette sortie reçoit le son des applis. Choisis celle reliée au casque ou aux enceintes que tu utilises."
        } else {
            "Cette entrée fournit du son aux applis. Choisis-la si c’est bien le micro ou la source que tu veux envoyer."
        },
    )
}
fn db_text(db: f32) -> String {
    if db <= FLOOR {
        "Silence".into()
    } else {
        format!("{db:.0} dB")
    }
}
fn frequency_axis() -> Div {
    let mut axis = div().relative().w_full().h(px(17.));
    for (hz, label) in [
        (20., "20 Hz"),
        (100., "100 Hz"),
        (1000., "1 kHz"),
        (10000., "10 kHz"),
        (20000., "20 kHz"),
    ] {
        let p = ((hz / 20f64).ln() / 1000f64.ln()) as f32;
        let text = ui::caption(label).absolute();
        axis = axis.child(if hz == 20000. {
            text.right_0()
        } else {
            text.left(gpui::relative(p))
                .ml(px(if hz == 20. { 0. } else { -18. }))
        });
    }
    axis
}

fn plot(bands: Vec<f32>, height: f32, enabled: bool) -> Div {
    div()
        .w_full()
        .min_w(px(0.))
        .h(px(height))
        .rounded(px(12.))
        .bg(alpha(ui::INK, 0.035))
        .child(
            canvas(
                |_, _, _| (),
                move |bounds, _, window, _| {
                    let width = f32::from(bounds.size.width);
                    let h = f32::from(bounds.size.height);
                    for n in 1..4 {
                        let y = h * n as f32 / 4.;
                        window.paint_quad(gpui::fill(
                            gpui::Bounds::new(
                                bounds.origin + gpui::point(px(0.), px(y)),
                                gpui::size(bounds.size.width, px(1.)),
                            ),
                            alpha(ui::INK, 0.07),
                        ));
                    }
                    let step = width / bands.len().max(1) as f32;
                    for (i, db) in bands.iter().enumerate() {
                        let level = if enabled {
                            ((*db - FLOOR) / -FLOOR).clamp(0., 1.)
                        } else {
                            0.
                        };
                        let bh = (level * (h - 8.)).max(2.);
                        let colour = if !enabled || *db <= FLOOR {
                            alpha(ui::INK, 0.1)
                        } else {
                            rgb(ui::ACCENT)
                        };
                        window.paint_quad(
                            gpui::fill(
                                gpui::Bounds::new(
                                    bounds.origin
                                        + gpui::point(px(i as f32 * step + 2.), px(h - bh)),
                                    gpui::size(px((step - 3.).max(1.)), px(bh)),
                                ),
                                colour,
                            )
                            .corner_radii(gpui::Corners::all(px(2.))),
                        );
                    }
                },
            )
            .size_full(),
        )
}

impl Mixer {
    pub(crate) fn choose_audio(&mut self, source: Source, cx: &mut Context<Self>) {
        self.win.selected = Some(source);
        self.win.frame = Frame::default();
        self.win.sync_monitor();
        cx.notify();
    }
    pub(crate) fn audio_guide(&self, wide: bool, cx: &mut Context<Self>) -> Div {
        let goxlr = self.win.inventory.as_ref().is_some_and(|d| {
            ["outputs", "inputs"].iter().any(|k| {
                d[*k].as_array().into_iter().flatten().any(|r| {
                    r["name"]
                        .as_str()
                        .is_some_and(|s| s.to_lowercase().contains("goxlr"))
                })
            })
        });
        let mut card = ui::card().w_full().min_w(px(0.)).gap(px(9.)).child(
            div()
                .flex()
                .items_center()
                .gap(px(10.))
                .child(
                    ui::card_title(Some((Icon::Help, ui::ACCENT)), "Les choix conseillés", None)
                        .flex_1(),
                )
                .child(
                    ui::pill(
                        "audio-guide-toggle",
                        Some(Icon::ChevronDown),
                        if self.win.guide_hidden {
                            "Afficher"
                        } else {
                            "Réduire"
                        },
                        Tone::Plain,
                    )
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.win.guide_hidden = !this.win.guide_hidden;
                        cx.notify();
                    })),
                ),
        );
        if self.win.guide_hidden {
            return card;
        }
        if goxlr {
            card=card.child(ui::hint("Avec ta GoXLR, pars de ces choix. Game, Music et Sample servent ensuite à séparer tes volumes."));
            let mut tiles = vec![];
            for (title, choice) in [
                ("Son de Windows", "System"),
                ("Discord · sortie", "Chat"),
                ("Windows et Discord · micro", "Chat Mic"),
                ("OBS · son du stream", "Broadcast Stream Mix / Stream Mix 1"),
            ] {
                tiles.push(
                    div()
                        .min_w(px(0.))
                        .flex()
                        .flex_col()
                        .gap(px(3.))
                        .child(ui::caption(title))
                        .child(
                            div()
                                .text_size(px(14.))
                                .font_weight(gpui::FontWeight::SEMIBOLD)
                                .child(choice),
                        )
                        .into_any_element(),
                );
            }
            card=card.child(div().grid().grid_cols(if wide{4}else{2}).gap(px(12.)).children(tiles))
                .child(ui::caption("Dans OBS, capture ce mix une seule fois : ajouter en plus le même son du bureau ou le même micro peut doubler le son."));
        } else {
            card=card.child(ui::hint("Sortie : le casque ou les enceintes où tu veux entendre le PC. Entrée : le micro avec lequel tu veux parler. « Son principal » et « Entrée principale » sont les choix généraux de Windows ; « Appels » sert aux applis de conversation qui suivent ce réglage."));
        }
        card
    }
    pub(crate) fn audio_spectrum(&self, cx: &mut Context<Self>) -> Div {
        let mut sources = vec![];
        let mut names = vec![];
        let mut title = "Choisis une source".to_owned();
        if let Some(data) = &self.win.inventory {
            for (key, output) in [("outputs", true), ("inputs", false)] {
                for row in data[key].as_array().into_iter().flatten() {
                    let (Some(id), Some(name)) = (row["id"].as_str(), row["name"].as_str()) else {
                        continue;
                    };
                    let s = Source {
                        id: id.into(),
                        output,
                    };
                    let selected = self.win.selected.as_ref() == Some(&s);
                    let human = explanation(name, output).0;
                    let label = format!("{} · {}", if output { "Sortie" } else { "Entrée" }, human);
                    if selected {
                        title = label.clone();
                    }
                    names.push((label, selected));
                    sources.push(s);
                }
            }
        }
        let select = self
            .dropdown(
                "audio-spectrum-source",
                title,
                names,
                !sources.is_empty(),
                move |this, i, cx| {
                    this.choose_audio(sources[i].clone(), cx);
                },
                cx,
            )
            .w_full()
            .flex()
            .flex_col();
        let frame = &self.win.frame;
        let enabled = !self.win.paused && frame.ready;
        let status = if self.win.paused {
            "Analyse en pause"
        } else if frame.error.is_some() {
            "Analyse indisponible"
        } else if !frame.ready {
            "Ouverture de la source…"
        } else if !self.client.is_live() {
            "Démonstration · signal simulé"
        } else {
            "En direct"
        };
        let mut card=ui::card().w_full().min_w(px(0.)).gap(px(10.)).child(div().flex().flex_wrap().items_center().gap(px(10.)).child(ui::title("Le son en direct",17.)).child(ui::badge(status,if frame.error.is_some(){ui::ORANGE}else{ui::ACCENT})).child(div().flex_1()).child(ui::pill("audio-spectrum-pause",Some(if self.win.paused{Icon::Play}else{Icon::Stop}),if self.win.paused{"Reprendre"}else{"Pause"},Tone::Plain).on_click(cx.listener(|this,_,_,cx|{this.win.paused = !this.win.paused; this.win.sync_monitor(); cx.notify();}))))
            .child(select).child(ui::hint("Graves à gauche, aigus à droite. Plus une barre monte, plus cette fréquence est présente."))
            .child(plot(frame.bands.clone(),120.,enabled))
            .child(frequency_axis())
            .child(div().flex().flex_wrap().items_center().gap(px(12.)).child(ui::badge(if enabled{db_text(frame.peak_db)}else{"—".into()},if frame.peak_db>=-1.{ui::ORANGE}else{ui::GREEN})).child(ui::caption(if frame.ready&&frame.peak_db<=-78.{if self.win.selected.as_ref().is_some_and(|s|s.output){"Signal faible ou absent : lance un son sur cette sortie."}else{"Signal faible ou absent : parle ou envoie du son sur cette entrée."}}else{"Les dB mesurent le signal numérique, pas le volume réel de ton casque."})))
            .child(ui::caption("L’analyse reste sur ce PC, sans sauvegarde audio. Cliquer une source sert à l’observer, sans changer les choix de Windows."));
        if let Some(error) = &frame.error {
            card=card.child(ui::notice(false,format!("Windows ne peut pas lire cette source. Elle peut être occupée, débranchée ou son accès refusé. {error}")));
        }
        card
    }
    pub(crate) fn audio_endpoint(
        &self,
        row: &Value,
        output: bool,
        n: usize,
        cx: &mut Context<Self>,
    ) -> Div {
        let name = row["name"].as_str().unwrap_or("Périphérique audio");
        let (human, help) = explanation(name, output);
        let id = row["id"].as_str().unwrap_or("").to_owned();
        let source = Source {
            id: id.clone(),
            output,
        };
        let selected = self.win.selected.as_ref() == Some(&source);
        let level = self
            .win
            .frame
            .levels
            .iter()
            .find(|(s, _)| s == &id)
            .and_then(|(_, v)| *v);
        let normalized = level
            .map(|db| ((db + 60.) / 60.).clamp(0., 1.))
            .unwrap_or(0.);
        let mut badges = div().flex().flex_wrap().gap(px(5.));
        if row["default"] == true {
            badges = badges.child(ui::badge(
                if output {
                    "Son principal"
                } else {
                    "Entrée principale"
                },
                ui::GREEN,
            ));
        }
        if row["communications"] == true {
            badges = badges.child(ui::badge("Appels", ui::ACCENT));
        }
        if selected {
            badges = badges.child(ui::badge("Spectre affiché", ui::ACCENT));
        }
        let body = div()
            .id(SharedString::from(format!(
                "audio-{}-{n}",
                if output { "output" } else { "input" }
            )))
            .w_full()
            .min_w(px(0.))
            .p(px(14.))
            .flex()
            .flex_col()
            .gap(px(8.))
            .rounded(px(14.))
            .border_1()
            .border_color(alpha(ui::ACCENT, if selected { 0.55 } else { 0.05 }))
            .bg(alpha(ui::ACCENT, if selected { 0.06 } else { 0.01 }))
            .cursor(gpui::CursorStyle::PointingHand)
            .child(
                div()
                    .flex()
                    .items_start()
                    .gap(px(8.))
                    .child(ui::icon(
                        if output { Icon::Headphones } else { Icon::Mic },
                        17.,
                        rgb(ui::ACCENT),
                    ))
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(0.))
                            .child(ui::title(human.to_owned(), 14.)),
                    )
                    .child(ui::icon(
                        if selected {
                            Icon::Check
                        } else {
                            Icon::ChevronRight
                        },
                        14.,
                        rgb(ui::TEXT3),
                    )),
            )
            .when(human != name, |d| {
                d.child(ui::caption(name.to_owned()).min_w(px(0.)))
            })
            .child(badges)
            .child(ui::hint(help))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .child(
                        div()
                            .flex_1()
                            .h(px(5.))
                            .rounded_full()
                            .bg(alpha(ui::INK, 0.08))
                            .child(
                                div()
                                    .w(gpui::relative(normalized))
                                    .h_full()
                                    .rounded_full()
                                    .bg(rgb(if level.is_some_and(|v| v >= -1.) {
                                        ui::ORANGE
                                    } else {
                                        ui::GREEN
                                    })),
                            ),
                    )
                    .child(ui::caption(if self.win.paused {
                        "En pause".into()
                    } else {
                        level
                            .map(db_text)
                            .unwrap_or_else(|| "Niveau indisponible".into())
                    })),
            )
            .when(selected, |d| {
                d.child(plot(
                    self.win.frame.bands.clone(),
                    44.,
                    !self.win.paused && self.win.frame.ready,
                ))
            })
            .on_click(cx.listener(move |this, _, _, cx| {
                if !source.id.is_empty() {
                    this.choose_audio(source.clone(), cx);
                }
            }));
        div().w_full().min_w(px(0.)).child(body)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn guide_distinguishes_microphone_stream_and_driver_aliases() {
        assert_eq!(
            explanation("Chat Mic (TC-HELICON GoXLR Mini)", false).0,
            "Ton micro"
        );
        for name in ["Broadcast Stream Mix", "Stream Mix 1"] {
            let name = format!("{name} (TC-HELICON GoXLR Mini)");
            assert_eq!(explanation(&name, false).0, "Mix pour le stream");
            assert!(explanation(&name, false).1.contains("Discord"));
        }
        assert_eq!(
            explanation("VOD (GoXLR)", false).0,
            "Mix pour les rediffusions"
        );
        assert_ne!(
            explanation("Stream Mix 10 (GoXLR)", false).0,
            "Mix pour le stream"
        );
        assert_eq!(explanation("Chat (GoXLR)", true).0, "Voix de tes amis");
    }
}
