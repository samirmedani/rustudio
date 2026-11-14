use crate::backend::{self, Channel, Client, Command, Event, Fader, Snapshot};
use crate::desktop::{Confirm, Page};
use crate::profiles::{Profile, Store};
use crate::ui::{self, Icon, alpha, rgb};
use gpui::{
    AnyElement, Bounds, Context, CursorStyle, Div, ElementId, FocusHandle, FontWeight, IntoElement, MouseButton, Pixels,
    Render, SharedString, Stateful, Window, canvas, div, linear_color_stop, linear_gradient, prelude::*, px,
};
use serde_json::{Value, json};
use std::{
    cell::Cell,
    collections::HashMap,
    rc::Rc,
    sync::mpsc,
    time::{Duration, Instant},
};

const TRAVEL: f32 = 214.;
const FADER_W: f32 = 68.;
const FADER_R: f32 = 22.;
const VOLUME_INTERVAL: Duration = Duration::from_millis(45);

struct Gesture {
    channel: String,
    bounds: Bounds<Pixels>,
    vertical: bool,
    value: u8,
    sent_at: Option<Instant>,
}

pub struct Mixer {
    pub(crate) snapshot: Option<Snapshot>,
    pub(crate) client: Client,
    pub(crate) inventory: Option<backend::Inventory>,
    pub(crate) page: Page,
    pub(crate) mouse: crate::mouse::MousePage,
    pub(crate) keyboard: crate::keyboard::KeyboardPage,
    pub(crate) mouse_focus: Vec<FocusHandle>,
    pub(crate) go: crate::goxlr::GoPage,
    pub(crate) go_focus: Vec<FocusHandle>,
    pub(crate) win: crate::win_session::WinPage,
    pub(crate) voice: crate::voice::VoiceExtras,
    pub(crate) library: crate::library::Library,
    pub(crate) store: Store,
    pub(crate) profiles: Vec<(usize, Profile)>,
    pub(crate) profile_name: String,
    pub(crate) profile_focus: FocusHandle,
    pub(crate) profile_notice: Option<(bool, String)>,
    pub(crate) profile_request: Option<u64>,
    pub(crate) confirm: Option<Confirm>,
    pub(crate) menu_open: bool,
    pub(crate) select_open: Option<SharedString>,
    sender: mpsc::SyncSender<Command>,
    gesture: Option<Gesture>,
    pending: HashMap<String, (u64, u8)>,
    picker: Option<usize>,
    pub(crate) notice: Option<String>,
    toast_key: String,
    pub(crate) param_drag: Option<ParamDrag>,
    pub(crate) param_held: HashMap<String, (f64, Instant)>,
    pub(crate) history: Vec<Page>,
    pub(crate) nav_count: u64,
    pub(crate) last_space: Option<crate::desktop::Space>,
    pub(crate) update: Option<Value>,
    /// recherche demandée à la main : roue dans la barre latérale jusqu'à la réponse
    pub(crate) update_then_open: bool,
    /// fenêtre de mise à jour (notes, installation, « installé »)
    pub(crate) sheet: Option<crate::update_sheet::Sheet>,
    /// l'installation qui vient de finir a déjà rouvert sa fenêtre
    pub(crate) installed_seen: bool,
    /// résultat d'une recherche à la main (« Rustudio est à jour »…)
    pub(crate) update_notice: Option<(bool, String)>,
    pub(crate) sheet_preview_done: bool,
    seq: u64,
    focus: Vec<FocusHandle>,
    received: usize,
    renders: usize,
    started: Instant,
}

impl Mixer {
    pub fn new(
        client: Client,
        store: Store,
        initial: Page,
        native_osd: bool,
        duration: Option<Duration>,
        started: Instant,
        cx: &mut Context<Self>,
    ) -> Self {
        let (sender, events) = backend::start(client.clone(), None, native_osd);
        let (profiles, ignored) = store.list();
        cx.spawn(async move |this, cx| {
            while let Ok(event) = events.recv().await {
                if this
                    .update(cx, |mixer, cx| mixer.apply_event(event, cx))
                    .is_err()
                {
                    break;
                }
            }
        })
        .detach();
        if let Some(duration) = duration {
            let exec = cx.background_executor().clone();
            cx.spawn(async move |this, cx| {
                exec.timer(duration).await;
                let _ = this.update(cx, |mixer, cx| {
                    println!("snapshots={} renders={}", mixer.received, mixer.renders);
                    cx.quit();
                });
            })
            .detach();
        }
        Self {
            snapshot: None,
            client,
            inventory: None,
            mouse: crate::mouse::MousePage::default(),
            keyboard: crate::keyboard::KeyboardPage::default(),
            mouse_focus: (0..12).map(|_| cx.focus_handle()).collect(),
            go: crate::goxlr::GoPage::default(),
            win: crate::win_session::WinPage::default(),
            voice: crate::voice::VoiceExtras::default(),
            library: crate::library::Library::default(),
            go_focus: (0..3).map(|_| cx.focus_handle()).collect(),
            page: initial,
            store,
            profiles,
            profile_name: String::new(),
            profile_focus: cx.focus_handle(),
            profile_notice: (ignored > 0).then(|| {
                (
                    false,
                    format!("{ignored} profil(s) illisible(s) ignoré(s)."),
                )
            }),
            profile_request: None,
            confirm: None,
            menu_open: false,
            select_open: None,
            sender,
            gesture: None,
            pending: HashMap::new(),
            picker: None,
            notice: None,
            toast_key: String::new(),
            param_drag: None,
            param_held: HashMap::new(),
            history: vec![],
            nav_count: 0,
            last_space: None,
            update: None,
            update_then_open: false,
            sheet: None,
            installed_seen: false,
            update_notice: None,
            sheet_preview_done: false,
            seq: 0,
            focus: (0..15).map(|_| cx.focus_handle()).collect(),
            received: 0,
            renders: 0,
            started,
        }
    }

    /// Change de page et règle les lectures en direct de la page visible.
    pub fn go_to(&mut self, page: Page, cx: &mut Context<Self>) {
        if page != self.page {
            self.history.push(self.page);
            if self.history.len() > 40 {
                self.history.remove(0);
            }
        }
        self.show(page, cx);
    }

    /// Page précédente (bouton Retour, bouton « précédent » de la souris).
    pub fn go_back(&mut self, cx: &mut Context<Self>) {
        if let Some(page) = self.history.pop() {
            self.show(page, cx);
        }
    }

    fn show(&mut self, page: Page, cx: &mut Context<Self>) {
        if page != self.page {
            self.nav_count += 1;
        }
        match page.space() {
            Some(space) => self.last_space = Some(space),
            None if page == Page::Devices => self.last_space = None,
            None => {}
        }
        self.page = page;
        self.client.set_mixer_visible(page == Page::Mixer);
        self.client.set_mouse_visible(page == Page::Mouse);
        self.client.set_keyboard_visible(page == Page::Keyboard);
        self.client.set_go_visible(matches!(page, Page::Go(_)));
        self.client.set_level_visible(page == Page::Go(crate::goxlr::Section::Microphone));
        self.client.set_latency_visible(page == Page::Go(crate::goxlr::Section::Diagnostic));
        self.client.set_audio_visible(matches!(page, Page::WindowsAudio | Page::Mixer));
        self.win.cancel();
        if page == Page::Go(crate::goxlr::Section::Microphone) {
            self.load_voice_info(cx);
        }
        // le diagnostic se lit dès l'ouverture de la page, sans attendre « Actualiser »
        if page == Page::Go(crate::goxlr::Section::Diagnostic) {
            self.refresh_diag(cx);
        }
        if matches!(page, Page::Catalogue | Page::Versions | Page::Help | Page::Settings) {
            self.library_load(page, cx);
        }
        self.go.clear_confirm();
        self.confirm = None;
        ui::reset_hover();
        self.menu_open = false;
        self.select_open = None;
        self.picker = None;
        cx.notify();
    }

    fn apply_event(&mut self, event: Event, cx: &mut Context<Self>) {
        let was_enabled = self.enabled();
        let mut force_render = true;
        match event {
            Event::Snapshot(Some(next)) => {
                force_render = self.page == Page::Mixer;
                self.received += 1;
                if let Some(old) = &self.snapshot {
                    if old.boot != next.boot || !next.connected {
                        self.pending.clear();
                        self.gesture = None;
                        self.picker = None;
                    }
                    for (old_fader, next_fader) in old.faders.iter().zip(next.faders.iter()) {
                        if old_fader.position != next_fader.position {
                            self.pending.remove(&next_fader.channel);
                        }
                    }
                }
                self.pending.retain(|id, (_, value)| {
                    next.channels
                        .iter()
                        .find(|row| row.id == *id)
                        .is_some_and(|row| row.volume != *value)
                });
                self.snapshot = Some(next);
            }
            Event::Snapshot(None) => {
                force_render = self.page == Page::Mixer;
                self.snapshot = None;
                self.gesture = None;
                self.pending.clear();
                self.picker = None;
            }
            Event::Inventory(inventory) => self.inventory = inventory,
            Event::Update(update) => {
                self.note_installed(&update, cx);
                force_render = force_render || self.sheet.is_some();
                self.update = Some(update);
            }
            Event::Audio(data) => {
                self.win.inventory = Some(data);
                force_render = matches!(self.page, Page::WindowsAudio | Page::Mixer);
            }
            Event::MicOsd(state) => {
                crate::win_session::show_mic(&state);
                force_render = false;
            }
            Event::MicLevel(level) => {
                self.go.level = level;
                force_render = self.page == Page::Go(crate::goxlr::Section::Microphone);
            }
            Event::Latency(value) => {
                self.go.latency = Some(value);
                force_render = self.page == Page::Go(crate::goxlr::Section::Diagnostic);
            }
            Event::Go(view) => {
                self.go.receive(view);
                force_render = matches!(self.page, Page::Go(_));
            }
            Event::Mouse(view) => {
                self.mouse.receive(view);
                force_render = self.page == Page::Mouse;
            }
            Event::Keyboard(view) => {
                self.keyboard.receive(view);
                force_render = self.page == Page::Keyboard;
            }
            Event::Done(seq, result) => {
                if self.profile_request == Some(seq) {
                    self.profile_request = None;
                    self.profile_notice = Some(match &result {
                        Ok(()) => (true, "Profil appliqué au mixeur.".into()),
                        Err(err) => (false, err.clone()),
                    });
                }
                if let Err(err) = result {
                    self.pending.retain(|_, (token, _)| *token != seq);
                    self.notice = Some(err);
                } else {
                    // L’état publié est la source de vérité, même si un fader physique a déjà repris la main.
                    self.pending.retain(|_, (token, _)| *token != seq);
                }
            }
        }
        if force_render || was_enabled != self.enabled() {
            cx.notify();
        }
    }

    pub(crate) fn enabled(&self) -> bool {
        self.snapshot
            .as_ref()
            .is_some_and(|s| s.connected && s.health == "ok")
    }

    pub(crate) fn enqueue(
        &mut self,
        action: &'static str,
        params: Value,
        cx: &mut Context<Self>,
    ) -> Option<u64> {
        if !self.enabled() {
            return None;
        }
        let boot = self.snapshot.as_ref()?.boot.clone();
        self.seq += 1;
        let id = self.seq;
        self.notice = self
            .sender
            .try_send(Command {
                id,
                boot,
                action,
                params,
            })
            .err()
            .map(|_| "Commande non envoyée : moteur occupé.".into());
        cx.notify();
        self.notice.is_none().then_some(id)
    }

    fn set_volume(&mut self, channel: String, value: u8, cx: &mut Context<Self>) {
        if let Some(id) = self.enqueue("volume", json!({"channel":channel, "value":value}), cx) {
            self.pending.insert(channel, (id, value));
        }
    }

    fn toggle(&mut self, channel: &Channel, mode: &str, cx: &mut Context<Self>) {
        self.enqueue(
            "mute",
            json!({"channel":channel.id, "muted":channel.mute.is_none(), "target":mode}),
            cx,
        );
    }

    fn volume(&self, channel: &Channel) -> u8 {
        self.gesture
            .as_ref()
            .filter(|g| g.channel == channel.id)
            .map(|g| g.value)
            .or_else(|| self.pending.get(&channel.id).map(|(_, value)| *value))
            .unwrap_or(channel.volume)
    }

    fn drag_to(&mut self, position: gpui::Point<Pixels>, cx: &mut Context<Self>) {
        if let Some(g) = &mut self.gesture {
            let next = position_value(g.bounds, position, g.vertical);
            if g.value != next {
                g.value = next;
                let now = Instant::now();
                let update = if g.sent_at.is_none_or(|last| now - last >= VOLUME_INTERVAL) {
                    g.sent_at = Some(now);
                    Some(g.channel.clone())
                } else {
                    None
                };
                if let Some(channel) = update {
                    self.set_volume(channel, next, cx);
                }
                cx.notify();
            }
        }
    }

    /// Bouton de mise à jour : ouvre la fenêtre de mise à jour si une version est là (ou s'installe), sinon cherche
    /// tout de suite ; la fenêtre s'ouvre sur les notes si la recherche trouve une version. Rien ne s'installe sans
    /// le bouton « Mettre à jour » de cette fenêtre.
    pub(crate) fn update_click(&mut self, cx: &mut Context<Self>) {
        let u = self.update.clone().unwrap_or(Value::Null);
        let phase = u["phase"].as_str().unwrap_or("");
        if u["status"] == "available" || matches!(phase, "downloading" | "verifying" | "extracting" | "applying" | "failed") {
            self.open_update_sheet(crate::update_sheet::SheetKind::Update, cx);
            return;
        }
        if self.update_then_open {
            return;
        }
        self.update_then_open = true;
        self.update_notice = None;
        let client = self.client.clone();
        let job = cx.background_executor().spawn(async move { client.post_module("/api", "update-check", json!({})) });
        cx.spawn(async move |this, cx| {
            let result = job.await;
            let _ = this.update(cx, |this, cx| {
                this.update_then_open = false;
                match result {
                    Ok(r) if r["update"].is_object() => {
                        let u = r["update"].clone();
                        let found = u["status"] == "available";
                        this.update_notice = match u["result"].as_str() {
                            _ if found => None,
                            Some("offline") => Some((false, "Recherche impossible : vérifie ta connexion, puis réessaie.".into())),
                            Some("rate-limited") => Some((false, "GitHub limite les recherches : réessaie plus tard.".into())),
                            _ => Some((true, "Rustudio est à jour.".into())),
                        };
                        this.update = Some(u);
                        if found {
                            this.open_update_sheet(crate::update_sheet::SheetKind::Update, cx);
                        }
                    }
                    Ok(_) => this.update_notice = Some((false, "Réponse du moteur illisible.".into())),
                    Err(error) => this.update_notice = Some((false, error)),
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    pub(crate) fn update_post(&mut self, action: &'static str, cx: &mut Context<Self>) {
        let client = self.client.clone();
        let job = cx.background_executor().spawn(async move { client.post_module("/api", action, json!({})) });
        cx.spawn(async move |this, cx| {
            let result = job.await;
            let _ = this.update(cx, |this, cx| {
                if let Err(error) = result {
                    this.notice = Some(error);
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    fn finish_param(&mut self, cx: &mut Context<Self>) {
        if let Some(d) = self.param_drag.take() {
            self.param_held.insert(d.key.to_string(), (d.value, Instant::now()));
            (d.commit)(self, d.value, cx);
            cx.notify();
        }
    }

    fn commit_drag(&mut self, position: gpui::Point<Pixels>, cx: &mut Context<Self>) {
        if let Some(g) = self.gesture.take() {
            self.set_volume(
                g.channel,
                position_value(g.bounds, position, g.vertical),
                cx,
            );
            cx.notify();
        }
    }

    /// Zone de réglage d'un volume : glisser, cliquer ou flèches du clavier. Le dessin est fourni par l'appelant.
    fn rail(&self, channel: &Channel, vertical: bool, index: usize, cx: &mut Context<Self>) -> Stateful<Div> {
        let rect = Rc::new(Cell::new(None::<Bounds<Pixels>>));
        let layout = rect.clone();
        let handle = self.focus[index].clone();
        let press_focus = handle.clone();
        let id = channel.id.clone();
        let key_channel = channel.clone();
        let enabled = self.enabled();
        let drag_origin = cx.listener(move |this, event: &gpui::MouseDownEvent, window, cx| {
            if this.enabled()
                && let Some(bounds) = rect.get() {
                    press_focus.focus(window);
                    this.gesture = Some(Gesture { channel: id.clone(), bounds, vertical, value: position_value(bounds, event.position, vertical), sent_at: None });
                    cx.notify();
                }
        });
        let el = div()
            .relative()
            .id(("volume", index))
            .track_focus(&handle)
            .tab_stop(enabled)
            .when(enabled, |el| el.cursor(CursorStyle::PointingHand))
            .on_mouse_down(MouseButton::Left, drag_origin)
            .on_key_down(cx.listener(move |this, key: &gpui::KeyDownEvent, _, cx| {
                let old = this.volume(&key_channel);
                let next = match key.keystroke.key.as_str() {
                    "up" | "right" => old.saturating_add(5),
                    "down" | "left" => old.saturating_sub(5),
                    "home" => 0,
                    "end" => 255,
                    _ => return,
                };
                this.set_volume(key_channel.id.clone(), next, cx);
                cx.stop_propagation();
            }))
            .child(canvas(move |bounds, _, _| layout.set(Some(bounds)), |_, _, _, _| {}).absolute().top_0().left_0().size_full());
        ui::hoverable(el, format!("volume-{index}"))
    }

    fn mute_caption(&self, channel: &Channel, target: &str) -> String {
        match &channel.mute {
            Some(m) if m == "all" => "Coupé".into(),
            Some(m) => format!("Coupé {}", self.target(m)),
            None if target == "all" => "Couper".into(),
            None => format!("Couper {}", self.target(target)),
        }
    }

    fn strip(&self, index: usize, fader: &Fader, channel: &Channel, cx: &mut Context<Self>) -> Div {
        let c = channel.clone();
        let colour = channel_colour(channel);
        let target = if channel.input { fader.mode.as_str() } else { "all" }.to_owned();
        let value = self.volume(channel);
        let fraction = f32::from(value) / 255.;
        let muted = channel.mute.is_some();
        let partial = channel.mute.as_deref().is_some_and(|m| m != "all");
        let caption = self.mute_caption(channel, &target);
        let fill = TRAVEL * fraction;
        let physical = fader.position.filter(|p| p.abs_diff(value) > 3);
        let hover = if self.enabled() { ui::hover_t(&format!("volume-{index}")) } else { 0. };
        // Remplissage plat en haut, qui épouse le rail : la forme complète du rail, découpée à la hauteur du volume.
        let shape = div()
            .absolute()
            .bottom_0()
            .left_0()
            .w(px(FADER_W))
            .h(px(TRAVEL))
            .rounded(px(FADER_R))
            .bg(linear_gradient(180., linear_color_stop(ui::to_white(colour, 0.25 + 0.08 * hover), 0.), linear_color_stop(ui::to_black(colour, 0.1 - 0.05 * hover), 1.)));
        let mut track = self
            .rail(channel, true, index, cx)
            .mt(px(14.))
            .w(px(FADER_W))
            .h(px(TRAVEL))
            .rounded(px(FADER_R))
            .bg(linear_gradient(180., linear_color_stop(rgb(0xe3e3e8), 0.), linear_color_stop(rgb(0xeeeef2), 1.)))
            .border_1()
            .border_color(ui::lerp(alpha(ui::INK, 0.05), alpha(colour, 0.4), hover))
            .shadow(vec![gpui::BoxShadow { color: alpha(colour, 0.24 * hover), offset: gpui::point(px(0.), px(10.)), blur_radius: px(26.), spread_radius: px(0.) }])
            .child(
                div()
                    .absolute()
                    .left_0()
                    .bottom_0()
                    .w(px(FADER_W))
                    .h(px(fill))
                    .overflow_hidden()
                    .when(muted, |d| d.opacity(0.4))
                    .child(shape)
                    .when(fill > 30., |d| d.child(div().absolute().top(px(10.)).left(px(FADER_W / 2. - 11. + 3. * hover)).w(px(22. - 6. * hover)).h(px(4.)).rounded_full().bg(alpha(ui::WHITE, 0.9)))),
            );
        if let Some(p) = physical {
            track = track.child(div().absolute().left(px(6.)).right(px(6.)).top(px(TRAVEL * (1. - f32::from(p) / 255.))).h(px(2.)).rounded_full().bg(alpha(ui::TEXT, 0.4)));
        }
        let (mute_bg, mute_fg, glyph) = if partial {
            (rgb(ui::ORANGE), rgb(ui::WHITE), Icon::VolumeOff)
        } else if muted {
            (rgb(ui::RED), rgb(ui::WHITE), Icon::VolumeOff)
        } else {
            (alpha(0x767680, 0.12), rgb(0x3a3a3c), Icon::Volume)
        };
        div()
            .flex()
            .flex_col()
            .flex_1()
            .min_w(px(0.))
            .items_center()
            .child(ui::hover_fill(
                div()
                    .id(("assign", index))
                    .flex()
                    .max_w_full()
                    .min_w(px(0.))
                    .items_center()
                    .gap(px(9.))
                    .px(px(10.))
                    .py(px(6.))
                    .rounded(px(12.))
                    .cursor(CursorStyle::PointingHand)
                    .child(ui::icon(channel_icon(&channel.id), 22., rgb(colour)))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .flex_1()
                            .min_w(px(0.))
                            .overflow_hidden()
                            .child(div().text_size(px(16.5)).font_weight(FontWeight::SEMIBOLD).truncate().child(channel.label.clone()))
                            // une seule ligne, coupée proprement : le texte ne déborde jamais sur la voie voisine
                            .child(ui::caption(format!("Fader {}", index + 1)).truncate()),
                    )
                    .child(ui::icon(Icon::ChevronDown, 14., rgb(ui::TEXT3)).flex_none())
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.picker = if this.picker == Some(index) { None } else { Some(index) };
                        cx.notify();
                    })),
                format!("assign-{index}"),
                alpha(ui::PAPER, if self.picker == Some(index) { 0.7 } else { 0. }),
                alpha(ui::PAPER, 0.78),
            ))
            .child(track)
            .child(
                div()
                    .mt(px(12.))
                    .flex()
                    .items_baseline()
                    .justify_center()
                    .child(div().text_size(px(40.)).line_height(px(44.)).font_weight(FontWeight::LIGHT).child(percent(value).to_string()))
                    .child(div().ml(px(2.)).text_size(px(18.)).text_color(rgb(ui::TEXT3)).child("%")),
            )
            .child(
                ui::round(("mute", index), glyph, 48., mute_bg, mute_fg)
                    .mt(px(10.))
                    .when(muted, |d| d.shadow(vec![gpui::BoxShadow { color: alpha(if partial { ui::ORANGE } else { ui::RED }, 0.3), offset: gpui::point(px(0.), px(6.)), blur_radius: px(18.), spread_radius: px(0.) }]))
                    .on_click(cx.listener(move |this, _, _, cx| this.toggle(&c, &target, cx))),
            )
            .child(
                div()
                    .mt(px(6.))
                    .text_size(px(13.))
                    .font_weight(if muted { FontWeight::SEMIBOLD } else { FontWeight::MEDIUM })
                    .text_color(rgb(if partial { ui::ORANGE_TEXT } else if muted { ui::RED_TEXT } else { ui::TEXT2 }))
                    .child(caption),
            )
    }

    fn target(&self, id: &str) -> &str {
        self.snapshot
            .as_ref()
            .and_then(|s| s.targets.iter().find(|target| target.id == id))
            .map(|target| target.label.as_str())
            .unwrap_or("partout")
    }

    fn selector(&self, snap: &Snapshot, i: usize, cx: &mut Context<Self>) -> Div {
        let f = &snap.faders[i];
        ui::card()
            .child(ui::card_title(Some((Icon::Sliders, ui::ACCENT)), format!("Fader {}", i + 1), Some("Choisis sa voie et ce que coupe son bouton".into())))
            .child(ui::label("Voie"))
            .child(div().flex().flex_wrap().gap(px(8.)).children(snap.channels.iter().map(|ch| {
                let id = ch.id.clone();
                let fader = f.id.clone();
                ui::button(format!("choose-{id}"), ch.label.clone(), f.channel == id).on_click(cx.listener(move |this, _, _, cx| {
                    if this.enqueue("fader", json!({"fader":fader, "channel":id}), cx).is_some() {
                        this.picker = None;
                    }
                }))
            })))
            .child(ui::label("Le bouton coupe"))
            .child(div().flex().flex_wrap().gap(px(8.)).children(snap.targets.iter().map(|target| {
                let id = target.id.clone();
                let fader = f.id.clone();
                ui::button(format!("target-{id}"), target.label.clone(), f.mode == id).on_click(cx.listener(move |this, _, _, cx| {
                    if this.enqueue("settings", json!({"mute_modes": {(fader.clone()): id}}), cx).is_some() {
                        this.picker = None;
                    }
                }))
            })))
    }

    fn channel_row(&self, i: usize, ch: &Channel, first: bool, cx: &mut Context<Self>) -> Div {
        let channel = ch.clone();
        let colour = channel_colour(ch);
        let muted = ch.mute.is_some();
        let value = self.volume(ch);
        let used = self.windows_use(&ch.id);
        div()
            .flex()
            .items_center()
            .gap(px(14.))
            .py(px(10.))
            .when(!first, |d| d.border_t_1().border_color(alpha(ui::INK, 0.07)))
            .child(ui::chip(channel_icon(&ch.id), colour, 32.))
            .child(
                div()
                    .w(px(210.))
                    .flex()
                    .flex_col()
                    .child(div().flex().items_center().gap(px(8.)).child(div().text_size(px(14.)).font_weight(FontWeight::SEMIBOLD).child(ch.label.clone())).when_some(used, |d, u| d.child(ui::badge(u, ui::ACCENT))))
                    .child(div().text_size(px(12.)).text_color(rgb(ui::TEXT2)).child(channel_hint(&ch.id))),
            )
            .child(self.rail(ch, false, 4 + i, cx).flex_1().h(px(22.)).child(ui::hslider(f32::from(value) / 255., if muted { alpha(colour, 0.35) } else { rgb(colour) }, if self.enabled() { ui::hover_t(&format!("volume-{}", 4 + i)) } else { 0. })))
            .child(div().w(px(48.)).flex().justify_end().text_size(px(13.5)).font_weight(FontWeight::MEDIUM).text_color(rgb(ui::TEXT2)).child(format!("{} %", percent(value))))
            .child(
                ui::round(SharedString::from(format!("mute-other-{}", ch.id)), if muted { Icon::VolumeOff } else { Icon::Volume }, 32., if muted { rgb(ui::RED) } else { alpha(0x767680, 0.) }, if muted { rgb(ui::WHITE) } else { rgb(ui::TEXT2) })
                    .on_click(cx.listener(move |this, _, _, cx| this.toggle(&channel, "all", cx))),
            )
    }

    /// Ce que Windows envoie sur cette voie (sortie par défaut, appels), d'après les périphériques actifs.
    fn windows_use(&self, id: &str) -> Option<&'static str> {
        let outputs = self.win.inventory.as_ref()?["outputs"].as_array()?;
        let named = |o: &&serde_json::Value| {
            let n = o["name"].as_str().unwrap_or("").to_lowercase();
            let word = match id {
                "System" => "system",
                "Game" => "game",
                "Chat" => "chat",
                "Music" => "music",
                "Sample" => "sample",
                _ => return false,
            };
            n.contains("goxlr") && n.contains(word)
        };
        let o = outputs.iter().find(named)?;
        match (o["default"] == true, o["communications"] == true) {
            (true, true) => Some("Son de Windows et appels"),
            (true, false) => Some("Son de Windows"),
            (false, true) => Some("Appels Windows"),
            _ => None,
        }
    }

    fn others(&self, snap: &Snapshot, cx: &mut Context<Self>) -> Div {
        let free: Vec<&Channel> = snap.channels.iter().filter(|ch| !snap.faders.iter().any(|f| f.channel == ch.id)).collect();
        let groups: [(&str, &str, Icon, &[&str]); 3] = [
            ("Sons de ton PC", "Chaque appli choisit sa sortie GoXLR dans ses réglages audio (ou dans les paramètres de son de Windows).", Icon::Monitor, &["System", "Game", "Chat", "Music", "Sample"]),
            ("Prises de la GoXLR", "Seulement si un appareil est branché derrière la GoXLR.", Icon::Plug, &["LineIn", "Console"]),
            ("Ce que tu entends", "Le volume final de ton casque, de tes enceintes et de ton propre retour.", Icon::Headphones, &["Headphones", "LineOut", "MicMonitor"]),
        ];
        let mut cards: Vec<AnyElement> = vec![];
        let mut index = 0;
        for (title, hint, glyph, ids) in groups {
            let rows: Vec<&&Channel> = free.iter().filter(|c| ids.contains(&c.id.as_str())).collect();
            if rows.is_empty() {
                continue;
            }
            let mut card = ui::section(glyph, ui::ACCENT, title, None).gap(px(4.)).child(ui::hint(hint).text_size(px(12.)).mb(px(4.)));
            for (n, ch) in rows.into_iter().enumerate() {
                card = card.child(self.channel_row(index, ch, n == 0, cx));
                index += 1;
            }
            cards.push(card.into_any_element());
        }
        div().flex().flex_col().gap(px(16.)).children(cards)
    }

    pub fn mixer_view(&self, cx: &mut Context<Self>) -> Div {
        let Some(snap) = &self.snapshot else {
            return div().child(ui::card().items_center().py(px(40.)).child(ui::chip(Icon::Refresh, ui::ACCENT, 52.)).child(ui::title("Le moteur ne répond plus", 20.)).child(ui::hint("Reconnexion automatique…")));
        };
        let mut page = div().flex().flex_col().gap(px(18.));
        if !self.enabled() {
            page = page.child(ui::notice(false, "GoXLR Mini indisponible : les réglages reviennent dès qu’elle répond."));
        }
        page = page.child(div().flex().gap(px(12.)).children(snap.faders.iter().enumerate().filter_map(|(i, f)| {
            let channel = snap.channels.iter().find(|ch| ch.id == f.channel)?;
            Some(self.strip(i, f, channel, cx))
        })));
        if let Some(i) = self.picker {
            page = page.child(self.selector(snap, i, cx));
        }
        page.child(self.others(snap, cx))
    }
}

impl Mixer {
    /// Message passager après une action : affiché en surimpression, il ne décale jamais la page.
    fn toast(&self) -> Option<(bool, String)> {
        self.go
            .notice
            .clone()
            .or_else(|| self.update_notice.clone())
            .or_else(|| self.profile_notice.clone())
            .or_else(|| self.library.notice.clone())
            .or_else(|| self.win.note.clone().map(|t| (true, t)))
            .or_else(|| self.notice.clone().map(|t| (false, t)))
    }
    fn clear_toast(&mut self) {
        if self.go.notice.take().is_some() {
            return;
        }
        if self.update_notice.take().is_some() {
            return;
        }
        if self.profile_notice.take().is_some() {
            return;
        }
        if self.library.notice.take().is_some() {
            return;
        }
        if self.win.note.take().is_some() {
            return;
        }
        self.notice = None;
    }
}

impl Render for Mixer {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        ui::sync_theme(window);
        self.renders += 1;
        if self.renders == 1 {
            let start = self.started;
            window.on_next_frame(move |_, _| println!("first_frame_ms={}", start.elapsed().as_millis()));
        }
        let toast = self.toast();
        let key = toast.as_ref().map(|(ok, t)| format!("{ok}{t}")).unwrap_or_default();
        if key != self.toast_key {
            self.toast_key = key.clone();
            if let Some((ok, _)) = &toast {
                // les confirmations s'effacent vite, les erreurs restent un peu plus longtemps
                let wait = std::time::Duration::from_millis(if *ok { 3000 } else { 7000 });
                cx.spawn(async move |this, cx| {
                    cx.background_executor().timer(wait).await;
                    let _ = this.update(cx, |this, cx| {
                        if this.toast_key == key {
                            this.clear_toast();
                            cx.notify();
                        }
                    });
                })
                .detach();
            }
        }
        let dialog = self.profile_confirm_box(cx).or_else(|| self.go_confirm_box(cx)).or_else(|| self.confirm_box(cx)).or_else(|| self.win_confirm_box(cx));
        self.sheet_tick(cx);
        let sheet = self.update_sheet_view(cx);
        let shell = self
            .shell(window, cx)
            .children(toast.map(|(ok, text)| ui::toast(ok, text)))
            .children(sheet.map(|s| ui::modal_sized(s, 640.)))
            .children(dialog.map(ui::modal));
        ui::keep_animating(window);
        shell
            .on_mouse_move(cx.listener(|this, ev: &gpui::MouseMoveEvent, _, cx| {
                if let Some(d) = this.param_drag.as_mut() {
                    if ev.pressed_button == Some(MouseButton::Left) {
                        let next = d.at(ev.position);
                        if (next - d.value).abs() > 1e-9 {
                            d.value = next;
                            cx.notify();
                        }
                    } else {
                        this.finish_param(cx);
                    }
                    return;
                }
                if ev.pressed_button == Some(MouseButton::Left) {
                    this.drag_to(ev.position, cx);
                } else if this.gesture.take().is_some() {
                    cx.notify();
                }
            }))
            .on_mouse_up(MouseButton::Left, cx.listener(|this, ev: &gpui::MouseUpEvent, _, cx| {
                this.finish_param(cx);
                this.commit_drag(ev.position, cx)
            }))
            .on_mouse_up_out(MouseButton::Left, cx.listener(|this, ev: &gpui::MouseUpEvent, _, cx| {
                this.finish_param(cx);
                this.commit_drag(ev.position, cx)
            }))
            .on_mouse_down(MouseButton::Navigate(gpui::NavigationDirection::Back), cx.listener(|this, _, _, cx| this.go_back(cx)))
    }
}

fn position_value(bounds: Bounds<Pixels>, at: gpui::Point<Pixels>, vertical: bool) -> u8 {
    let fraction = if vertical {
        1. - f32::from(at.y - bounds.origin.y) / f32::from(bounds.size.height).max(1.)
    } else {
        f32::from(at.x - bounds.origin.x) / f32::from(bounds.size.width).max(1.)
    };
    (fraction.clamp(0., 1.) * 255.).round() as u8
}

fn percent(volume: u8) -> u16 {
    (u16::from(volume) * 100 + 127) / 255
}

/// Couleur de la voie (celle du moteur), assombrie si elle est trop claire pour un fond blanc.
fn channel_colour(channel: &Channel) -> u32 {
    let c = u32::from_str_radix(&channel.colour, 16).unwrap_or(ui::ACCENT);
    let light = ((c >> 16) & 255) + ((c >> 8) & 255) + (c & 255) > 690;
    if light { ui::TEXT3 } else { c }
}

/// Ce que transporte une voie, en clair.
pub(crate) fn channel_hint(id: &str) -> &'static str {
    match id {
        "Mic" => "Ta voix",
        "System" => "Sons de Windows",
        "Game" => "Le son de tes jeux",
        "Chat" => "Discord et appels",
        "Music" => "Spotify, YouTube…",
        "Sample" => "Les sons du sampler",
        "LineIn" => "Appareil branché sur la prise Line In",
        "Console" => "Console branchée sur la prise dédiée",
        "Headphones" => "Volume général de ton casque",
        "LineOut" => "Enceintes sur la prise Line Out",
        "MicMonitor" => "T’entendre dans ton casque",
        _ => "Voie audio",
    }
}

fn channel_icon(id: &str) -> Icon {
    match id {
        "Mic" => Icon::Mic,
        "Music" => Icon::Music,
        "Chat" => Icon::Chat,
        "System" => Icon::Monitor,
        "Game" | "Console" => Icon::Game,
        "LineIn" => Icon::Plug,
        "Sample" => Icon::Lines,
        "Headphones" => Icon::Headphones,
        "MicMonitor" => Icon::Ear,
        _ => Icon::Volume,
    }
}


/// Curseur de réglage en cours de glissement (gain, égaliseur, seuils…). La valeur part au relâchement.
pub(crate) struct ParamDrag {
    key: SharedString,
    bounds: Bounds<Pixels>,
    vertical: bool,
    min: f64,
    max: f64,
    step: f64,
    value: f64,
    commit: Rc<dyn Fn(&mut Mixer, f64, &mut Context<Mixer>)>,
}

impl ParamDrag {
    fn at(&self, position: gpui::Point<Pixels>) -> f64 {
        let fraction = position_fraction(self.bounds, position, self.vertical) as f64;
        let raw = self.min + fraction * (self.max - self.min);
        ((raw / self.step).round() * self.step).clamp(self.min, self.max)
    }
}

impl Mixer {
    /// Valeur affichée d'un réglage : celle du geste en cours, puis celle envoyée tant que le moteur ne l'a pas relue.
    pub(crate) fn param_value(&self, key: &str, actual: f64) -> f64 {
        if let Some(d) = self.param_drag.as_ref().filter(|d| d.key.as_ref() == key) {
            return d.value;
        }
        match self.param_held.get(key) {
            Some((v, at)) if at.elapsed() < Duration::from_millis(1800) => *v,
            _ => actual,
        }
    }

    /// Zone cliquable d'un curseur ; le dessin (horizontal ou vertical) est ajouté par l'appelant.
    pub(crate) fn param_area(
        &self,
        key: &str,
        range: (f64, f64),
        step: f64,
        vertical: bool,
        enabled: bool,
        commit: impl Fn(&mut Mixer, f64, &mut Context<Mixer>) + 'static,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let rect = Rc::new(Cell::new(None::<Bounds<Pixels>>));
        let layout = rect.clone();
        let key: SharedString = format!("param-{key}").into();
        let commit: Rc<dyn Fn(&mut Mixer, f64, &mut Context<Mixer>)> = Rc::new(commit);
        let id = key.clone();
        let el = div()
            .id(ElementId::Name(key.clone()))
            .relative()
            .when(enabled, |d| d.cursor(CursorStyle::PointingHand))
            .when(!enabled, |d| d.opacity(0.5))
            .on_mouse_down(MouseButton::Left, cx.listener(move |this, event: &gpui::MouseDownEvent, _, cx| {
                let Some(bounds) = rect.get() else { return };
                if !enabled {
                    return;
                }
                let mut drag = ParamDrag { key: id.clone(), bounds, vertical, min: range.0, max: range.1, step, value: range.0, commit: commit.clone() };
                drag.value = drag.at(event.position);
                this.param_drag = Some(drag);
                cx.stop_propagation();
                cx.notify();
            }))
            .child(canvas(move |bounds, _, _| layout.set(Some(bounds)), |_, _, _, _| {}).absolute().top_0().left_0().size_full());
        ui::hoverable(el, key.to_string())
    }

    /// Ligne « libellé … valeur » avec un curseur horizontal dessous.
    pub(crate) fn slider_row(
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
            .flex()
            .flex_col()
            .gap(px(4.))
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(div().text_size(px(13.)).text_color(rgb(ui::TEXT)).child(label.into()))
                    .child(div().text_size(px(13.)).font_weight(FontWeight::MEDIUM).text_color(rgb(ui::TEXT2)).child(format(value))),
            )
            .child(self.param_area(key, range, step, false, enabled, commit, cx).h(px(22.)).w_full().child(ui::hslider(fraction, rgb(ui::ACCENT), hover)))
    }

    /// Colonne d'égaliseur : valeur en dB au-dessus, curseur vertical, libellé dessous.
    pub(crate) fn eq_column(
        &self,
        key: &str,
        actual: f64,
        range: (f64, f64),
        step: f64,
        below: AnyElement,
        enabled: bool,
        commit: impl Fn(&mut Mixer, f64, &mut Context<Mixer>) + 'static,
        cx: &mut Context<Self>,
    ) -> Div {
        let value = self.param_value(&format!("param-{key}"), actual);
        let fraction = ((value - range.0) / (range.1 - range.0)).clamp(0., 1.) as f32;
        let zero = ((0. - range.0) / (range.1 - range.0)).clamp(0., 1.) as f32;
        let hover = ui::hover_t(&format!("param-{key}"));
        let shown = if value > 0. { format!("+{}", fmt_num(value)) } else { fmt_num(value) };
        div()
            .flex()
            .flex_col()
            .items_center()
            .flex_1()
            .min_w(px(0.))
            .gap(px(8.))
            .child(div().text_size(px(12.)).font_weight(FontWeight::SEMIBOLD).text_color(rgb(if value.abs() < 0.01 { ui::TEXT3 } else { ui::ACCENT })).child(shown))
            .child(self.param_area(key, range, step, true, enabled, commit, cx).w(px(28.)).h(px(150.)).child(ui::vslider(fraction, zero, rgb(ui::ACCENT), hover, 150.)))
            .child(below)
    }
}

/// 1.5 → « 1,5 », 2.0 → « 2 ».
pub(crate) fn fmt_num(v: f64) -> String {
    let r = (v * 10.).round() / 10.;
    if (r - r.round()).abs() < 1e-6 { format!("{}", r as i64) } else { format!("{r:.1}").replace('.', ",") }
}

fn position_fraction(bounds: Bounds<Pixels>, at: gpui::Point<Pixels>, vertical: bool) -> f32 {
    let fraction = if vertical {
        1. - f32::from(at.y - bounds.origin.y) / f32::from(bounds.size.height).max(1.)
    } else {
        f32::from(at.x - bounds.origin.x) / f32::from(bounds.size.width).max(1.)
    };
    fraction.clamp(0., 1.)
}
