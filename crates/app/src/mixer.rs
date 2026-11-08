use crate::backend::{self, Channel, Client, Command, Event, Fader, Snapshot};
use crate::desktop::{Confirm, Page};
use crate::profiles::{Profile, Store};
use crate::ui::{self, Icon, alpha, rgb};
use gpui::{
    Bounds, Context, CursorStyle, Div, FocusHandle, FontWeight, IntoElement, MouseButton, Pixels, Render, SharedString,
    Stateful, Window, canvas, div, linear_color_stop, linear_gradient, prelude::*, px,
};
use serde_json::{Value, json};
use std::{
    cell::Cell,
    collections::HashMap,
    rc::Rc,
    sync::mpsc,
    time::{Duration, Instant},
};

const TRAVEL: f32 = 200.;
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
            seq: 0,
            focus: (0..15).map(|_| cx.focus_handle()).collect(),
            received: 0,
            renders: 0,
            started,
        }
    }

    /// Change de page et règle les lectures en direct de la page visible.
    pub fn go_to(&mut self, page: Page, cx: &mut Context<Self>) {
        self.page = page;
        self.client.set_mixer_visible(page == Page::Mixer);
        self.client.set_mouse_visible(page == Page::Mouse);
        self.client.set_go_visible(matches!(page, Page::Go(_)));
        self.client.set_audio_visible(page == Page::WindowsAudio);
        self.win.cancel();
        if page == Page::Go(crate::goxlr::Section::Microphone) {
            self.load_voice_info(cx);
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
            Event::Audio(data) => {
                self.win.inventory = Some(data);
                force_render = self.page == Page::WindowsAudio;
            }
            Event::MicOsd(state) => {
                crate::win_session::show_mic(&state);
                force_render = false;
            }
            Event::Go(view) => {
                self.go.receive(view);
                force_render = matches!(self.page, Page::Go(_));
            }
            Event::Mouse(view) => {
                self.mouse.receive(view);
                force_render = self.page == Page::Mouse;
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
            if this.enabled() {
                if let Some(bounds) = rect.get() {
                    press_focus.focus(window);
                    this.gesture = Some(Gesture { channel: id.clone(), bounds, vertical, value: position_value(bounds, event.position, vertical), sent_at: None });
                    cx.notify();
                }
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
        let grip = 24. - 6. * hover;
        let mut track = self
            .rail(channel, true, index, cx)
            .mt(px(12.))
            .w(px(72.))
            .h(px(TRAVEL))
            .rounded_full()
            .overflow_hidden()
            .bg(linear_gradient(180., linear_color_stop(rgb(0xe6e6eb), 0.), linear_color_stop(rgb(0xefeff3), 1.)))
            .border_1()
            .border_color(ui::lerp(alpha(0x000000, 0.04), alpha(colour, 0.35), hover))
            .shadow(vec![gpui::BoxShadow { color: alpha(colour, 0.22 * hover), offset: gpui::point(px(0.), px(8.)), blur_radius: px(22.), spread_radius: px(0.) }])
            .child(
                div()
                    .absolute()
                    .left_0()
                    .right_0()
                    .bottom_0()
                    .h(px(fill))
                    .rounded_t(px(24.))
                    .rounded_b(px(36.))
                    .bg(linear_gradient(180., linear_color_stop(ui::tint(colour, 0.22 + 0.1 * hover), 0.), linear_color_stop(ui::shade(colour, 0.12 - 0.06 * hover), 1.)))
                    .when(muted, |d| d.opacity(0.42))
                    .child(div().absolute().top(px(11.)).left(px(grip)).right(px(grip)).h(px(4.)).rounded_full().bg(alpha(ui::WHITE, 0.92))),
            );
        if let Some(p) = physical {
            track = track.child(div().absolute().left(px(4.)).right(px(4.)).top(px(TRAVEL * (1. - f32::from(p) / 255.))).h(px(2.)).rounded_full().bg(alpha(ui::TEXT, 0.45)));
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
                            .min_w(px(0.))
                            .child(div().text_size(px(16.5)).font_weight(FontWeight::SEMIBOLD).child(channel.label.clone()))
                            .child(ui::caption(format!("Fader {} · {}", index + 1, if target == "all" { "partout".to_owned() } else { self.target(&target).to_owned() }))),
                    )
                    .child(ui::icon(Icon::ChevronDown, 14., rgb(ui::TEXT3)))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.picker = if this.picker == Some(index) { None } else { Some(index) };
                        cx.notify();
                    })),
                format!("assign-{index}"),
                alpha(ui::WHITE, if self.picker == Some(index) { 0.7 } else { 0. }),
                alpha(ui::WHITE, 0.78),
            ))
            .child(track)
            .child(
                div()
                    .mt(px(10.))
                    .flex()
                    .items_baseline()
                    .child(div().text_size(px(42.)).line_height(px(46.)).font_weight(FontWeight::LIGHT).child(percent(value).to_string()))
                    .child(div().ml(px(3.)).text_size(px(19.)).text_color(rgb(ui::TEXT3)).child("%")),
            )
            .child(
                ui::round(("mute", index), glyph, 50., mute_bg, mute_fg)
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

    fn channel_row(&self, i: usize, ch: &Channel, cx: &mut Context<Self>) -> Div {
        let channel = ch.clone();
        let colour = channel_colour(ch);
        let muted = ch.mute.is_some();
        let value = self.volume(ch);
        div()
            .flex()
            .items_center()
            .gap(px(12.))
            .h(px(50.))
            .child(ui::icon(channel_icon(&ch.id), 20., rgb(colour)))
            .child(div().w(px(112.)).text_size(px(14.)).font_weight(FontWeight::MEDIUM).child(ch.label.clone()))
            .child(self.rail(ch, false, 4 + i, cx).flex_1().h(px(22.)).child(ui::hslider(f32::from(value) / 255., if muted { alpha(colour, 0.35) } else { rgb(colour) }, if self.enabled() { ui::hover_t(&format!("volume-{}", 4 + i)) } else { 0. })))
            .child(div().w(px(46.)).flex().justify_end().text_size(px(13.5)).text_color(rgb(ui::TEXT2)).child(format!("{} %", percent(value))))
            .child(
                ui::round(SharedString::from(format!("mute-other-{}", ch.id)), if muted { Icon::VolumeOff } else { Icon::Volume }, 30., if muted { rgb(ui::RED) } else { alpha(0x767680, 0.) }, if muted { rgb(ui::WHITE) } else { rgb(ui::TEXT2) })
                    .on_click(cx.listener(move |this, _, _, cx| this.toggle(&channel, "all", cx))),
            )
    }

    fn others(&self, snap: &Snapshot, cx: &mut Context<Self>) -> Div {
        let rows: Vec<&Channel> = snap.channels.iter().filter(|ch| !snap.faders.iter().any(|f| f.channel == ch.id)).collect();
        let column = || div().flex().flex_col().flex_1().min_w(px(0.));
        let (mut left, mut right) = (column(), column());
        for (i, ch) in rows.iter().enumerate() {
            let row = self.channel_row(i, ch, cx).when(i + 2 < rows.len(), |d| d.border_b_1().border_color(alpha(0x000000, 0.07)));
            if i % 2 == 0 {
                left = left.child(row);
            } else {
                right = right.child(row);
            }
        }
        ui::card().flex_row().gap(px(40.)).px(px(26.)).py(px(6.)).child(left).child(right)
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

impl Render for Mixer {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.renders += 1;
        if self.renders == 1 {
            let start = self.started;
            window.on_next_frame(move |_, _| println!("first_frame_ms={}", start.elapsed().as_millis()));
        }
        let shell = self.shell(window, cx);
        ui::keep_animating(window);
        shell
            .on_mouse_move(cx.listener(|this, ev: &gpui::MouseMoveEvent, _, cx| {
                if ev.pressed_button == Some(MouseButton::Left) {
                    this.drag_to(ev.position, cx);
                } else if this.gesture.take().is_some() {
                    cx.notify();
                }
            }))
            .on_mouse_up(MouseButton::Left, cx.listener(|this, ev: &gpui::MouseUpEvent, _, cx| this.commit_drag(ev.position, cx)))
            .on_mouse_up_out(MouseButton::Left, cx.listener(|this, ev: &gpui::MouseUpEvent, _, cx| this.commit_drag(ev.position, cx)))
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

pub(crate) use crate::ui::button;
