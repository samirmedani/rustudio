//! Page « Ma webcam » : Logitech C920 en USB.
//! Préréglages en un clic, réglages image et optique complets, contrôles XU et maintien anti-réinitialisation.
use crate::{
    backend::Client,
    desktop::Page,
    mixer::Mixer,
    ui::{self, Icon, Tone, alpha, rgb},
};
use gpui::{AnyElement, Context, Div, FontWeight, IntoElement, SharedString, div, prelude::*, px};
use serde::Deserialize;
use serde_json::{Value, json};

pub const MODULE: &str = "/api/modules/logitech-c920";

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
pub struct ControlRange {
    pub min: i32,
    pub max: i32,
    pub step: i32,
    pub default: i32,
    pub supports_auto: bool,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
pub struct Ranges {
    pub brightness: ControlRange,
    pub contrast: ControlRange,
    pub saturation: ControlRange,
    pub sharpness: ControlRange,
    pub gain: ControlRange,
    pub white_balance: ControlRange,
    pub power_line: ControlRange,
    pub exposure: ControlRange,
    pub focus: ControlRange,
    pub zoom: ControlRange,
    pub pan: ControlRange,
    pub tilt: ControlRange,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
pub struct VideoFormat {
    pub codec: String,
    pub width: u32,
    pub height: u32,
    pub max_fps: u32,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
pub struct XuUnitDesc {
    pub node_id: u32,
    pub name: String,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Eq)]
pub struct XuInfo {
    #[serde(default)]
    pub units: Vec<XuUnitDesc>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
pub struct DeviceInfo {
    pub name: String,
    pub vid: u16,
    pub pid: u16,
    pub revision: String,
    pub firmware: Option<String>,
    #[serde(default)]
    pub dsp_platform: Option<String>,
    pub revision_label: String,
    #[serde(default)]
    pub formats: Vec<VideoFormat>,
    #[serde(default)]
    pub xu: XuInfo,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
pub struct Settings {
    pub device: DeviceInfo,
    pub brightness: i32,
    pub contrast: i32,
    pub saturation: i32,
    pub sharpness: i32,
    pub gain: i32,
    pub white_balance_auto: bool,
    pub white_balance: i32,
    pub backlight_comp: bool,
    pub power_line: String,
    pub exposure_auto: bool,
    pub exposure: i32,
    pub low_light_comp: bool,
    pub focus_auto: bool,
    pub focus: i32,
    pub zoom: i32,
    pub pan: i32,
    pub tilt: i32,
    #[serde(default)]
    pub led_mode: Option<String>,
    pub ranges: Ranges,
    #[serde(default)]
    pub locked: bool,
    #[serde(default)]
    pub active_preset: Option<String>,
    #[serde(default)]
    pub reapply_count: u64,
    #[serde(default)]
    pub last_reapplied_fields: Vec<String>,
    #[serde(default)]
    pub backoff_fields: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
pub struct PresetSummary {
    pub id: String,
    pub label: String,
    pub summary: String,
    #[serde(default)]
    pub badge: Option<String>,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Eq)]
pub struct WebcamMeta {
    #[serde(default)]
    pub presets: Vec<PresetSummary>,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct WebcamView {
    pub device: crate::backend::Device,
    pub state: Option<Settings>,
    #[serde(default)]
    pub meta: WebcamMeta,
}

#[derive(Default)]
pub struct WebcamPage {
    pub status: Option<WebcamView>,
    last: Option<Settings>,
    pub(crate) busy: bool,
    notice: Option<(bool, String)>,
}

impl WebcamPage {
    pub fn receive(&mut self, view: Option<WebcamView>) {
        if self.busy {
            return;
        }
        if let Some(settings) = view
            .as_ref()
            .filter(|v| v.device.status == "ready")
            .and_then(|v| v.state.as_ref())
        {
            self.last = Some(settings.clone());
        }
        self.status = view;
    }

    fn live(&self) -> Option<&Settings> {
        self.status
            .as_ref()
            .filter(|v| v.device.status == "ready")?
            .state
            .as_ref()
    }
}

fn preset_label(id: Option<&str>) -> &'static str {
    match id {
        Some("studio") => "Studio / Stream",
        Some("low-light") => "Faible lumière",
        Some("natural") => "Naturel",
        Some("cinema") => "Cinéma",
        _ => "Personnalisé",
    }
}

fn preset_icon(id: &str) -> (Icon, u32) {
    match id {
        "studio" => (Icon::Sparkles, ui::ACCENT),
        "low-light" => (Icon::Sun, ui::ORANGE),
        "natural" => (Icon::Leaf, ui::GREEN),
        "cinema" => (Icon::Sliders, 0x5856d6),
        _ => (Icon::Camera, ui::ACCENT),
    }
}

fn format_exposure(val: f64) -> String {
    let n = val.round() as i32;
    if (-13..=-1).contains(&n) {
        let denom = 1u32 << (-n) as u32;
        format!("1/{denom} s ({n})")
    } else if n == 0 {
        "1 s (0)".into()
    } else {
        format!("{n}")
    }
}

fn format_kelvin(val: f64) -> String {
    let k = val.round() as i32;
    if k >= 1000 {
        format!("{} {:03} K", k / 1000, (k % 1000).abs())
    } else {
        format!("{k} K")
    }
}

fn field_label(name: &str) -> &str {
    match name {
        "brightness" => "luminosité",
        "contrast" => "contraste",
        "saturation" => "saturation",
        "sharpness" => "netteté",
        "gain" => "gain",
        "white_balance_auto" | "white_balance" => "balance des blancs",
        "backlight_comp" => "contre-jour",
        "power_line" => "anti-scintillement",
        "exposure_auto" | "exposure" => "exposition",
        "low_light_comp" => "basse lumière",
        "focus_auto" | "focus" => "mise au point",
        "zoom" => "zoom",
        "pan" => "panoramique",
        "tilt" => "inclinaison",
        "led_mode" => "LED",
        other => other,
    }
}

fn metric(glyph: Icon, title: &'static str, value: String, sub: String, colour: u32, stale: bool) -> Div {
    ui::card()
        .flex_1()
        .min_w(px(0.))
        .gap(px(4.))
        .px(px(16.))
        .py(px(12.))
        .rounded(px(18.))
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(8.))
                .text_size(px(12.))
                .text_color(rgb(ui::TEXT2))
                .child(ui::icon(glyph, 18., rgb(colour)))
                .child(title),
        )
        .child(
            div()
                .truncate()
                .text_size(px(19.))
                .font_weight(FontWeight::BOLD)
                .when(stale, |d| d.opacity(0.45))
                .child(value),
        )
        .child(ui::hint(sub).text_size(px(12.)))
}

fn safe_range(r: ControlRange) -> ((f64, f64), f64) {
    let min = f64::from(r.min);
    let max = f64::from(r.max.max(r.min + 1));
    let step = f64::from(r.step.max(1));
    ((min, max), step)
}

impl Mixer {
    pub(crate) fn webcam_send(&mut self, action: &'static str, body: Value, done: &'static str, cx: &mut Context<Self>) {
        if self.webcam.busy {
            return;
        }
        self.webcam.busy = true;
        self.webcam.notice = None;
        cx.notify();
        let client = self.client.clone();
        let work = cx.background_executor().spawn(async move {
            let result = client.webcam_command(action, &body);
            (result, client.webcam_view())
        });
        cx.spawn(async move |this, cx| {
            let (result, view) = work.await;
            let _ = this.update(cx, |this, cx| {
                this.webcam.busy = false;
                this.webcam.receive(view.ok());
                match result {
                    Ok(settings) => {
                        this.webcam.last = Some(settings.clone());
                        if let Some(v) = this.webcam.status.as_mut()
                            && v.device.status == "ready"
                        {
                            v.state = Some(settings);
                        }
                        this.webcam.notice = Some((true, done.into()));
                        this.webcam_fade_notice(cx);
                    }
                    Err(error) => this.webcam.notice = Some((false, error)),
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn webcam_fade_notice(&mut self, cx: &mut Context<Self>) {
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(std::time::Duration::from_secs(2)).await;
            let _ = this.update(cx, |this, cx| {
                if this.webcam.notice.as_ref().is_some_and(|n| n.0) {
                    this.webcam.notice = None;
                    cx.notify();
                }
            });
        })
        .detach();
    }

    /// En-tête de la page webcam : état du dernier envoi, réinitialisation usine et actualisation.
    pub(crate) fn webcam_actions(&self, cx: &mut Context<Self>) -> Div {
        let w = &self.webcam;
        let (colour, text) = if w.busy {
            (ui::TEXT3, Some("Envoi à la webcam…".to_owned()))
        } else {
            match &w.notice {
                Some((true, t)) => (ui::GREEN_TEXT, Some(t.clone())),
                Some((false, t)) => (ui::RED_TEXT, Some(t.clone())),
                None => (ui::TEXT3, None),
            }
        };
        let editable = w.live().is_some() && !w.busy;
        div()
            .flex()
            .items_center()
            .gap(px(10.))
            .child(div().max_w(px(320.)).truncate().text_size(px(13.)).text_color(rgb(colour)).children(text))
            .when(editable, |d| {
                d.child(
                    ui::pill("c920-reset", Some(Icon::History), "Défaut usine", Tone::Plain)
                        .on_click(cx.listener(|this, _, _, cx| this.webcam_send("reset", json!({}), "Réglages d’usine restaurés", cx))),
                )
            })
            .child(
                ui::pill("c920-refresh", Some(Icon::Refresh), "Actualiser", Tone::Plain)
                    .on_click(cx.listener(|this, _, _, cx| this.webcam_send("refresh", json!({}), "Réglages relus", cx))),
            )
    }

    fn webcam_image_card(&self, s: &Settings, enabled: bool, cx: &mut Context<Self>) -> Div {
        let r = &s.ranges;
        let (b_range, b_step) = safe_range(r.brightness);
        let (c_range, c_step) = safe_range(r.contrast);
        let (sat_range, sat_step) = safe_range(r.saturation);
        let (sh_range, sh_step) = safe_range(r.sharpness);
        let (g_range, g_step) = safe_range(r.gain);
        let (wb_range, wb_step) = safe_range(r.white_balance);

        let wb_auto = s.white_balance_auto;
        let bl_on = s.backlight_comp;
        let mut pl_seg = ui::segmented();
        let pl_choices: &[(&str, &str)] = if r.power_line.min == 0 {
            &[("off", "Désactivé"), ("hz50", "50 Hz (France)"), ("hz60", "60 Hz")]
        } else {
            &[("hz50", "50 Hz (France)"), ("hz60", "60 Hz")]
        };
        for &(val, label) in pl_choices {
            let selected = s.power_line == val;
            pl_seg = pl_seg.child(
                ui::seg(SharedString::from(format!("c920-pl-{val}")), label, selected).when(enabled && !selected, |d| {
                    d.on_click(cx.listener(move |this, _, _, cx| {
                        this.webcam_send("settings", json!({"power_line": val}), "Anti-scintillement appliqué", cx);
                    }))
                }),
            );
        }

        ui::section(Icon::Sun, ui::ORANGE, "Image et couleur", Some("Luminosité, contraste et balance".into()))
            .child(self.slider_row(
                "c920-brightness", "Luminosité", f64::from(s.brightness), b_range, b_step,
                |v| format!("{}", v.round() as i32), enabled,
                |this, v, cx| this.webcam_send("settings", json!({"brightness": v.round() as i32}), "Luminosité appliquée", cx),
                cx,
            ))
            .child(self.slider_row(
                "c920-contrast", "Contraste", f64::from(s.contrast), c_range, c_step,
                |v| format!("{}", v.round() as i32), enabled,
                |this, v, cx| this.webcam_send("settings", json!({"contrast": v.round() as i32}), "Contraste appliqué", cx),
                cx,
            ))
            .child(self.slider_row(
                "c920-saturation", "Saturation", f64::from(s.saturation), sat_range, sat_step,
                |v| format!("{}", v.round() as i32), enabled,
                |this, v, cx| this.webcam_send("settings", json!({"saturation": v.round() as i32}), "Saturation appliquée", cx),
                cx,
            ))
            .child(self.slider_row(
                "c920-sharpness", "Netteté", f64::from(s.sharpness), sh_range, sh_step,
                |v| format!("{}", v.round() as i32), enabled,
                |this, v, cx| this.webcam_send("settings", json!({"sharpness": v.round() as i32}), "Netteté appliquée", cx),
                cx,
            ))
            .child(self.slider_row(
                "c920-gain", "Gain", f64::from(s.gain), g_range, g_step,
                |v| format!("{}", v.round() as i32), enabled,
                |this, v, cx| this.webcam_send("settings", json!({"gain": v.round() as i32}), "Gain appliqué", cx),
                cx,
            ))
            .child(ui::toggle_row(
                Some(Icon::Sparkles),
                "Balance des blancs automatique",
                ui::toggle("c920-wb-auto", wb_auto, enabled && r.white_balance.supports_auto).when(enabled && r.white_balance.supports_auto, |d| {
                    d.on_click(cx.listener(move |this, _, _, cx| {
                        this.webcam_send("settings", json!({"white_balance_auto": !wb_auto}), "Balance des blancs mise à jour", cx);
                    }))
                }),
            ))
            .child(self.slider_row(
                "c920-wb", "Température (manuelle)", f64::from(s.white_balance), wb_range, wb_step,
                format_kelvin, enabled && !wb_auto,
                |this, v, cx| this.webcam_send("settings", json!({"white_balance_auto": false, "white_balance": v.round() as i32}), "Balance des blancs appliquée", cx),
                cx,
            ))
            .child(ui::toggle_row(
                Some(Icon::Sun),
                "Compensation de contre-jour",
                ui::toggle("c920-backlight", bl_on, enabled).when(enabled, |d| {
                    d.on_click(cx.listener(move |this, _, _, cx| {
                        this.webcam_send("settings", json!({"backlight_comp": !bl_on}), "Contre-jour mis à jour", cx);
                    }))
                }),
            ))
            .child(ui::label("Anti-scintillement (éclairage secteur)"))
            .child(pl_seg)
    }

    fn webcam_optics_card(&self, s: &Settings, enabled: bool, cx: &mut Context<Self>) -> Div {
        let r = &s.ranges;
        let (exp_range, exp_step) = safe_range(r.exposure);
        let (foc_range, foc_step) = safe_range(r.focus);
        let (zoom_range, zoom_step) = safe_range(r.zoom);
        let (pan_range, pan_step) = safe_range(r.pan);
        let (tilt_range, tilt_step) = safe_range(r.tilt);

        let exp_auto = s.exposure_auto;
        let ll_on = s.low_light_comp;
        let foc_auto = s.focus_auto;
        let off_center = s.zoom != r.zoom.default || s.pan != r.pan.default || s.tilt != r.tilt.default;

        let mut card = ui::section(Icon::Camera, 0x5856d6, "Optique et cadrage", Some("Exposition, mise au point et zoom".into()))
            .child(ui::toggle_row(
                Some(Icon::Gauge),
                "Exposition automatique",
                ui::toggle("c920-exp-auto", exp_auto, enabled && r.exposure.supports_auto).when(enabled && r.exposure.supports_auto, |d| {
                    d.on_click(cx.listener(move |this, _, _, cx| {
                        this.webcam_send("settings", json!({"exposure_auto": !exp_auto}), "Exposition mise à jour", cx);
                    }))
                }),
            ))
            .child(self.slider_row(
                "c920-exposure", "Exposition manuelle", f64::from(s.exposure), exp_range, exp_step,
                format_exposure, enabled && !exp_auto,
                |this, v, cx| this.webcam_send("settings", json!({"exposure_auto": false, "exposure": v.round() as i32}), "Exposition appliquée", cx),
                cx,
            ))
            .child(ui::toggle_row(
                Some(Icon::Activity),
                "Compensation basse lumière",
                ui::toggle("c920-lowlight", ll_on, enabled).when(enabled, |d| {
                    d.on_click(cx.listener(move |this, _, _, cx| {
                        this.webcam_send("settings", json!({"low_light_comp": !ll_on}), "Basse lumière mise à jour", cx);
                    }))
                }),
            ))
            .child(ui::caption("Coupe cette option pour bloquer un flux fluide à 30 images/s sans flou en intérieur."))
            .child(ui::toggle_row(
                Some(Icon::Camera),
                "Autofocus",
                ui::toggle("c920-focus-auto", foc_auto, enabled && r.focus.supports_auto).when(enabled && r.focus.supports_auto, |d| {
                    d.on_click(cx.listener(move |this, _, _, cx| {
                        this.webcam_send("settings", json!({"focus_auto": !foc_auto}), "Mise au point mise à jour", cx);
                    }))
                }),
            ))
            .child(self.slider_row(
                "c920-focus", "Distance de mise au point", f64::from(s.focus), foc_range, foc_step,
                |v| format!("{}", v.round() as i32), enabled && !foc_auto,
                |this, v, cx| this.webcam_send("settings", json!({"focus_auto": false, "focus": v.round() as i32}), "Mise au point appliquée", cx),
                cx,
            ))
            .child(self.slider_row(
                "c920-zoom", "Zoom numérique", f64::from(s.zoom), zoom_range, zoom_step,
                |v| format!("{} %", v.round() as i32), enabled,
                |this, v, cx| this.webcam_send("settings", json!({"zoom": v.round() as i32}), "Zoom appliqué", cx),
                cx,
            ))
            .child(self.slider_row(
                "c920-pan", "Panoramique (gauche / droite)", f64::from(s.pan), pan_range, pan_step,
                |v| format!("{}", v.round() as i32), enabled,
                |this, v, cx| this.webcam_send("settings", json!({"pan": v.round() as i32}), "Panoramique appliqué", cx),
                cx,
            ))
            .child(self.slider_row(
                "c920-tilt", "Inclinaison (haut / bas)", f64::from(s.tilt), tilt_range, tilt_step,
                |v| format!("{}", v.round() as i32), enabled,
                |this, v, cx| this.webcam_send("settings", json!({"tilt": v.round() as i32}), "Inclinaison appliquée", cx),
                cx,
            ));

        if off_center && enabled {
            let (z, p, t) = (r.zoom.default, r.pan.default, r.tilt.default);
            card = card.child(
                ui::pill("c920-recenter", Some(Icon::Refresh), "Recentrer le cadrage", Tone::Plain)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.webcam_send("settings", json!({"zoom": z, "pan": p, "tilt": t}), "Cadrage recentré", cx);
                    })),
            );
        }
        card
    }

    fn webcam_xu_card(&self, s: &Settings, enabled: bool, cx: &mut Context<Self>) -> Option<Div> {
        let current_led = s.led_mode.as_deref()?;
        let mut seg = ui::segmented();
        for (val, label) in [("off", "Éteinte"), ("on", "Allumée"), ("blink", "Clignotante"), ("auto", "Auto")] {
            let selected = current_led == val;
            seg = seg.child(
                ui::seg(SharedString::from(format!("c920-led-{val}")), label, selected).when(enabled && !selected, |d| {
                    d.on_click(cx.listener(move |this, _, _, cx| {
                        this.webcam_send("settings", json!({"led_mode": val}), "LED mise à jour", cx);
                    }))
                }),
            );
        }
        Some(
            ui::section(Icon::Sparkles, ui::ACCENT, "LED d’activité (XU Logitech)", None)
                .child(ui::label("Mode de la LED bleue"))
                .child(seg)
                .child(ui::caption("Automatique allume la LED uniquement quand la webcam filme.")),
        )
    }

    fn webcam_guard_card(&self, s: &Settings, enabled: bool, cx: &mut Context<Self>) -> Div {
        let locked = s.locked;
        let status_label = if locked {
            if s.reapply_count > 0 {
                format!("Actif · {} réapplication(s)", s.reapply_count)
            } else {
                "Actif".into()
            }
        } else {
            "Inactif".into()
        };
        let mut card = ui::section(Icon::Shield, if locked { ui::GREEN } else { ui::TEXT3 }, "Garder mes réglages", Some(status_label.into()))
            .child(ui::toggle_row(
                Some(Icon::Shield),
                "Maintenir automatiquement mes réglages",
                ui::toggle("c920-lock", locked, enabled).when(enabled, |d| {
                    d.on_click(cx.listener(move |this, _, _, cx| {
                        let next = !locked;
                        let msg = if next { "Maintien activé" } else { "Maintien désactivé" };
                        this.webcam_send("lock", json!({"enabled": next}), msg, cx);
                    }))
                }),
            ))
            .child(ui::hint("Réapplique tes choix dès que la C920 se reconnecte ou quand Windows, OBS ou Discord les réinitialise."));

        if locked && !s.last_reapplied_fields.is_empty() {
            let names: Vec<&str> = s.last_reapplied_fields.iter().map(|f| field_label(f)).collect();
            card = card.child(ui::caption(format!("Dernière correction automatique : {}.", names.join(", "))));
        }
        if !s.backoff_fields.is_empty() {
            let names: Vec<&str> = s.backoff_fields.iter().map(|f| field_label(f)).collect();
            card = card.child(ui::notice(false, format!("Réglage temporairement en pause (refusé par le pilote) : {}.", names.join(", "))));
        }
        card
    }

    fn webcam_presets_card(&self, s: &Settings, enabled: bool, width: f32, cx: &mut Context<Self>) -> Div {
        let presets = self
            .webcam
            .status
            .as_ref()
            .map(|v| v.meta.presets.clone())
            .filter(|p| !p.is_empty())
            .unwrap_or_else(default_presets);
        let mut tiles: Vec<AnyElement> = Vec::new();
        for p in presets {
            let id = p.id.clone();
            let selected = s.active_preset.as_deref() == Some(id.as_str());
            let (glyph, colour) = preset_icon(&id);
            let badge = p.badge.map(|b| (SharedString::from(b), ui::ACCENT));
            tiles.push(
                ui::tile(
                    SharedString::from(format!("c920-preset-{id}")),
                    Some((glyph, colour)),
                    p.label,
                    Some(p.summary.into()),
                    selected,
                    badge,
                )
                .when(enabled, |d| {
                    d.on_click(cx.listener(move |this, _, _, cx| {
                        this.webcam_send("preset", json!({"id": id}), "Préréglage appliqué", cx);
                    }))
                })
                .into_any_element(),
            );
        }
        let cols = ui::columns_that_fit(width - 42., 240., 10., 2);
        ui::section(Icon::Sparkles, ui::ACCENT, "Préréglages en un clic", Some("Adaptés aux bornes de ta C920".into()))
            .child(ui::grid(tiles, cols))
    }

    fn webcam_specs(&self, s: &Settings, enabled: bool, cx: &mut Context<Self>) -> Div {
        let row = |k: &'static str, v: String| {
            div()
                .flex()
                .justify_between()
                .gap(px(12.))
                .py(px(7.))
                .border_b_1()
                .border_color(alpha(ui::INK, 0.06))
                .child(div().text_size(px(12.5)).text_color(rgb(ui::TEXT2)).child(k))
                .child(div().text_size(px(12.5)).font_weight(FontWeight::MEDIUM).text_right().child(v))
        };
        let formats = if s.device.formats.is_empty() {
            "YUY2 / MJPG (jusqu’à 1080p 30 ips)".into()
        } else {
            s.device
                .formats
                .iter()
                .take(4)
                .map(|f| format!("{} {}x{} @ {} ips", f.codec, f.width, f.height, f.max_fps))
                .collect::<Vec<_>>()
                .join(" · ")
        };
        let xu_summary = if s.device.xu.units.is_empty() {
            "Aucune unité XU détectée".into()
        } else {
            s.device
                .xu
                .units
                .iter()
                .map(|u| format!("{} (nœud {})", u.name, u.node_id))
                .collect::<Vec<_>>()
                .join(" · ")
        };
        ui::section(Icon::Camera, ui::ACCENT, "Ta Logitech C920", None)
            .gap(px(4.))
            .child(row("Modèle", s.device.name.clone()))
            .child(row("Fabricant", "Logitech".into()))
            .child(row("Révision", s.device.revision_label.clone()))
            .child(row("Identifiant USB", format!("{:04X}:{:04X} (rév. {})", s.device.vid, s.device.pid, s.device.revision)))
            .child(row("Firmware", s.device.firmware.clone().unwrap_or_else(|| "Non annoncé".into())))
            .when_some(s.device.dsp_platform.clone(), |d, dsp| d.child(row("Plateforme DSP", dsp)))
            .child(row("Formats vidéo", formats))
            .child(row("Unités XU", xu_summary))
            .child(row(
                "Maintien des réglages",
                if s.locked { format!("Actif ({} réapplication(s))", s.reapply_count) } else { "Désactivé".into() },
            ))
            .when(enabled, |d| {
                d.child(
                    div().pt(px(10.)).child(
                        ui::pill("c920-info-reset", Some(Icon::History), "Réinitialiser aux valeurs d’usine", Tone::Warn)
                            .on_click(cx.listener(|this, _, _, cx| this.webcam_send("reset", json!({}), "Réglages d’usine restaurés", cx))),
                    ),
                )
            })
            .child(ui::hint("Rustudio pilote uniquement les contrôles UVC et XU de la C920 : aucun flux vidéo ni micro n’est ouvert et aucun firmware n’est modifié.").text_size(px(12.)).pt(px(8.)))
    }

    pub fn webcam_view(&self, window: &gpui::Window, cx: &mut Context<Self>) -> Div {
        let w = &self.webcam;
        let width = crate::desktop::page_width(window);
        let mut content = div().flex().flex_col().gap(px(14.));
        let state = w.status.as_ref().map(|v| v.device.status.as_str());
        let waiting = match state {
            Some("disconnected") => Some(("Logitech C920 introuvable", "Branche ta webcam C920 en USB. Ses réglages apparaissent tout seuls.")),
            Some("error" | "blocked") => Some(("Lecture de la C920 impossible", "Rustudio réessaie tout seul dès que le pilote répond.")),
            Some("ready") => None,
            _ => Some(("En attente de la C920…", "Les réglages apparaissent après la première lecture.")),
        };
        if let Some((title, text)) = waiting {
            let err = w.status.as_ref().and_then(|v| v.device.error.clone());
            content = content.child(
                ui::card()
                    .items_center()
                    .py(px(30.))
                    .child(ui::chip(Icon::Camera, ui::ACCENT, 52.))
                    .child(ui::title(title, 17.))
                    .child(ui::hint(text))
                    .children(err.map(ui::caption)),
            );
        }
        let Some(s) = w.live().or(w.last.as_ref()) else {
            return content;
        };
        let stale = w.live().is_none();
        let enabled = !stale && !w.busy;

        match self.page {
            Page::WebcamImage => return content.child(div().max_w(px(680.)).child(self.webcam_image_card(s, enabled, cx))),
            Page::WebcamOptics => {
                let mut right = vec![self.webcam_optics_card(s, enabled, cx).into_any_element()];
                if let Some(xu) = self.webcam_xu_card(s, enabled, cx) {
                    right.push(xu.into_any_element());
                }
                return content.child(div().max_w(px(680.)).child(ui::stack_cards(right)));
            }
            Page::WebcamInfo => return content.child(div().max_w(px(640.)).child(self.webcam_specs(s, enabled, cx))),
            _ => {}
        }

        let exp_val = if s.exposure_auto { "Automatique".into() } else { format_exposure(f64::from(s.exposure)) };
        let exp_sub = if s.low_light_comp { "Basse lumière active" } else { "Basse lumière coupée (30 ips)" };
        let foc_val = if s.focus_auto { "Autofocus".into() } else { format!("Manuel · {}", s.focus) };
        let guard_val = if s.locked { "Verrouillé".into() } else { "Libre".into() };
        let guard_sub = if s.locked && s.reapply_count > 0 {
            format!("{} dérive(s) corrigée(s)", s.reapply_count)
        } else if s.locked {
            "Protégé contre OBS / Discord".into()
        } else {
            "Aucune écriture auto".into()
        };

        content = content
            .child(
                div()
                    .flex()
                    .gap(px(12.))
                    .child(metric(Icon::Sparkles, "Mode actif", preset_label(s.active_preset.as_deref()).into(), format!("Anti-scintillement : {}", power_line_short(&s.power_line)), ui::ACCENT, stale))
                    .child(metric(Icon::Gauge, "Exposition", exp_val, exp_sub.into(), ui::ORANGE, stale))
                    .child(metric(Icon::Camera, "Mise au point", foc_val, format!("Zoom {} %", s.zoom), 0x5856d6, stale))
                    .child(metric(Icon::Shield, "Maintien", guard_val, guard_sub, if s.locked { ui::GREEN } else { ui::TEXT3 }, stale)),
            )
            .child(self.webcam_guard_card(s, enabled, cx))
            .child(self.webcam_presets_card(s, enabled, width, cx));

        let left = vec![self.webcam_image_card(s, enabled, cx).into_any_element()];
        let mut right = vec![self.webcam_optics_card(s, enabled, cx).into_any_element()];
        if let Some(xu) = self.webcam_xu_card(s, enabled, cx) {
            right.push(xu.into_any_element());
        }
        content
            .child(ui::settings_columns(width, left, right))
            .child(ui::caption("Les réglages UVC agissent en direct sans ouvrir le flux vidéo ni le micro. Active « Garder mes réglages » pour éviter les réinitialisations Windows, OBS ou Discord."))
    }
}

fn power_line_short(val: &str) -> &'static str {
    match val {
        "hz50" => "50 Hz",
        "hz60" => "60 Hz",
        _ => "coupé",
    }
}

fn default_presets() -> Vec<PresetSummary> {
    vec![
        PresetSummary {
            id: "studio".into(),
            label: "Studio / Stream".into(),
            summary: "Exposition manuelle fluide (30 ips réels), basse lumière coupée, balance verrouillée et 50 Hz.".into(),
            badge: Some("Conseillé".into()),
        },
        PresetSummary {
            id: "low-light".into(),
            label: "Faible lumière".into(),
            summary: "Plus de luminosité et de gain en soirée, tout en limitant le flou de mouvement.".into(),
            badge: None,
        },
        PresetSummary {
            id: "natural".into(),
            label: "Naturel".into(),
            summary: "Rendu neutre pour les appels quotidiens : exposition, balance et mise au point automatiques.".into(),
            badge: None,
        },
        PresetSummary {
            id: "cinema".into(),
            label: "Cinéma".into(),
            summary: "Contraste soutenu, netteté douce sans halo numérique et couleur verrouillée.".into(),
            badge: None,
        },
    ]
}

/// Auto-test (démonstration) : réglages, préréglage Studio, maintien anti-réinitialisation et remise à zéro.
pub fn verify(client: &Client) -> Result<(), String> {
    let view = client.webcam_view()?;
    let original = view.state.ok_or("Réglages de démo C920 absents")?;
    if original.device.vid != 0x046D || original.brightness != 128 {
        return Err("Le test exige les données de démonstration de la C920.".into());
    }
    let s = client.webcam_command("preset", &json!({"id": "studio"}))?;
    if s.exposure_auto || s.low_light_comp || s.power_line != "hz50" || s.active_preset.as_deref() != Some("studio") {
        return Err("Le préréglage Studio C920 n’a pas été relu.".into());
    }
    let s = client.webcam_command("settings", &json!({"brightness": 144, "zoom": 135, "focus_auto": false, "focus": 40, "led_mode": "off"}))?;
    if s.brightness != 144 || s.zoom != 135 || s.focus_auto || s.focus != 40 || s.led_mode.as_deref() != Some("off") {
        return Err("Réglages manuels C920 non relus.".into());
    }
    let s = client.webcam_command("lock", &json!({"enabled": true}))?;
    if !s.locked {
        return Err("Le maintien des réglages C920 ne s’est pas activé.".into());
    }
    if client.webcam_command("settings", &json!({"brightness": 999})).is_ok() {
        return Err("Une luminosité hors limites a été acceptée.".into());
    }
    let s = client.webcam_command("lock", &json!({"enabled": false}))?;
    if s.locked {
        return Err("Le maintien des réglages C920 ne s’est pas désactivé.".into());
    }
    let reset = client.webcam_command("reset", &json!({}))?;
    if reset.brightness != 128 || reset.zoom != 100 || !reset.exposure_auto {
        return Err("La réinitialisation d’usine C920 n’a pas restauré les valeurs.".into());
    }
    println!("OK : image, optique, XU, préréglage Studio, maintien et réinitialisation C920.");
    Ok(())
}
