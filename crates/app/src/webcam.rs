//! Page « Ma webcam » : espace studio unifié pour la Logitech C920 avec aperçu vidéo en temps réel,
//! préréglages en un clic, réglages organisés par onglets (Image & Couleur, Optique & Cadrage, Appareil & LED)
//! et maintien anti-réinitialisation.
use crate::{
    backend::Client,
    mixer::Mixer,
    ui::{self, Icon, Tone, alpha, rgb},
};
use gpui::{AnyElement, Context, Div, FontWeight, IntoElement, ObjectFit, RenderImage, SharedString, Window, div, img, prelude::*, px};
use image::{Frame as ImgFrame, ImageBuffer, Rgba};
use rustudio_windows_desktop::webcam_preview::{Status as PreviewStatus, WebcamPreview, flip_horizontal_bgra};
use serde::Deserialize;
use serde_json::{Value, json};
use std::sync::Arc;
use std::time::Duration;

pub const MODULE: &str = "/api/modules/logitech-c920";
/// Rythme de rafraîchissement de l'aperçu vidéo (~30 ips).
pub(crate) const PREVIEW_FRAME: Duration = Duration::from_millis(33);

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

/// Onglet actif dans l'espace studio C920.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum WebcamTab {
    #[default]
    Image,
    Optics,
    Device,
}

pub struct WebcamPage {
    pub status: Option<WebcamView>,
    last: Option<Settings>,
    pub(crate) busy: bool,
    /// Dernière commande mise en attente pendant un envoi : garantit qu'aucun glissement de curseur n'est perdu.
    pending_cmd: Option<(&'static str, Value, &'static str)>,
    notice: Option<(bool, String)>,
    pub(crate) tab: WebcamTab,
    /// Aperçu vidéo activé (vrai par défaut : se coupe automatiquement dès qu'on quitte la page).
    pub(crate) preview_on: bool,
    /// Effet miroir local de l'aperçu (pratique pour se cadrer, ne change pas OBS).
    pub(crate) mirror: bool,
    preview: WebcamPreview,
    preview_status: PreviewStatus,
    rendered_seq: u64,
    rendered_mirror: bool,
    rendered_demo_sig: Option<DemoSig>,
    rendered_image: Option<Arc<RenderImage>>,
}

impl Default for WebcamPage {
    fn default() -> Self {
        Self {
            status: None,
            last: None,
            busy: false,
            pending_cmd: None,
            notice: None,
            tab: WebcamTab::Image,
            preview_on: true,
            mirror: true,
            preview: WebcamPreview::new(),
            preview_status: PreviewStatus::Idle,
            rendered_seq: 0,
            rendered_mirror: true,
            rendered_demo_sig: None,
            rendered_image: None,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct DemoSig {
    brightness: i32,
    contrast: i32,
    saturation: i32,
    sharpness: i32,
    wb_auto: bool,
    wb: i32,
    exp_auto: bool,
    exp: i32,
    zoom: i32,
    pan: i32,
    tilt: i32,
    focus_auto: bool,
    focus: i32,
    backlight: bool,
    mirror: bool,
}

impl DemoSig {
    fn of(s: &Settings, mirror: bool) -> Self {
        Self {
            brightness: s.brightness,
            contrast: s.contrast,
            saturation: s.saturation,
            sharpness: s.sharpness,
            wb_auto: s.white_balance_auto,
            wb: s.white_balance,
            exp_auto: s.exposure_auto,
            exp: s.exposure,
            zoom: s.zoom,
            pan: s.pan,
            tilt: s.tilt,
            focus_auto: s.focus_auto,
            focus: s.focus,
            backlight: s.backlight_comp,
            mirror,
        }
    }
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

    pub(crate) fn live(&self) -> Option<&Settings> {
        self.status
            .as_ref()
            .filter(|v| v.device.status == "ready")?
            .state
            .as_ref()
    }

    pub(crate) fn preview_running(&self) -> bool {
        self.preview.is_active()
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

fn safe_range(r: ControlRange) -> ((f64, f64), f64) {
    let min = f64::from(r.min);
    let max = f64::from(r.max.max(r.min + 1));
    let step = f64::from(r.step.max(1));
    ((min, max), step)
}

fn make_render_image(width: u32, height: u32, bgra: Vec<u8>) -> Option<Arc<RenderImage>> {
    let buf = ImageBuffer::<Rgba<u8>, Vec<u8>>::from_raw(width, height, bgra)?;
    Some(Arc::new(RenderImage::new(vec![ImgFrame::new(buf)])))
}

/// Synthétise une scène 16:9 de studio en temps réel pour le mode démonstration :
/// réagit visiblement à la luminosité, au contraste, à la saturation, à la balance (Kelvin),
/// à l'exposition, au zoom, au panoramique, à l'inclinaison et à la mise au point.
fn render_demo_frame(s: &Settings, mirror: bool) -> Option<Arc<RenderImage>> {
    const W: usize = 480;
    const H: usize = 270;
    let mut bgra = vec![0u8; W * H * 4];

    let zoom = (f32::from(s.zoom as i16) / 100.0).clamp(1.0, 5.0);
    let pan_norm = (s.pan as f32 / s.ranges.pan.max.max(1) as f32).clamp(-1.0, 1.0);
    let tilt_norm = (s.tilt as f32 / s.ranges.tilt.max.max(1) as f32).clamp(-1.0, 1.0);

    let bright = (s.brightness - 128) as f32 / 128.0; // -1..+1
    let contrast = (s.contrast as f32 / 128.0).clamp(0.25, 2.2);
    let sat = (s.saturation as f32 / 128.0).clamp(0.0, 2.2);
    let exp_mult = if s.exposure_auto {
        1.0
    } else {
        // -5 = nominal, plus bas = plus sombre, plus haut = plus lumineux
        2.0f32.powf((s.exposure + 5) as f32 * 0.35).clamp(0.25, 2.2)
    };
    let backlight_lift = if s.backlight_comp { 0.08 } else { 0.0 };

    // Température de couleur (2000 K chaud -> 6500 K froid)
    let wb_norm = if s.white_balance_auto {
        0.0
    } else {
        ((s.white_balance as f32 - 4200.0) / 2200.0).clamp(-1.0, 1.0)
    };
    let (wb_r, wb_g, wb_b) = (1.0 - wb_norm * 0.22, 1.0, 1.0 + wb_norm * 0.26);

    // Décalage de netteté / focus simulé
    let blur_factor = if s.focus_auto {
        0.0
    } else {
        ((s.focus - 35).abs() as f32 / 180.0).clamp(0.0, 1.0)
    };
    let sharp_edge = 0.02 + blur_factor * 0.14;

    for y in 0..H {
        let ny = (y as f32 / H as f32 - 0.5) * 2.0; // -1..1
        let sy = ny / zoom - tilt_norm * 0.32;
        for x in 0..W {
            let ux = if mirror { W - 1 - x } else { x };
            let nx = (ux as f32 / W as f32 - 0.5) * 2.0;
            let sx = nx / zoom + pan_norm * 0.38;

            // Fond studio dégradé (mur sombre avec halo néon bleu/violet + keylight chaud)
            let mut r = 0.09 + (1.0 - (sy + 1.0) * 0.5).clamp(0.0, 1.0) * 0.09;
            let mut g = 0.10 + (1.0 - (sy + 1.0) * 0.5).clamp(0.0, 1.0) * 0.10;
            let mut b = 0.15 + (1.0 - (sy + 1.0) * 0.5).clamp(0.0, 1.0) * 0.16;

            // Halo néon bleu à gauche
            let d_left = ((sx + 0.72).powi(2) + (sy + 0.15).powi(2)).sqrt();
            let glow_l = (-d_left * d_left * 3.2).exp();
            r += glow_l * 0.08;
            g += glow_l * 0.30;
            b += glow_l * 0.62;

            // Halo chaud orange/magenta à droite
            let d_right = ((sx - 0.78).powi(2) + (sy + 0.25).powi(2)).sqrt();
            let glow_r = (-d_right * d_right * 3.4).exp();
            r += glow_r * 0.54;
            g += glow_r * 0.24;
            b += glow_r * 0.14;

            // Étagère / bureau subtil en arrière-plan
            if (sy - 0.42).abs() < 0.03 {
                r += 0.06;
                g += 0.06;
                b += 0.08;
            }

            // Silhouette studio centrée (tête + épaules) pour visualiser immédiatement zoom, pan, tilt et focus
            let d_head = (((sx) / 0.20).powi(2) + ((sy + 0.12) / 0.28).powi(2)).sqrt();
            let d_body = (((sx) / 0.46).powi(2) + ((sy - 0.68) / 0.48).powi(2)).sqrt();
            let subject_d = d_head.min(d_body);
            if subject_d < 1.0 + sharp_edge {
                let mask = ((1.0 + sharp_edge - subject_d) / (sharp_edge * 2.0)).clamp(0.0, 1.0);
                // Éclairage keylight latéral doux sur le sujet
                let shade = (0.72 - sx * 0.35 - sy * 0.18).clamp(0.35, 1.15);
                let (sr, sg, sb) = if d_head < d_body {
                    (0.76 * shade, 0.58 * shade, 0.48 * shade)
                } else {
                    (0.18 * shade, 0.22 * shade, 0.28 * shade)
                };
                r = r * (1.0 - mask) + sr * mask;
                g = g * (1.0 - mask) + sg * mask;
                b = b * (1.0 - mask) + sb * mask;
            }

            // Micro bras de studio en bas à gauche
            let d_mic = (((sx + 0.38) / 0.09).powi(2) + ((sy - 0.48) / 0.22).powi(2)).sqrt();
            if d_mic < 1.0 {
                let m = (1.0 - d_mic).clamp(0.0, 1.0);
                r = r * (1.0 - m) + 0.12 * m;
                g = g * (1.0 - m) + 0.13 * m;
                b = b * (1.0 - m) + 0.16 * m;
            }

            // Applique exposition, balance des blancs, luminosité, contraste et saturation
            r = r * exp_mult * wb_r + bright * 0.42 + backlight_lift;
            g = g * exp_mult * wb_g + bright * 0.42 + backlight_lift;
            b = b * exp_mult * wb_b + bright * 0.42 + backlight_lift;

            r = (r - 0.5) * contrast + 0.5;
            g = (g - 0.5) * contrast + 0.5;
            b = (b - 0.5) * contrast + 0.5;

            let luma = 0.299 * r + 0.587 * g + 0.114 * b;
            r = luma + (r - luma) * sat;
            g = luma + (g - luma) * sat;
            b = luma + (b - luma) * sat;

            // Légère accentuation si netteté élevée
            if s.sharpness > 150 {
                let vig = 1.0 - (nx * nx + ny * ny) * 0.06;
                r *= vig;
                g *= vig;
                b *= vig;
            }

            let idx = (y * W + x) * 4;
            bgra[idx] = (b.clamp(0.0, 1.0) * 255.0) as u8;
            bgra[idx + 1] = (g.clamp(0.0, 1.0) * 255.0) as u8;
            bgra[idx + 2] = (r.clamp(0.0, 1.0) * 255.0) as u8;
            bgra[idx + 3] = 255;
        }
    }
    make_render_image(W as u32, H as u32, bgra)
}

impl Mixer {
    /// Active ou libère la capture vidéo live de la C920 selon la visibilité de la page.
    pub(crate) fn webcam_visibility(&mut self, visible: bool) {
        if visible && self.webcam.preview_on && self.client.is_live() {
            self.webcam.preview.start();
        } else {
            self.webcam.preview.stop();
        }
    }

    /// Synchronise la dernière image vidéo (live Windows ou scène simulée démo) et libère proprement
    /// la texture GPU précédente pour garder une mémoire vidéo constante à 30 ips.
    pub(crate) fn sync_webcam_preview(&mut self, window: &mut Window) {
        if !self.page.is_webcam() {
            if let Some(prev) = self.webcam.rendered_image.take() {
                let _ = window.drop_image(prev);
                self.webcam.rendered_seq = 0;
                self.webcam.rendered_demo_sig = None;
            }
            return;
        }

        if !self.webcam.preview_on {
            self.webcam.preview_status = PreviewStatus::Idle;
            if let Some(prev) = self.webcam.rendered_image.take() {
                let _ = window.drop_image(prev);
                self.webcam.rendered_seq = 0;
                self.webcam.rendered_demo_sig = None;
            }
            return;
        }

        if !self.client.is_live() {
            // Mode démonstration : aperçu synthétique réactif en temps réel à chaque curseur.
            if let Some(s) = self.webcam.live().or(self.webcam.last.as_ref()).cloned() {
                let sig = DemoSig::of(&s, self.webcam.mirror);
                if self.webcam.rendered_demo_sig != Some(sig) {
                    let next = render_demo_frame(&s, self.webcam.mirror);
                    if let Some(prev) = self.webcam.rendered_image.replace_if(next) {
                        let _ = window.drop_image(prev);
                    }
                    self.webcam.rendered_demo_sig = Some(sig);
                }
                self.webcam.preview_status = PreviewStatus::Live { width: 480, height: 270, fps: 30 };
            }
            return;
        }

        // Mode réel : lit la dernière image capturée par Media Foundation (strictement vidéo).
        let (status, frame) = self.webcam.preview.latest();
        self.webcam.preview_status = status;
        if let Some(f) = frame {
            let need_upload = f.seq != self.webcam.rendered_seq || self.webcam.rendered_mirror != self.webcam.mirror;
            if need_upload {
                let pixels = if self.webcam.mirror {
                    flip_horizontal_bgra(f.width as usize, f.height as usize, &f.bgra)
                } else {
                    (*f.bgra).clone()
                };
                if let Some(next) = make_render_image(f.width, f.height, pixels) {
                    if let Some(prev) = self.webcam.rendered_image.replace(next) {
                        let _ = window.drop_image(prev);
                    }
                    self.webcam.rendered_seq = f.seq;
                    self.webcam.rendered_mirror = self.webcam.mirror;
                }
            }
        } else if !matches!(self.webcam.preview_status, PreviewStatus::Live { .. })
            && let Some(prev) = self.webcam.rendered_image.take()
        {
            let _ = window.drop_image(prev);
            self.webcam.rendered_seq = 0;
        }
    }

    fn toggle_webcam_preview(&mut self, cx: &mut Context<Self>) {
        self.webcam.preview_on = !self.webcam.preview_on;
        self.webcam_visibility(self.page.is_webcam());
        cx.notify();
    }

    fn retry_webcam_preview(&mut self, cx: &mut Context<Self>) {
        self.webcam.preview_on = true;
        if self.client.is_live() {
            self.webcam.preview.restart();
        }
        cx.notify();
    }

    /// Envoi avec file de fusion : un glissement rapide de curseur ne perd jamais la valeur finale.
    pub(crate) fn webcam_send(&mut self, action: &'static str, body: Value, done: &'static str, cx: &mut Context<Self>) {
        if self.webcam.busy {
            // Fusionne les patchs "settings" consécutifs pour garder tous les réglages touchés.
            if action == "settings"
                && let Some((prev_act, prev_body, _)) = self.webcam.pending_cmd.as_mut()
                && *prev_act == "settings"
                && let (Some(dst), Some(src)) = (prev_body.as_object_mut(), body.as_object())
            {
                for (k, v) in src {
                    dst.insert(k.clone(), v.clone());
                }
                return;
            }
            self.webcam.pending_cmd = Some((action, body, done));
            return;
        }
        self.webcam_dispatch(action, body, done, cx);
    }

    fn webcam_dispatch(&mut self, action: &'static str, body: Value, done: &'static str, cx: &mut Context<Self>) {
        self.webcam.busy = true;
        self.webcam.notice = None;
        cx.notify();
        let client = self.client.clone();
        let work = cx.background_executor().spawn(async move {
            let result = client.webcam_command(action, &body);
            let view = client.webcam_view();
            (result, view)
        });
        cx.spawn(async move |this, cx| {
            let (result, view) = work.await;
            let _ = this.update(cx, |this, cx| {
                this.webcam.busy = false;
                if let Some((next_act, next_body, next_done)) = this.webcam.pending_cmd.take() {
                    if let Ok(settings) = result {
                        this.webcam.last = Some(settings.clone());
                        if let Some(v) = this.webcam.status.as_mut()
                            && v.device.status == "ready"
                        {
                            v.state = Some(settings);
                        }
                    }
                    this.webcam_dispatch(next_act, next_body, next_done, cx);
                    return;
                }
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
            cx.background_executor().timer(Duration::from_secs(2)).await;
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
            (ui::TEXT3, Some("Envoi à la C920…".to_owned()))
        } else {
            match &w.notice {
                Some((true, t)) => (ui::GREEN_TEXT, Some(t.clone())),
                Some((false, t)) => (ui::RED_TEXT, Some(t.clone())),
                None => (ui::TEXT3, None),
            }
        };
        let editable = w.live().is_some();
        div()
            .flex()
            .items_center()
            .gap(px(10.))
            .child(div().max_w(px(280.)).truncate().text_size(px(13.)).text_color(rgb(colour)).children(text))
            .when(editable, |d| {
                d.child(
                    ui::pill("c920-reset", Some(Icon::History), "Défaut usine", Tone::Plain)
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.param_held.clear();
                            this.webcam_send("reset", json!({}), "Réglages d’usine restaurés", cx);
                        })),
                )
            })
            .child(
                ui::pill("c920-refresh", Some(Icon::Refresh), "Actualiser", Tone::Plain)
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.param_held.clear();
                        this.webcam_send("refresh", json!({}), "Réglages relus", cx);
                    })),
            )
    }

    /// Curseur en direct : envoie les valeurs pendant le glissement (coalescées sans perte) pour voir l'image bouger en temps réel.
    #[allow(clippy::too_many_arguments)]
    fn webcam_slider_row(
        &self,
        key: &str,
        label: impl Into<SharedString>,
        actual: f64,
        range: (f64, f64),
        step: f64,
        format: impl Fn(f64) -> String,
        enabled: bool,
        commit: impl Fn(&mut Mixer, f64, &mut Context<Mixer>) + 'static,
        cx: &mut Context<Self>,
    ) -> Div {
        let value = self.param_value(&format!("param-{key}"), actual);
        let fraction = ((value - range.0) / (range.1 - range.0)).clamp(0., 1.) as f32;
        let hover = ui::hover_t(&format!("param-{key}"));
        div()
            .w_full()
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(div().text_size(px(13.)).text_color(rgb(ui::TEXT)).child(label.into()))
                    .child(div().text_size(px(13.)).font_weight(FontWeight::MEDIUM).text_color(rgb(ui::TEXT2)).child(format(value))),
            )
            .child(
                self.param_area_with(key, range, step, false, enabled, true, commit, cx)
                    .mt(px(4.))
                    .h(px(22.))
                    .w_full()
                    .child(ui::hslider(fraction, rgb(ui::ACCENT), hover)),
            )
    }

    /// Panneau aperçu vidéo en temps réel (toujours visible pendant qu'on ajuste les réglages).
    fn webcam_preview_card(&self, s: &Settings, preview_h: f32, stale: bool, cx: &mut Context<Self>) -> Div {
        let w = &self.webcam;
        let demo = !self.client.is_live();
        let (badge_colour, badge_text) = if !w.preview_on {
            (ui::TEXT3, "Aperçu en pause".to_owned())
        } else if demo {
            (ui::ACCENT, "Aperçu simulé · en direct".to_owned())
        } else {
            match &w.preview_status {
                PreviewStatus::Live { width, height, fps } => (ui::GREEN, format!("En direct · {width}×{height} · {fps} ips")),
                PreviewStatus::Starting => (ui::ACCENT, "Ouverture du flux vidéo…".to_owned()),
                PreviewStatus::Busy(_) => (ui::ORANGE, "Flux utilisé par OBS / Discord".to_owned()),
                PreviewStatus::Disconnected => (ui::RED, "Flux vidéo introuvable".to_owned()),
                PreviewStatus::Error(_) => (ui::RED, "Aperçu indisponible".to_owned()),
                PreviewStatus::Idle => (ui::TEXT3, "Aperçu arrêté".to_owned()),
            }
        };

        let mut viewport = div()
            .relative()
            .w_full()
            .h(px(preview_h))
            .rounded(px(14.))
            .overflow_hidden()
            .bg(ui::raw(0x111216))
            .border_1()
            .border_color(alpha(ui::INK, 0.12))
            .flex()
            .items_center()
            .justify_center();

        if let Some(frame_img) = w.rendered_image.as_ref() {
            viewport = viewport.child(
                img(frame_img.clone())
                    .w_full()
                    .h_full()
                    .object_fit(ObjectFit::Cover),
            );
        } else {
            let (icon_c, title, msg, can_retry) = if !w.preview_on {
                (
                    ui::TEXT3,
                    "Aperçu vidéo en pause",
                    "La caméra est libre pour OBS ou Discord. Clique sur « Reprendre » pour voir tes réglages en direct.",
                    false,
                )
            } else {
                match &w.preview_status {
                    PreviewStatus::Starting => (ui::ACCENT, "Connexion au flux vidéo…", "Ouverture vidéo seule (le micro n’est jamais ouvert).", false),
                    PreviewStatus::Busy(m) => (ui::ORANGE, "Flux vidéo déjà ouvert ailleurs", m.as_str(), true),
                    PreviewStatus::Disconnected => (ui::RED, "Webcam C920 non détectée en vidéo", "Vérifie le branchement USB de la C920.", true),
                    PreviewStatus::Error(m) => (ui::RED, "Impossible d’afficher l’aperçu", m.as_str(), true),
                    _ => (ui::TEXT3, "Aperçu prêt", "Clique sur « Reprendre » pour afficher l’image en direct.", false),
                }
            };
            viewport = viewport.child(
                div()
                    .px(px(24.))
                    .flex()
                    .flex_col()
                    .items_center()
                    .gap(px(8.))
                    .child(ui::chip(Icon::Camera, icon_c, 44.))
                    .child(div().text_size(px(14.5)).font_weight(FontWeight::SEMIBOLD).text_color(rgb(ui::WHITE)).child(title))
                    .child(div().max_w(px(360.)).text_center().text_size(px(12.)).line_height(px(18.)).text_color(alpha(ui::WHITE, 0.72)).child(msg.to_owned()))
                    .when(can_retry, |d| {
                        d.child(
                            ui::pill("c920-preview-retry", Some(Icon::Refresh), "Réessayer l’aperçu", Tone::Primary)
                                .mt(px(4.))
                                .on_click(cx.listener(|this, _, _, cx| this.retry_webcam_preview(cx))),
                        )
                    }),
            );
        }

        // Badge discret en haut à gauche sur l'image
        viewport = viewport.child(
            div()
                .absolute()
                .top(px(10.))
                .left(px(10.))
                .px(px(10.))
                .py(px(5.))
                .rounded_full()
                .bg(alpha(0x000000, 0.62))
                .flex()
                .items_center()
                .gap(px(6.))
                .child(div().size(px(7.)).rounded_full().bg(rgb(badge_colour)))
                .child(div().text_size(px(11.5)).font_weight(FontWeight::SEMIBOLD).text_color(rgb(ui::WHITE)).child(badge_text)),
        );

        let exp_short = if s.exposure_auto { "Auto".into() } else { format_exposure(f64::from(s.exposure)) };
        let foc_short = if s.focus_auto { "Auto".into() } else { format!("Manuel ({})", s.focus) };
        let wb_short = if s.white_balance_auto { "Auto".into() } else { format_kelvin(f64::from(s.white_balance)) };
        let lock_short = if s.locked {
            if s.reapply_count > 0 { format!("Verrouillé ({})", s.reapply_count) } else { "Verrouillé".into() }
        } else {
            "Libre".into()
        };

        let controls_bar = div()
            .flex()
            .flex_wrap()
            .items_center()
            .justify_between()
            .gap(px(8.))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .child(
                        ui::pill(
                            "c920-preview-toggle",
                            Some(if w.preview_on { Icon::Stop } else { Icon::Play }),
                            if w.preview_on { "Mettre en pause" } else { "Afficher l’aperçu" },
                            if w.preview_on { Tone::Plain } else { Tone::Primary },
                        )
                        .on_click(cx.listener(|this, _, _, cx| this.toggle_webcam_preview(cx))),
                    )
                    .child(
                        ui::pill("c920-preview-mirror", Some(Icon::Refresh), if w.mirror { "Miroir : oui" } else { "Miroir : non" }, Tone::Plain)
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.webcam.mirror = !this.webcam.mirror;
                                cx.notify();
                            })),
                    ),
            )
            .child(ui::caption("Vidéo seule · libérée dès que tu quittes cette page"));

        let telemetry = div()
            .p(px(10.))
            .rounded(px(10.))
            .bg(alpha(ui::INK, 0.035))
            .flex()
            .flex_wrap()
            .gap_x(px(22.))
            .gap_y(px(8.))
            .when(stale, |d| d.opacity(0.5))
            .child(ui::fact(preset_label(s.active_preset.as_deref()), "Préréglage"))
            .child(ui::fact(exp_short, "Exposition"))
            .child(ui::fact(wb_short, "Balance"))
            .child(ui::fact(foc_short, "Focus"))
            .child(ui::fact(format!("{} %", s.zoom), "Zoom"))
            .child(ui::fact(lock_short, "Maintien"));

        ui::section(Icon::Camera, ui::ACCENT, "Aperçu en direct", None)
            .gap(px(12.))
            .child(viewport)
            .child(controls_bar)
            .child(telemetry)
    }

    fn webcam_presets_card(&self, s: &Settings, enabled: bool, cx: &mut Context<Self>) -> Div {
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
                        this.param_held.clear();
                        this.webcam_send("preset", json!({"id": id}), "Préréglage appliqué", cx);
                    }))
                })
                .into_any_element(),
            );
        }
        ui::section(Icon::Sparkles, ui::ACCENT, "Préréglages en un clic", Some("Adaptés à ta C920".into()))
            .child(ui::grid(tiles, 2))
    }

    fn webcam_guard_card(&self, s: &Settings, enabled: bool, cx: &mut Context<Self>) -> Div {
        let locked = s.locked;
        let aside = if locked {
            if s.reapply_count > 0 {
                format!("Actif · {} correction(s)", s.reapply_count)
            } else {
                "Actif · OBS / Discord bloqués".into()
            }
        } else {
            "Inactif".into()
        };
        let mut card = ui::section(Icon::Shield, if locked { ui::GREEN } else { ui::TEXT3 }, "Garder mes réglages", Some(aside.into()))
            .child(ui::list_row(
                None,
                if locked { "Maintien anti-réinitialisation actif" } else { "Activer le maintien automatique" },
                Some("Réapplique tes choix dès que Windows, OBS ou Discord tente de les remettre à zéro.".into()),
                ui::toggle("c920-lock", locked, enabled).when(enabled, |d| {
                    d.on_click(cx.listener(move |this, _, _, cx| {
                        let next = !locked;
                        let msg = if next { "Maintien activé" } else { "Maintien désactivé" };
                        this.webcam_send("lock", json!({"enabled": next}), msg, cx);
                    }))
                }),
                true,
            ));
        if locked && !s.last_reapplied_fields.is_empty() {
            let names: Vec<&str> = s.last_reapplied_fields.iter().map(|f| field_label(f)).collect();
            card = card.child(ui::caption(format!("Dernière correction automatique : {}.", names.join(", "))));
        }
        if !s.backoff_fields.is_empty() {
            let names: Vec<&str> = s.backoff_fields.iter().map(|f| field_label(f)).collect();
            card = card.child(ui::notice(false, format!("Réglage en pause (refusé par le pilote) : {}.", names.join(", "))));
        }
        card
    }

    /// Onglet 1 : Image & Couleur (Lumière, Couleurs & Netteté, Balance des blancs, 50/60 Hz).
    fn webcam_image_tab(&self, s: &Settings, enabled: bool, cx: &mut Context<Self>) -> Div {
        let r = &s.ranges;
        let (b_range, b_step) = safe_range(r.brightness);
        let (c_range, c_step) = safe_range(r.contrast);
        let (sat_range, sat_step) = safe_range(r.saturation);
        let (sh_range, sh_step) = safe_range(r.sharpness);
        let (g_range, g_step) = safe_range(r.gain);
        let (wb_range, wb_step) = safe_range(r.white_balance);
        let wb_auto = s.white_balance_auto;
        let blc = s.backlight_comp;

        let mut pl = ui::segmented();
        for (val, label) in [("off", "Coupé"), ("hz50", "50 Hz (Europe)"), ("hz60", "60 Hz")] {
            let selected = s.power_line == val;
            pl = pl.child(
                ui::seg(SharedString::from(format!("c920-pl-{val}")), label, selected).when(enabled && !selected, |d| {
                    d.on_click(cx.listener(move |this, _, _, cx| {
                        this.webcam_send("settings", json!({"power_line": val}), "Anti-scintillement appliqué", cx);
                    }))
                }),
            );
        }

        let light = ui::section(Icon::Sun, ui::ORANGE, "Lumière et contraste", None)
            .child(self.webcam_slider_row(
                "c920-brightness", "Luminosité", f64::from(s.brightness), b_range, b_step,
                |v| format!("{}", v.round() as i32), enabled,
                |this, v, cx| this.webcam_send("settings", json!({"brightness": v.round() as i32}), "Luminosité appliquée", cx), cx,
            ))
            .child(self.webcam_slider_row(
                "c920-contrast", "Contraste", f64::from(s.contrast), c_range, c_step,
                |v| format!("{}", v.round() as i32), enabled,
                |this, v, cx| this.webcam_send("settings", json!({"contrast": v.round() as i32}), "Contraste appliqué", cx), cx,
            ))
            .child(self.webcam_slider_row(
                "c920-gain", "Gain capteur (lumière en soirée)", f64::from(s.gain), g_range, g_step,
                |v| format!("{}", v.round() as i32), enabled,
                |this, v, cx| this.webcam_send("settings", json!({"gain": v.round() as i32}), "Gain appliqué", cx), cx,
            ))
            .child(ui::list_row(
                None,
                "Compensation de contre-jour",
                Some("Éclaircit le visage s’il y a une fenêtre ou une lampe derrière toi.".into()),
                ui::toggle("c920-blc", blc, enabled).when(enabled, |d| {
                    d.on_click(cx.listener(move |this, _, _, cx| {
                        this.webcam_send("settings", json!({"backlight_comp": !blc}), "Contre-jour mis à jour", cx);
                    }))
                }),
                false,
            ));

        let color = ui::section(Icon::Palette, 0xaf52de, "Couleurs, netteté et balance des blancs", None)
            .child(self.webcam_slider_row(
                "c920-saturation", "Saturation des couleurs", f64::from(s.saturation), sat_range, sat_step,
                |v| format!("{}", v.round() as i32), enabled,
                |this, v, cx| this.webcam_send("settings", json!({"saturation": v.round() as i32}), "Saturation appliquée", cx), cx,
            ))
            .child(self.webcam_slider_row(
                "c920-sharpness", "Netteté (trop haut crée un halo numérique)", f64::from(s.sharpness), sh_range, sh_step,
                |v| format!("{}", v.round() as i32), enabled,
                |this, v, cx| this.webcam_send("settings", json!({"sharpness": v.round() as i32}), "Netteté appliquée", cx), cx,
            ))
            .child(ui::list_row(
                None,
                "Balance des blancs automatique",
                Some("Coupe-la pour figer la teinte de ta peau (évite les virages bleu/jaune en live).".into()),
                ui::toggle("c920-wb-auto", wb_auto, enabled && r.white_balance.supports_auto).when(enabled && r.white_balance.supports_auto, |d| {
                    d.on_click(cx.listener(move |this, _, _, cx| {
                        this.webcam_send("settings", json!({"white_balance_auto": !wb_auto}), "Balance des blancs mise à jour", cx);
                    }))
                }),
                false,
            ))
            .child(
                div().when(wb_auto, |d| d.opacity(0.55)).child(self.webcam_slider_row(
                    "c920-wb", "Température (manuelle, en Kelvin)", f64::from(s.white_balance), wb_range, wb_step,
                    format_kelvin, enabled,
                    |this, v, cx| this.webcam_send("settings", json!({"white_balance_auto": false, "white_balance": v.round() as i32}), "Balance des blancs appliquée", cx), cx,
                )),
            )
            .child(ui::subhead("Anti-scintillement (éclairage néon / LED)"))
            .child(pl);

        div().flex().flex_col().gap(px(16.)).child(light).child(color)
    }

    /// Onglet 2 : Optique & Cadrage (Exposition 30 ips, Autofocus + distance, Zoom / Pan / Tilt).
    fn webcam_optics_tab(&self, s: &Settings, enabled: bool, cx: &mut Context<Self>) -> Div {
        let r = &s.ranges;
        let (exp_range, exp_step) = safe_range(r.exposure);
        let (foc_range, foc_step) = safe_range(r.focus);
        let (zoom_range, zoom_step) = safe_range(r.zoom);
        let (pan_range, pan_step) = safe_range(r.pan);
        let (tilt_range, tilt_step) = safe_range(r.tilt);
        let exp_auto = s.exposure_auto;
        let llc = s.low_light_comp;
        let foc_auto = s.focus_auto;
        let can_pan = enabled && s.zoom > 100;

        let exposure = ui::section(Icon::Gauge, ui::ORANGE, "Exposition et fluidité 30 ips", None)
            .child(ui::list_row(
                None,
                "Exposition automatique",
                Some("Coupe-la pour garder 30 ips fluides et une lumière constante.".into()),
                ui::toggle("c920-exp-auto", exp_auto, enabled && r.exposure.supports_auto).when(enabled && r.exposure.supports_auto, |d| {
                    d.on_click(cx.listener(move |this, _, _, cx| {
                        this.webcam_send("settings", json!({"exposure_auto": !exp_auto}), "Mode d’exposition mis à jour", cx);
                    }))
                }),
                true,
            ))
            .child(
                div().when(exp_auto, |d| d.opacity(0.55)).child(self.webcam_slider_row(
                    "c920-exposure", "Vitesse d’obturation (manuelle)", f64::from(s.exposure), exp_range, exp_step,
                    format_exposure, enabled,
                    |this, v, cx| this.webcam_send("settings", json!({"exposure_auto": false, "exposure": v.round() as i32}), "Exposition appliquée", cx), cx,
                )),
            )
            .child(ui::list_row(
                None,
                "Compensation basse lumière",
                Some("Conseil stream : laisse-la coupée, sinon la C920 tombe à 15 ips dès qu’il fait sombre.".into()),
                ui::toggle("c920-llc", llc, enabled).when(enabled, |d| {
                    d.on_click(cx.listener(move |this, _, _, cx| {
                        this.webcam_send("settings", json!({"low_light_comp": !llc}), "Compensation basse lumière mise à jour", cx);
                    }))
                }),
                false,
            ));

        let focus_quick = div()
            .flex()
            .flex_wrap()
            .gap(px(6.))
            .child(ui::caption("Raccourcis distance :"))
            .child(
                ui::pill("c920-foc-close", None, "Proche (45)", Tone::Plain)
                    .when(!enabled, |d| d.opacity(0.45))
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.param_held.clear();
                        this.webcam_send("settings", json!({"focus_auto": false, "focus": 45}), "Mise au point proche", cx);
                    })),
            )
            .child(
                ui::pill("c920-foc-desk", None, "Bureau (30)", Tone::Soft)
                    .when(!enabled, |d| d.opacity(0.45))
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.param_held.clear();
                        this.webcam_send("settings", json!({"focus_auto": false, "focus": 30}), "Mise au point bureau", cx);
                    })),
            )
            .child(
                ui::pill("c920-foc-far", None, "Pièce / fond (10)", Tone::Plain)
                    .when(!enabled, |d| d.opacity(0.45))
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.param_held.clear();
                        this.webcam_send("settings", json!({"focus_auto": false, "focus": 10}), "Mise au point lointaine", cx);
                    })),
            );

        let focus = ui::section(Icon::Camera, 0x5856d6, "Mise au point", None)
            .child(ui::list_row(
                None,
                "Autofocus",
                Some("Verrouille en manuel si la caméra fait des allers-retours de netteté quand tu bouges.".into()),
                ui::toggle("c920-focus-auto", foc_auto, enabled && r.focus.supports_auto).when(enabled && r.focus.supports_auto, |d| {
                    d.on_click(cx.listener(move |this, _, _, cx| {
                        this.webcam_send("settings", json!({"focus_auto": !foc_auto}), "Mise au point mise à jour", cx);
                    }))
                }),
                true,
            ))
            .child(
                div().when(foc_auto, |d| d.opacity(0.55)).child(self.webcam_slider_row(
                    "c920-focus", "Distance de mise au point (manuelle)", f64::from(s.focus), foc_range, foc_step,
                    |v| format!("{}", v.round() as i32), enabled,
                    |this, v, cx| this.webcam_send("settings", json!({"focus_auto": false, "focus": v.round() as i32}), "Distance de mise au point appliquée", cx), cx,
                )),
            )
            .child(focus_quick);

        let framing = ui::section(Icon::Sliders, ui::ACCENT, "Cadrage numérique (Zoom, Pan, Tilt)", None)
            .child(self.webcam_slider_row(
                "c920-zoom", "Zoom", f64::from(s.zoom), zoom_range, zoom_step,
                |v| format!("{} %", v.round() as i32), enabled,
                |this, v, cx| this.webcam_send("settings", json!({"zoom": v.round() as i32}), "Zoom appliqué", cx), cx,
            ))
            .child(
                div().when(!can_pan, |d| d.opacity(0.45)).child(self.webcam_slider_row(
                    "c920-pan", "Panoramique (gauche / droite)", f64::from(s.pan), pan_range, pan_step,
                    |v| format!("{:+}°", v.round() as i32), can_pan,
                    |this, v, cx| this.webcam_send("settings", json!({"pan": v.round() as i32}), "Panoramique appliqué", cx), cx,
                )),
            )
            .child(
                div().when(!can_pan, |d| d.opacity(0.45)).child(self.webcam_slider_row(
                    "c920-tilt", "Inclinaison (bas / haut)", f64::from(s.tilt), tilt_range, tilt_step,
                    |v| format!("{:+}°", v.round() as i32), can_pan,
                    |this, v, cx| this.webcam_send("settings", json!({"tilt": v.round() as i32}), "Inclinaison appliquée", cx), cx,
                )),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap(px(8.))
                    .child(ui::caption(if s.zoom <= 100 { "Monte le zoom au-dessus de 100 % pour débloquer panoramique et inclinaison." } else { "Déplace le cadre en direct sur l’aperçu." }))
                    .when(enabled && (s.zoom != 100 || s.pan != 0 || s.tilt != 0), |d| {
                        d.child(
                            ui::pill("c920-recenter", Some(Icon::History), "Recentrer", Tone::Plain)
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.param_held.clear();
                                    this.webcam_send("settings", json!({"zoom": 100, "pan": 0, "tilt": 0}), "Cadrage recentré", cx);
                                })),
                        )
                    }),
            );

        div().flex().flex_col().gap(px(16.)).child(exposure).child(focus).child(framing)
    }

    /// Onglet 3 : Appareil & LED (LED bleue XU, fiche technique C920, réinitialisation usine).
    fn webcam_device_tab(&self, s: &Settings, enabled: bool, cx: &mut Context<Self>) -> Div {
        let mut col = div().flex().flex_col().gap(px(16.));
        if let Some(current) = s.led_mode.as_deref() {
            let mut led = ui::segmented();
            for (val, label) in [("auto", "Automatique"), ("on", "Allumée"), ("off", "Éteinte"), ("blinking", "Clignotante")] {
                let selected = current == val;
                led = led.child(
                    ui::seg(SharedString::from(format!("c920-led-{val}")), label, selected).when(enabled && !selected, |d| {
                        d.on_click(cx.listener(move |this, _, _, cx| {
                            this.webcam_send("settings", json!({"led_mode": val}), "Voyant LED mis à jour", cx);
                        }))
                    }),
                );
            }
            col = col.child(
                ui::section(Icon::Sparkles, ui::ACCENT, "Voyant LED bleu (contrôle Logitech XU)", Some("Spécifique C920".into()))
                    .child(led)
                    .child(ui::caption("Contrôle matériel de la LED autour de la lentille (utile si la lumière bleue se reflète dans tes lunettes).")),
            );
        }

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
                .map(|f| format!("{} {}×{} @ {} ips", f.codec, f.width, f.height, f.max_fps))
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
        let specs = ui::section(Icon::Settings, ui::TEXT2, "Fiche technique de ta Logitech C920", None)
            .gap(px(4.))
            .child(row("Modèle", s.device.name.clone()))
            .child(row("Révision", s.device.revision_label.clone()))
            .child(row("Identifiant USB", format!("{:04X}:{:04X} (rév. {})", s.device.vid, s.device.pid, s.device.revision)))
            .child(row("Firmware", s.device.firmware.clone().unwrap_or_else(|| "Non annoncé".into())))
            .when_some(s.device.dsp_platform.clone(), |d, dsp| d.child(row("Plateforme DSP", dsp)))
            .child(row("Formats vidéo", formats))
            .child(row("Unités XU", xu_summary))
            .when(enabled, |d| {
                d.child(
                    div().pt(px(12.)).child(
                        ui::pill("c920-info-reset", Some(Icon::History), "Réinitialiser aux valeurs d’usine", Tone::Warn)
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.param_held.clear();
                                this.webcam_send("reset", json!({}), "Réglages d’usine restaurés", cx);
                            })),
                    ),
                )
            })
            .child(ui::caption("Les réglages UVC et XU agissent directement sur le capteur sans toucher au micro de la webcam ni flasher le firmware.").pt(px(6.)));

        col.child(specs)
    }

    pub fn webcam_view(&self, window: &Window, cx: &mut Context<Self>) -> Div {
        let w = &self.webcam;
        let width = crate::desktop::page_width(window);
        let mut content = div().flex().flex_col().gap(px(16.));
        let state = w.status.as_ref().map(|v| v.device.status.as_str());
        let waiting = match state {
            Some("disconnected") => Some(("Logitech C920 introuvable", "Branche ta webcam C920 en USB. Ses réglages et son aperçu apparaissent tout seuls.")),
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
        let enabled = !stale;

        let tab = w.tab;
        let mut tabs = ui::tabs();
        for (target, glyph, label) in [
            (WebcamTab::Image, Icon::Sun, "Image et couleur"),
            (WebcamTab::Optics, Icon::Sliders, "Optique et cadrage"),
            (WebcamTab::Device, Icon::Settings, "Appareil et LED"),
        ] {
            tabs = tabs.child(
                ui::tab(SharedString::from(format!("c920-tab-{label}")), glyph, label, tab == target)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.webcam.tab = target;
                        cx.notify();
                    })),
            );
        }
        let tab_body = match tab {
            WebcamTab::Image => self.webcam_image_tab(s, enabled, cx),
            WebcamTab::Optics => self.webcam_optics_tab(s, enabled, cx),
            WebcamTab::Device => self.webcam_device_tab(s, enabled, cx),
        };

        let left_w = if width >= 920. { (width * 0.46).clamp(400., 540.) } else { width.min(640.) };
        let preview_h = (left_w * 9. / 16.).round().clamp(210., 320.);

        let left_col = div()
            .flex()
            .flex_col()
            .gap(px(16.))
            .child(self.webcam_preview_card(s, preview_h, stale, cx))
            .child(self.webcam_presets_card(s, enabled, cx))
            .child(self.webcam_guard_card(s, enabled, cx));

        let right_col = div()
            .flex()
            .flex_col()
            .gap(px(10.))
            .child(tabs)
            .child(tab_body);

        if width >= 920. {
            content.child(
                div()
                    .flex()
                    .items_start()
                    .gap(px(24.))
                    .child(left_col.w(px(left_w)).flex_none())
                    .child(right_col.flex_1().min_w(px(0.))),
            )
        } else {
            content.child(left_col).child(right_col)
        }
    }
}

trait OptionReplaceExt<T> {
    fn replace_if(&mut self, next: Option<T>) -> Option<T>;
}
impl<T> OptionReplaceExt<T> for Option<T> {
    fn replace_if(&mut self, next: Option<T>) -> Option<T> {
        match next {
            Some(v) => self.replace(v),
            None => self.take(),
        }
    }
}

fn default_presets() -> Vec<PresetSummary> {
    vec![
        PresetSummary {
            id: "studio".into(),
            label: "Studio / Stream".into(),
            summary: "Exposition manuelle fluide (30 ips), basse lumière coupée, balance verrouillée et 50 Hz.".into(),
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
            summary: "Rendu neutre pour les appels quotidiens : exposition, balance et focus automatiques.".into(),
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

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_settings() -> Settings {
        let cr = ControlRange { min: 0, max: 255, step: 1, default: 128, supports_auto: true };
        Settings {
            device: DeviceInfo {
                name: "HD Pro Webcam C920".into(),
                vid: 0x046D,
                pid: 0x0892,
                revision: "0019".into(),
                firmware: Some("7.59.1035".into()),
                dsp_platform: Some("PYTHON_V2B".into()),
                revision_label: "C920 / C920 HD Pro".into(),
                formats: vec![],
                xu: XuInfo::default(),
            },
            brightness: 128,
            contrast: 128,
            saturation: 128,
            sharpness: 128,
            gain: 0,
            white_balance_auto: true,
            white_balance: 4000,
            backlight_comp: false,
            power_line: "hz50".into(),
            exposure_auto: false,
            exposure: -5,
            low_light_comp: false,
            focus_auto: true,
            focus: 30,
            zoom: 100,
            pan: 0,
            tilt: 0,
            led_mode: Some("auto".into()),
            ranges: Ranges {
                brightness: cr,
                contrast: cr,
                saturation: cr,
                sharpness: cr,
                gain: cr,
                white_balance: ControlRange { min: 2000, max: 6500, step: 1, default: 4000, supports_auto: true },
                power_line: ControlRange { min: 0, max: 2, step: 1, default: 1, supports_auto: false },
                exposure: ControlRange { min: -11, max: -2, step: 1, default: -5, supports_auto: true },
                focus: ControlRange { min: 0, max: 250, step: 5, default: 30, supports_auto: true },
                zoom: ControlRange { min: 100, max: 500, step: 1, default: 100, supports_auto: false },
                pan: ControlRange { min: -10, max: 10, step: 1, default: 0, supports_auto: false },
                tilt: ControlRange { min: -10, max: 10, step: 1, default: 0, supports_auto: false },
            },
            locked: false,
            active_preset: Some("studio".into()),
            reapply_count: 0,
            last_reapplied_fields: vec![],
            backoff_fields: vec![],
        }
    }

    #[test]
    fn apercu_demo_reagit_aux_reglages_sans_tiret_cadratin() {
        let mut s = sample_settings();
        let img_a = render_demo_frame(&s, false).expect("image demo");
        assert_eq!(img_a.size(0).width.0, 480);
        assert_eq!(img_a.size(0).height.0, 270);
        s.brightness = 210;
        s.zoom = 180;
        let img_b = render_demo_frame(&s, false).expect("image modifiee");
        assert_ne!(img_a.as_bytes(0), img_b.as_bytes(0), "l'apercu doit reagir en temps reel aux curseurs");
        for p in default_presets() {
            assert!(!p.label.contains('\u{2014}') && !p.summary.contains('\u{2014}'));
            assert!(!p.label.contains('\u{2013}') && !p.summary.contains('\u{2013}'));
        }
    }
}
