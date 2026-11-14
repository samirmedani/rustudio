use crate::{
    desktop::{Page, hint},
    mixer::Mixer,
};
use crate::ui::{self, Icon, Tone, rgb};
use gpui::{ClipboardItem, Context, Div, SharedString, div, prelude::*, px};
use serde_json::Value;

#[derive(Default)]
pub struct Library {
    pub(crate) data: Option<(Page, Value)>,
    pub(crate) loading: bool,
    pub(crate) error: Option<String>,
    pub(crate) expanded: Option<String>,
    pub(crate) busy: bool,
    pub(crate) notice: Option<(bool, String)>,
    /// filtre du catalogue : 0 tous, 1 livrés, 2 à contribuer
    pub(crate) filter: u8,
    pub(crate) confirm: Option<(String, &'static str, &'static str, Value, &'static str)>,
}

impl Mixer {
    pub fn library_load(&mut self, page: Page, cx: &mut Context<Self>) {
        self.library.error = None;
        self.library.loading = true;
        let client = self.client.clone();
        let job = cx.background_executor().spawn(async move {
            if page == Page::Catalogue {
                client.get("/api/library")
            } else if matches!(page, Page::Versions | Page::Settings) {
                Mixer::settings_data(&client, page)
            } else {
                client.get("/api/support")
            }
        });
        cx.spawn(async move |this, cx| {
            let result = job.await;
            let _ = this.update(cx, |this, cx| {
                if this.page != page {
                    return;
                }
                this.library.loading = false;
                match result {
                    Ok(data) => {
                        this.after_settings_load(page, &data, cx);
                        this.library.data = Some((page, data));
                    }
                    Err(error) => this.library.error = Some(error),
                }
                cx.notify();
            });
        })
        .detach();
    }

    pub fn library_view(&self, page: Page, cx: &mut Context<Self>) -> Div {
        let mut root = div().flex().flex_col().gap_3();
        if self.library.loading {
            root = root.child(hint("Lecture…"));
        }
        if let Some(err) = self.library.error.as_ref() {
            root = root.child(hint(err.clone()));
        }
        let Some((_, data)) = self.library.data.as_ref().filter(|(p, _)| *p == page) else {
            return root;
        };
        match page {
            Page::Catalogue => root.child(self.catalogue_view(data, cx)),
            Page::Versions => root.child(self.versions_view(data, cx)),
            Page::Settings => root.child(self.settings_view(data, cx)),
            _ => {
                let report = serde_json::to_string_pretty(&data["report"]).unwrap_or_default();
                let mut tiles: Vec<gpui::AnyElement> = vec![];
                for (key, label, desc, glyph, colour) in [
                    ("bug_url", "Signaler un problème", "Un formulaire GitHub prérempli : décris ce qui ne va pas.", Icon::Alert, ui::ORANGE),
                    ("ideas_url", "Proposer une idée", "Une fonction qui te manque ? Partage-la avec la communauté.", Icon::Sparkles, 0x5856d6),
                    ("questions_url", "Poser une question", "Les discussions de la communauté Rustudio.", Icon::Help, ui::ACCENT),
                    ("device_url", "Demander un appareil", "Ton matériel n’est pas encore pris en charge ? Dis-le-nous.", Icon::Plug, ui::GREEN),
                ] {
                    if let Some(url) = data[key].as_str().filter(|u| u.starts_with("https://github.com/samirmedani/rustudio/")) {
                        let url = url.to_owned();
                        tiles.push(
                            ui::tile(gpui::ElementId::Name(format!("help-{key}").into()), Some((glyph, colour)), label, Some(desc.into()), false, None)
                                .on_click(cx.listener(move |_, _, _, cx| cx.open_url(&url)))
                                .into_any_element(),
                        );
                    }
                }
                let diag = ui::section(Icon::Download, ui::GREEN, "Diagnostic anonyme", None)
                    .child(hint("Rustudio n’envoie rien tout seul. Ce rapport ne contient ni numéro de série, ni nom de PC, ni chemin personnel : relis-le avant de le partager."))
                    .child(ui::pill("help-copy", Some(Icon::Bookmark), "Copier le diagnostic", ui::Tone::Primary).on_click(cx.listener(move |_, _, _, cx| {
                        cx.write_to_clipboard(ClipboardItem::new_string(report.clone()));
                    })));
                root.child(ui::grid(tiles, 2)).child(diag)
            }
        }
    }
}

/// Catégories du catalogue, dans l'ordre d'affichage : (kind, titre, icône).
const KINDS: [(&str, &str, Icon); 6] = [
    ("audio-interface", "Interfaces audio", Icon::Sliders),
    ("keyboard", "Claviers", Icon::Keyboard),
    ("mouse", "Souris", Icon::Mouse),
    ("headset", "Casques", Icon::Headphones),
    ("monitor", "Écrans", Icon::Monitor),
    ("other", "Autres appareils", Icon::Grid),
];

/// Texte coupé à la fin d'un mot, avec des points de suspension : tient en trois lignes sur une carte.
fn short(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_owned();
    }
    let cut: String = text.chars().take(max).collect();
    let end = cut.rfind(' ').unwrap_or(cut.len());
    format!("{}…", cut[..end].trim_end_matches([',', ';', ':', '.', ' ']))
}

/// Module livré avec Rustudio (officiel ou communautaire validé).
fn shipped(m: &Value) -> bool {
    matches!(m["status"].as_str(), Some("official" | "community"))
}

fn module_page(id: &str) -> Option<Page> {
    match id {
        "goxlr-mini" => Some(Page::Mixer),
        "attack-shark-r5-ultra" => Some(Page::Mouse),
        "cooler-master-mk770" => Some(Page::Keyboard),
        _ => None,
    }
}

impl Mixer {
    /// Catalogue : filtre, puis une section par catégorie, des cartes de même taille.
    fn catalogue_view(&self, data: &Value, cx: &mut Context<Self>) -> Div {
        let modules: Vec<&Value> = data["modules"].as_array().into_iter().flatten().collect();
        let (total_shipped, total_wanted) = (modules.iter().filter(|m| shipped(m)).count(), modules.iter().filter(|m| !shipped(m)).count());
        let filter = self.library.filter;
        let mut seg = ui::segmented();
        for (i, label) in [format!("Tous · {}", modules.len()), format!("Livrés · {total_shipped}"), format!("À contribuer · {total_wanted}")].into_iter().enumerate() {
            let i = i as u8;
            seg = seg.child(ui::seg(SharedString::from(format!("catalogue-filter-{i}")), label, filter == i).on_click(cx.listener(move |this, _, _, cx| {
                this.library.filter = i;
                cx.notify();
            })));
        }
        let mut root = div()
            .flex()
            .flex_col()
            .gap(px(22.))
            .child(div().flex().items_center().gap(px(16.)).child(div().w(px(420.)).child(seg)).child(div().flex_1()).child(
                ui::pill("catalogue-ask", Some(Icon::Plus), "Demander un appareil", Tone::Plain)
                    .on_click(cx.listener(|_, _, _, cx| cx.open_url("https://github.com/samirmedani/rustudio/issues/new?template=appareil.yml"))),
            ));
        let known: Vec<&str> = KINDS.iter().map(|k| k.0).collect();
        for (kind, title, glyph) in KINDS {
            let mut group: Vec<&Value> = modules
                .iter()
                .copied()
                .filter(|m| {
                    let k = m["kind"].as_str().unwrap_or("other");
                    k == kind || (kind == "other" && !known.contains(&k))
                })
                .filter(|m| match filter {
                    1 => shipped(m),
                    2 => !shipped(m),
                    _ => true,
                })
                .collect();
            if group.is_empty() {
                continue;
            }
            // livrés d'abord, puis par nom
            group.sort_by_key(|m| (!shipped(m), m["name"].as_str().unwrap_or("").to_lowercase()));
            let mut section = div().flex().flex_col().gap(px(12.)).child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .child(ui::chip(glyph, ui::ACCENT, 30.))
                    .child(ui::title(title, 17.))
                    .child(ui::badge(group.len().to_string(), 0x8e8e93)),
            );
            for chunk in group.chunks(3) {
                let mut row = div().flex().gap(px(16.));
                for m in chunk {
                    row = row.child(div().flex().flex_1().min_w(px(0.)).child(self.module_card(m, glyph, cx)));
                }
                for _ in chunk.len()..3 {
                    row = row.child(div().flex_1());
                }
                section = section.child(row);
            }
            root = root.child(section);
        }
        root
    }

    fn module_card(&self, m: &Value, glyph: Icon, cx: &mut Context<Self>) -> gpui::AnyElement {
        let id = m["id"].as_str().unwrap_or("").to_owned();
        let dest = module_page(&id);
        let (status, colour) = match m["status"].as_str() {
            Some("official") => ("Officiel", ui::ACCENT),
            Some("community") => ("Communautaire", ui::GREEN),
            _ => ("À contribuer", 0x8e8e93),
        };
        let mut tags = div().flex().gap(px(6.)).h(px(26.)).overflow_hidden();
        for f in m["features"].as_array().into_iter().flatten().filter_map(Value::as_str).take(3) {
            tags = tags.child(ui::tag(f.to_owned()));
        }
        let footer = match dest {
            Some(page) => ui::pill(gpui::ElementId::Name(format!("module-{id}").into()), None, "Ouvrir", Tone::Primary)
                .h(px(34.))
                .child(ui::icon(Icon::ChevronRight, 14., rgb(ui::WHITE)))
                .on_click(cx.listener(move |this, _, _, cx| this.go_to(page, cx)))
                .into_any_element(),
            None => ui::caption("La communauté peut l’ajouter").into_any_element(),
        };
        let card = ui::card()
            .flex_1()
            .min_w(px(0.))
            .h(px(236.))
            .p(px(20.))
            .gap(px(11.))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(12.))
                    .child(ui::chip(glyph, if dest.is_some() { ui::ACCENT } else { 0x8e8e93 }, 38.))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .flex_1()
                            .min_w(px(0.))
                            .child(ui::title(m["name"].as_str().unwrap_or("Module").to_owned(), 16.).truncate())
                            .child(hint(m["vendor"].as_str().unwrap_or("").to_owned())),
                    )
                    .child(ui::badge(status, colour)),
            )
            .child(div().h(px(57.)).overflow_hidden().child(hint(short(m["summary"].as_str().or(m["description"].as_str()).unwrap_or(""), 85)).text_size(px(12.5)).line_height(px(19.))))
            .child(tags)
            .child(div().flex_1())
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(ui::caption(m["author"].as_str().map(|a| format!("Créé par {a}")).unwrap_or_default()))
                    .child(footer),
            );
        match dest {
            Some(page) => ui::lift(card, gpui::ElementId::Name(format!("card-module-{id}").into()))
                .flex_1()
                .on_click(cx.listener(move |this, _, _, cx| this.go_to(page, cx)))
                .into_any_element(),
            None => card.into_any_element(),
        }
    }
}
