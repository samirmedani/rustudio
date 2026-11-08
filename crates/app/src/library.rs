use crate::{
    desktop::{Page, hint, panel},
    mixer::Mixer,
};
use gpui::{ClipboardItem, Context, Div, div, prelude::*};
use serde_json::Value;

#[derive(Default)]
pub struct Library {
    pub(crate) data: Option<(Page, Value)>,
    pub(crate) loading: bool,
    pub(crate) error: Option<String>,
    pub(crate) expanded: Option<String>,
    pub(crate) busy: bool,
    pub(crate) notice: Option<(bool, String)>,
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
            Page::Catalogue => {
                let mut cards: Vec<gpui::AnyElement> = vec![];
                for module in data["modules"].as_array().into_iter().flatten() {
                    let (status, state) = match module["status"].as_str() {
                        Some("official") => ("Officiel Rustudio", crate::ui::State::Info),
                        Some("community") => ("Module communautaire", crate::ui::State::Ok),
                        _ => ("À contribuer", crate::ui::State::Idle),
                    };
                    let id = module["id"].as_str().unwrap_or("");
                    let dest = match id {
                        "goxlr-mini" => Some(Page::Mixer),
                        "attack-shark-r5-ultra" => Some(Page::Mouse),
                        _ => None,
                    };
                    let glyph = if id.contains("shark") { crate::ui::Icon::Mouse } else { crate::ui::Icon::Sliders };
                    let card = crate::ui::card()
                        .size_full()
                        .child(div().flex().items_center().gap(gpui::px(12.))
                            .child(crate::ui::chip(glyph, if dest.is_some() { crate::ui::ACCENT } else { crate::ui::TEXT3 }, 36.))
                            .child(div().flex().flex_col().child(crate::ui::title(module["name"].as_str().unwrap_or("Module").to_owned(), 16.)).child(hint(module["vendor"].as_str().unwrap_or("").to_owned()))))
                        .child(crate::ui::status(status, state))
                        .child(hint(module["summary"].as_str().or(module["description"].as_str()).unwrap_or("").to_owned()))
                        .when_some(module["author"].as_str(), |c, a| c.child(crate::ui::caption(format!("Créé par {a}"))))
                        .when_some(dest, |c, dest| c.child(crate::ui::pill(gpui::ElementId::Name(format!("module-{id}").into()), None, "Ouvrir", crate::ui::Tone::Primary)
                            .on_click(cx.listener(move |this, _, _, cx| this.go_to(dest, cx)))))
                        .when(dest.is_none(), |c| c.child(crate::ui::caption("Pas encore pris en charge : la communauté peut l’ajouter.")));
                    cards.push(match dest {
                        Some(dest) => crate::ui::lift(card, gpui::ElementId::Name(format!("card-module-{id}").into()))
                            .on_click(cx.listener(move |this, _, _, cx| this.go_to(dest, cx)))
                            .into_any_element(),
                        None => card.into_any_element(),
                    });
                }
                root.child(crate::ui::columns(cards))
            }
            Page::Versions => root.child(self.versions_view(data, cx)),
            Page::Settings => root.child(self.settings_view(data, cx)),
            _ => {
                let report = serde_json::to_string_pretty(&data["report"]).unwrap_or_default();
                let card=panel().child(crate::ui::heading("Aide et retours"))
                    .child(hint("Rustudio n’envoie aucun diagnostic. Relis le rapport avant de le partager."))
                    .child(crate::ui::pill("help-copy", Some(crate::ui::Icon::Download), "Copier le diagnostic", crate::ui::Tone::Primary).on_click(cx.listener(move |_,_,_,cx|{
                        cx.write_to_clipboard(ClipboardItem::new_string(report.clone()));
                    })));
                let mut links = div().flex().flex_wrap().gap(gpui::px(8.));
                for (key, label) in [
                    ("bug_url", "Signaler un problème"),
                    ("ideas_url", "Proposer une idée"),
                    ("questions_url", "Poser une question"),
                    ("device_url", "Demander un appareil"),
                ] {
                    if let Some(url) = data[key]
                        .as_str()
                        .filter(|u| u.starts_with("https://github.com/samirmedani/rustudio/"))
                    {
                        let url = url.to_owned();
                        links = links.child(
                            crate::ui::pill(gpui::ElementId::Name(format!("help-{key}").into()), Some(crate::ui::Icon::External), label, crate::ui::Tone::Plain)
                                .on_click(cx.listener(move |_, _, _, cx| cx.open_url(&url))),
                        );
                    }
                }
                root.child(card.child(links))
            }
        }
    }
}
