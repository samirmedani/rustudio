use crate::{
    backend::Client,
    desktop::{hint, panel},
    mixer::{Mixer, button},
};
use gpui::{Context, Div, div, prelude::*};
use rustudio_windows_desktop::{audio, osd, soundcheck};
use serde_json::{Value, json};

pub fn inventory(live: bool) -> Value {
    if live {
        let mut v = audio::inventory();
        v["issues"] = audio::status()["issues"].clone();
        return v;
    }
    json!({"status":"ok","outputs":[
        {"name":"Casque de démonstration","default":true,"communications":false},
        {"name":"Sortie de démonstration","default":false,"communications":true}],
        "inputs":[{"name":"Micro de démonstration","default":true,"communications":true}]})
}

#[derive(Default)]
pub struct WinPage {
    pub inventory: Option<Value>,
    busy: bool,
    confirm: bool,
    note: Option<String>,
}
impl WinPage {
    pub fn cancel(&mut self) {
        self.confirm = false;
    }
}

impl Mixer {
    pub fn win_audio(&self, cx: &mut Context<Self>) -> Div {
        let mut root = div().flex().flex_col().gap_3();
        if let Some(note) = &self.win.note {
            root = root.child(hint(note.clone()));
        }
        for issue in self.win.inventory.as_ref().and_then(|v| v["issues"].as_array()).into_iter().flatten() {
            let name = issue["name"].as_str().unwrap_or("un autre périphérique");
            let role = if issue["role"] == "communications" { "pour les appels" } else { "par défaut" };
            let text = match issue["kind"].as_str() {
                Some("capture-loop") => format!("Windows enregistre « {name} » {role} au lieu de « Chat Mic » : tes amis entendent le stream en écho."),
                Some("capture-other") => format!("Le micro de Windows {role} est « {name} » : la GoXLR n’est pas utilisée."),
                Some("capture-none") => format!("Aucun micro Windows {role}."),
                Some("render-other") => format!("Le son {role} part sur « {name} » au lieu de la GoXLR."),
                _ => format!("Aucune sortie Windows {role}."),
            };
            root = root.child(crate::ui::notice(false, text));
        }
        if let Some(data) = self.win.inventory.as_ref().filter(|v| v["status"] == "ok") {
            for (key, label) in [("outputs", "Sorties audio"), ("inputs", "Entrées audio")] {
                let mut group = panel().child(crate::ui::heading(label));
                for row in data[key].as_array().into_iter().flatten() {
                    let name = row["name"].as_str().unwrap_or("Périphérique audio");
                    let roles = match (row["default"] == true, row["communications"] == true) {
                        (true, true) => "Par défaut · Appels",
                        (true, false) => "Par défaut",
                        (false, true) => "Appels",
                        _ => "",
                    };
                    group = group.child(
                        div()
                            .flex()
                            .justify_between()
                            .gap_2()
                            .child(div().child(name.to_owned()))
                            .child(hint(roles)),
                    );
                }
                if data[key].as_array().is_none_or(Vec::is_empty) {
                    group = group.child(hint("Aucun périphérique actif."));
                }
                root = root.child(group);
            }
        } else {
            root = root.child(panel().child(hint(
                "Inventaire audio indisponible. Nouvelle lecture automatique.",
            )));
        }
        if self.client.is_live() {
            root = root.child(
                div()
                    .flex()
                    .gap_2()
                    .child(
                        button(
                            "windows-open".into(),
                            "Ouvrir les paramètres de son".into(),
                            false,
                        )
                        .on_click(cx.listener(|_, _, _, cx| cx.open_url("ms-settings:sound"))),
                    )
                    .when(!self.win.busy, |d| {
                        d.child(
                            button(
                                "windows-fix".into(),
                                "Rétablir les défauts GoXLR".into(),
                                false,
                            )
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.win.confirm = true;
                                cx.notify();
                            })),
                        )
                    }),
            );
        }
        if self.win.confirm {
            root=root.child(panel().child(hint("Utiliser les entrées et sorties GoXLR recommandées par défaut dans Windows ?"))
                .child(div().flex().gap_2()
                    .child(button("windows-fix-ok".into(),"Confirmer".into(),true)
                        .on_click(cx.listener(|this,_,_,cx|this.fix_audio(cx))))
                    .child(button("windows-fix-cancel".into(),"Annuler".into(),false)
                        .on_click(cx.listener(|this,_,_,cx|{this.win.cancel();cx.notify();})))));
        }
        root
    }

    fn fix_audio(&mut self, cx: &mut Context<Self>) {
        if !self.client.is_live() || self.win.busy {
            return;
        }
        self.win.confirm = false;
        self.win.busy = true;
        let work = cx
            .background_executor()
            .spawn(async move { (audio::fix(), audio::inventory()) });
        cx.spawn(async move |this, cx| {
            let (n, state) = work.await;
            let _ = this.update(cx, |this, cx| {
                this.win.busy = false;
                this.win.inventory = Some(state);
                this.win.note = Some(match n {
                    -1 => "Windows ne voit pas les voies nécessaires de la GoXLR.".into(),
                    0 => "Les périphériques par défaut sont déjà corrects.".into(),
                    _ => format!("{n} réglage(s) audio mis à jour."),
                });
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }
}

pub fn sound_test(client: &Client) -> Result<Value, String> {
    if !client.is_live() {
        return Ok(json!({"simulated":true,"scan":{"channels":[
            {"name":"Entrée de démonstration","status":"ok","peak_db":-18.0},
            {"name":"Sortie de démonstration","status":"ok","peak_db":-24.0}]},
            "loop":{"status":"ok","dropouts":0,"jumps":0}}));
    }
    client.live_state()?;
    let route = client.post_module(
        "/api/modules/goxlr-mini",
        "sound-test",
        json!({"seconds":22}),
    )?;
    if route["ok"] != true {
        return Err("Le moteur n’a pas isolé la voie Samples.".into());
    }
    // La durée côté moteur assure aussi le retour au routage normal si l’application se ferme.
    let loop_result = soundcheck::loop_test(5., false);
    let restored = client.post_module(
        "/api/modules/goxlr-mini",
        "sound-test",
        json!({"seconds":0}),
    );
    restored?;
    let scan = soundcheck::run(5.);
    Ok(json!({"simulated":false,"scan":scan,"loop":loop_result}))
}

pub fn show_mic(state: &str) {
    let muted = !state.is_empty();
    let subtitle = match state {
        "stream" => "Coupé du stream",
        "chat" => "Coupé de Discord",
        "phones" => "Coupé du casque",
        "lineout" => "Coupé des enceintes",
        "all" => "Toutes les sorties",
        _ => "Micro ouvert",
    };
    osd::show(osd::Notice {
        title: if muted { "Micro coupé" } else { "Micro actif" }.into(),
        subtitle: subtitle.into(),
        rgb: if muted { 0xff3b30 } else { 0x34c759 },
        icon: if muted { "mic-off" } else { "mic" },
    });
}
