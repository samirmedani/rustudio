//! Éditeur RGB du MK770. Les opérations manipulent un brouillon, jamais des rapports USB.
use crate::{
    desktop::Page,
    keyboard::{PALETTE, swatch},
    mixer::Mixer,
    ui::{self, Icon, Tone, alpha, rgb},
};
use gpui::{
    Bounds, Context, Div, FontWeight, MouseButton, PathPromptOptions, Pixels, Point, SharedString,
    canvas, div, prelude::*, px,
};
use rustudio_core::mk770::{self as data, LAYOUT};
use serde::{Deserialize, Serialize};
use std::{
    cell::Cell,
    collections::VecDeque,
    io::Read,
    path::{Path, PathBuf},
    rc::Rc,
    sync::LazyLock,
    time::{Duration, Instant},
};

const KEY: f32 = 36.;
const GAP: f32 = 4.;
const INSET: f32 = 16.;
const HISTORY: usize = 40;
const FADE: Duration = Duration::from_millis(160);
const OFF: u32 = 0;
const DEFAULT_BRUSH: u32 = 0x0071e3;
const PROFILE_FORMAT: &str = "rustudio-mk770-rgb-1";
const PROFILE_LIMIT: u64 = 8 * 1024;

pub(crate) type Colours = [u32; data::LEDS];

pub(crate) fn read_colours(values: &[String]) -> Colours {
    std::array::from_fn(|i| {
        values
            .get(i)
            .and_then(|v| data::parse_colour(v))
            .map_or(0, |bytes| {
                u32::from_be_bytes([0, bytes[0], bytes[1], bytes[2]])
            })
    })
}

pub(crate) fn write_colours(colours: &Colours) -> Vec<String> {
    colours.iter().map(|c| format!("#{c:06X}")).collect()
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Profile {
    format: String,
    keys: Vec<String>,
}

impl Profile {
    fn read(path: &Path) -> Result<Colours, String> {
        let mut bytes = Vec::new();
        std::fs::File::open(path)
            .and_then(|file| file.take(PROFILE_LIMIT + 1).read_to_end(&mut bytes))
            .map_err(|e| format!("Lecture du motif impossible : {e}"))?;
        if bytes.len() as u64 > PROFILE_LIMIT {
            return Err("Fichier de motif trop volumineux.".into());
        }
        let profile: Self =
            serde_json::from_slice(&bytes).map_err(|_| "Fichier de motif illisible.")?;
        if profile.format != PROFILE_FORMAT
            || profile.keys.len() != data::LEDS
            || profile.keys.iter().any(|c| data::parse_colour(c).is_none())
        {
            return Err("Ce fichier ne contient pas un motif RGB compatible avec le MK770.".into());
        }
        Ok(read_colours(&profile.keys))
    }

    fn export(path: &Path, colours: &Colours) -> Result<(), String> {
        crate::profiles::export_json(
            path,
            &Self {
                format: PROFILE_FORMAT.into(),
                keys: write_colours(colours),
            },
        )
    }
}

#[derive(Clone, Copy, Default, PartialEq, Eq)]
enum Tool {
    #[default]
    Brush,
    Erase,
    Sample,
}

#[derive(Clone, Copy, Default, PartialEq, Eq)]
enum Zone {
    #[default]
    All,
    Letters,
    Functions,
    Arrows,
    Numpad,
}

impl Zone {
    const ALL: [(Self, &'static str); 5] = [
        (Self::All, "Tout le clavier"),
        (Self::Letters, "Lettres"),
        (Self::Functions, "Touches F"),
        (Self::Arrows, "Flèches"),
        (Self::Numpad, "Pavé numérique"),
    ];
    fn contains(self, key: &KeyBox) -> bool {
        let led = key.leds[0];
        match self {
            Self::All => true,
            Self::Letters => key.label.len() == 1 && key.label.as_bytes()[0].is_ascii_uppercase(),
            Self::Functions => (1..=12).contains(&led),
            Self::Arrows => matches!(key.label, "←" | "↓" | "→" | "↑"),
            Self::Numpad => matches!(led, 32..=35 | 50..=53 | 67..=69 | 84..=87 | 100..=101),
        }
    }
}

#[derive(Clone, Copy)]
enum Pattern {
    Fill,
    Clear,
    Gradient,
    Fps,
}

enum FileChoice {
    Exported,
    Imported(Colours),
}

#[derive(Clone, Copy)]
struct KeyBox {
    label: &'static str,
    leds: &'static [u8],
    left: f32,
    top: f32,
    width: f32,
}

struct Board {
    keys: Vec<KeyBox>,
    width: f32,
}

static BOARD: LazyLock<Board> = LazyLock::new(|| {
    let mut keys = Vec::with_capacity(data::LEDS);
    let mut width = 0_f32;
    for (row, layout) in LAYOUT.iter().enumerate() {
        let mut x = 0.;
        for k in *layout {
            if k.leds.is_empty() {
                x += k.width * KEY;
            } else {
                let w = k.width * KEY + (k.width - 1.).max(0.) * GAP;
                keys.push(KeyBox {
                    label: k.label,
                    leds: k.leds,
                    left: x,
                    top: row as f32 * (KEY + GAP),
                    width: w,
                });
                width = width.max(x + w);
                x += w + GAP;
            }
        }
    }
    Board { keys, width }
});

pub(crate) fn draw_preview_keys(
    bounds: Bounds<Pixels>,
    window: &mut gpui::Window,
    mut colour: impl FnMut(f32, f32, usize) -> u32,
) {
    let width = f32::from(bounds.size.width);
    let height = f32::from(bounds.size.height);
    let board_height = LAYOUT.len() as f32 * (KEY + GAP) - GAP;
    let scale = (width / BOARD.width).min(height / board_height);
    let margin = gpui::point(
        px((width - BOARD.width * scale) / 2.),
        px((height - board_height * scale) / 2.),
    );
    for key in &BOARD.keys {
        let x = (key.left + key.width / 2.) / BOARD.width;
        let y = (key.top + KEY / 2.) / board_height;
        let c = colour(x, y, key.leds[0] as usize);
        let origin =
            bounds.origin + margin + gpui::point(px(key.left * scale), px(key.top * scale));
        let shape = Bounds::new(origin, gpui::size(px(key.width * scale), px(KEY * scale)));
        window.paint_quad(
            gpui::fill(shape, ui::raw(c)).corner_radii(gpui::Corners::all(px(6. * scale))),
        );
    }
}

impl KeyBox {
    // Intersection avec un segment. Un mouvement rapide traverse les touches sans sauter celles entre deux événements.
    fn crosses(self, from: [f32; 2], to: [f32; 2]) -> bool {
        let (mut enter, mut leave) = (0_f32, 1_f32);
        for (i, (min, extent)) in [(self.left, self.width), (self.top, KEY)]
            .into_iter()
            .enumerate()
        {
            let delta = to[i] - from[i];
            if delta == 0. {
                if from[i] < min || from[i] >= min + extent {
                    return false;
                }
            } else {
                let a = (min - from[i]) / delta;
                let b = (min + extent - from[i]) / delta;
                enter = enter.max(a.min(b));
                leave = leave.min(a.max(b));
                if enter > leave {
                    return false;
                }
            }
        }
        true
    }
}

#[derive(Clone, Copy)]
struct PaintFrame {
    bounds: Bounds<Pixels>,
    scale: f32,
}

impl PaintFrame {
    fn at(self, p: Point<Pixels>) -> [f32; 2] {
        [
            (f32::from(p.x - self.bounds.origin.x) - INSET) / self.scale,
            (f32::from(p.y - self.bounds.origin.y) - INSET) / self.scale,
        ]
    }
}

struct Stroke {
    before: Colours,
    frame: PaintFrame,
    last: [f32; 2],
    colour: u32,
}

#[derive(Clone, Copy)]
struct Fade {
    from: u32,
    since: Instant,
}

fn blend(from: u32, to: u32, t: f32) -> u32 {
    let ch = |shift: u32| {
        let a = ((from >> shift) & 255_u32) as f32;
        let b = ((to >> shift) & 255_u32) as f32;
        ((a + (b - a) * t).round() as u32) << shift
    };
    ch(16) | ch(8) | ch(0)
}

fn surface(colour: u32) -> u32 {
    if colour != OFF {
        colour
    } else if ui::is_dark() {
        0x24262c
    } else {
        0xe3e6ed
    }
}

pub(crate) fn label_colour(colour: u32) -> u32 {
    static LINEAR: LazyLock<[f32; 256]> = LazyLock::new(|| {
        std::array::from_fn(|i| {
            let c = i as f32 / 255.;
            if c <= 0.04045 {
                c / 12.92
            } else {
                ((c + 0.055) / 1.055).powf(2.4)
            }
        })
    });
    let [_, r, g, b] = colour.to_be_bytes();
    let luminance =
        0.2126 * LINEAR[r as usize] + 0.7152 * LINEAR[g as usize] + 0.0722 * LINEAR[b as usize];
    if luminance >= 0.179 {
        0x000000
    } else {
        0xffffff
    }
}

pub(crate) struct PaintEditor {
    saved: Colours,
    draft: Option<Colours>,
    undo: VecDeque<Colours>,
    redo: Vec<Colours>,
    fades: [Option<Fade>; data::LEDS],
    tool: Tool,
    zone: Zone,
    brush: u32,
    second: u32,
    before: bool,
    stroke: Option<Stroke>,
    file_busy: bool,
    file_note: Option<(bool, String)>,
}

impl Default for PaintEditor {
    fn default() -> Self {
        Self {
            saved: [OFF; data::LEDS],
            draft: None,
            undo: VecDeque::new(),
            redo: Vec::new(),
            fades: [None; data::LEDS],
            tool: Tool::Brush,
            zone: Zone::All,
            brush: DEFAULT_BRUSH,
            second: 0xb24dff,
            before: false,
            stroke: None,
            file_busy: false,
            file_note: None,
        }
    }
}

impl PaintEditor {
    pub(crate) fn current(&self) -> &Colours {
        self.draft.as_ref().unwrap_or(&self.saved)
    }
    pub(crate) fn saved(&self) -> &Colours {
        &self.saved
    }
    fn displayed(&self) -> &Colours {
        if self.before {
            &self.saved
        } else {
            self.current()
        }
    }
    pub(crate) fn dirty(&self) -> bool {
        self.draft.is_some()
    }
    pub(crate) fn set_brush(&mut self, colour: u32) {
        self.brush = colour;
        self.tool = Tool::Brush;
    }
    pub(crate) fn set_second(&mut self, colour: u32) {
        self.second = colour;
    }

    pub(crate) fn set_saved(&mut self, saved: Colours, applied: bool) {
        if self.saved == saved && !applied {
            return;
        }
        let previous = *self.displayed();
        self.saved = saved;
        if applied || self.draft == Some(saved) {
            self.draft = None;
            self.before = false;
        }
        self.animate(previous, Instant::now());
    }

    fn shown_at(&self, i: usize, colour: u32, now: Instant) -> u32 {
        let target = surface(colour);
        let Some(fade) = self.fades[i] else {
            return target;
        };
        let t = (now.duration_since(fade.since).as_secs_f32() / FADE.as_secs_f32()).min(1.);
        blend(fade.from, target, 1. - (1. - t).powi(3))
    }

    fn animate(&mut self, previous: Colours, now: Instant) {
        let next = *self.displayed();
        for (i, old) in previous.into_iter().enumerate() {
            if old != next[i] {
                self.fades[i] = Some(Fade {
                    from: self.shown_at(i, old, now),
                    since: now,
                });
            }
        }
    }

    pub(crate) fn animating(&self) -> bool {
        self.fades
            .iter()
            .flatten()
            .any(|fade| fade.since.elapsed() < FADE)
    }

    fn compare(&mut self, before: bool) {
        let previous = *self.displayed();
        self.before = before;
        self.animate(previous, Instant::now());
    }

    fn set_current(&mut self, next: Colours) {
        let previous = *self.displayed();
        self.draft = (next != self.saved).then_some(next);
        self.before = false;
        self.animate(previous, Instant::now());
    }

    fn record(&mut self, before: Colours) {
        if &before != self.current() {
            self.push_undo(before);
            self.redo.clear();
        }
    }

    fn push_undo(&mut self, colours: Colours) {
        if self.undo.len() == HISTORY {
            self.undo.pop_front();
        }
        self.undo.push_back(colours);
    }

    fn replace(&mut self, next: Colours) {
        self.finish();
        let before = *self.current();
        self.set_current(next);
        self.record(before);
    }

    fn undo(&mut self) {
        self.finish();
        if let Some(previous) = self.undo.pop_back() {
            self.redo.push(*self.current());
            self.set_current(previous);
        }
    }

    fn redo(&mut self) {
        self.finish();
        if let Some(next) = self.redo.pop() {
            self.push_undo(*self.current());
            self.set_current(next);
        }
    }

    pub(crate) fn finish(&mut self) -> bool {
        if let Some(stroke) = self.stroke.take() {
            let changed = &stroke.before != self.current();
            self.record(stroke.before);
            return changed;
        }
        false
    }

    fn begin(&mut self, frame: PaintFrame, position: Point<Pixels>) -> bool {
        if self.before {
            return false;
        }
        self.finish();
        let p = frame.at(position);
        if self.tool == Tool::Sample {
            if let Some(key) = BOARD.keys.iter().find(|k| k.crosses(p, p)) {
                self.set_brush(self.current()[key.leds[0] as usize]);
                return true;
            }
            return false;
        }
        let colour = if self.tool == Tool::Erase {
            OFF
        } else {
            self.brush
        };
        self.stroke = Some(Stroke {
            before: *self.current(),
            frame,
            last: p,
            colour,
        });
        self.segment(p, p, colour)
    }

    fn move_to(&mut self, position: Point<Pixels>) -> bool {
        let Some(stroke) = &mut self.stroke else {
            return false;
        };
        let to = stroke.frame.at(position);
        let from = std::mem::replace(&mut stroke.last, to);
        let colour = stroke.colour;
        self.segment(from, to, colour)
    }

    fn segment(&mut self, from: [f32; 2], to: [f32; 2], colour: u32) -> bool {
        let mut next = *self.current();
        for key in BOARD.keys.iter().filter(|key| key.crosses(from, to)) {
            for &led in key.leds {
                next[led as usize] = colour;
            }
        }
        if &next == self.current() {
            return false;
        }
        self.set_current(next);
        true
    }

    fn pattern(&mut self, pattern: Pattern) {
        let mut next = *self.current();
        let keys = BOARD
            .keys
            .iter()
            .filter(|k| matches!(pattern, Pattern::Fps) || self.zone.contains(k));
        let (left, right) = keys
            .clone()
            .fold((f32::INFINITY, 0_f32), |(left, right), k| {
                let center = k.left + k.width / 2.;
                (left.min(center), right.max(center))
            });
        for key in keys {
            let colour = match pattern {
                Pattern::Fill => self.brush,
                Pattern::Clear => OFF,
                Pattern::Gradient => blend(
                    self.brush,
                    self.second,
                    ((key.left + key.width / 2. - left) / (right - left).max(1.)).clamp(0., 1.),
                ),
                Pattern::Fps
                    if matches!(
                        key.label,
                        "Z" | "Q" | "S" | "D" | "Espace" | "↑" | "←" | "↓" | "→"
                    ) =>
                {
                    self.brush
                }
                Pattern::Fps if matches!(key.label, "R" | "E" | "Ctrl" | "Maj" | "Échap") => {
                    self.second
                }
                Pattern::Fps => OFF,
            };
            for &led in key.leds {
                next[led as usize] = colour;
            }
        }
        self.replace(next);
    }

    fn changed_keys(&self) -> usize {
        BOARD
            .keys
            .iter()
            .filter(|key| {
                key.leds
                    .iter()
                    .any(|&i| self.saved[i as usize] != self.current()[i as usize])
            })
            .count()
    }
}

impl Mixer {
    fn paint_file(&mut self, export: bool, cx: &mut Context<Self>) {
        if self.keyboard.busy || self.keyboard.paint.file_busy {
            return;
        }
        let paint = &mut self.keyboard.paint;
        paint.file_busy = true;
        paint.file_note = None;
        let colours = *paint.current();
        let export_picker = export.then(|| {
            let initial = std::env::var_os("USERPROFILE")
                .map(PathBuf::from)
                .map(|p| p.join("Documents"))
                .filter(|p| p.is_dir())
                .unwrap_or_else(std::env::temp_dir);
            cx.prompt_for_new_path(&initial, Some("motif-mk770.json"))
        });
        let import_picker = (!export).then(|| {
            cx.prompt_for_paths(PathPromptOptions {
                files: true,
                directories: false,
                multiple: false,
                prompt: None,
            })
        });
        cx.spawn(async move |this, cx| {
            let path = if let Some(picker) = export_picker {
                match picker.await {
                    Ok(Ok(path)) => Ok(path),
                    _ => Err("Sélecteur de fichiers indisponible.".to_owned()),
                }
            } else if let Some(picker) = import_picker {
                match picker.await {
                    Ok(Ok(paths)) => Ok(paths.and_then(|p| p.into_iter().next())),
                    _ => Err("Sélecteur de fichiers indisponible.".to_owned()),
                }
            } else {
                Ok(None)
            };
            let result = match path {
                Ok(Some(path)) => cx
                    .background_executor()
                    .spawn(async move {
                        if export {
                            let path = if path.extension().is_none() {
                                path.with_extension("json")
                            } else {
                                path
                            };
                            Profile::export(&path, &colours).map(|()| FileChoice::Exported)
                        } else {
                            Profile::read(&path).map(FileChoice::Imported)
                        }
                    })
                    .await
                    .map(Some),
                Ok(None) => Ok(None),
                Err(error) => Err(error),
            };
            let _ = this.update(cx, |this, cx| {
                let paint = &mut this.keyboard.paint;
                paint.file_busy = false;
                match result {
                    Ok(Some(FileChoice::Imported(imported))) => {
                        paint.replace(imported);
                        paint.file_note = Some((true, "Motif chargé dans le brouillon.".into()));
                    }
                    Ok(Some(FileChoice::Exported)) => {
                        paint.file_note = Some((true, "Motif enregistré.".into()))
                    }
                    Ok(None) => {}
                    Err(error) => paint.file_note = Some((false, error)),
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    pub(crate) fn move_keyboard_paint(
        &mut self,
        event: &gpui::MouseMoveEvent,
        cx: &mut Context<Self>,
    ) {
        let paint = &mut self.keyboard.paint;
        if self.page != Page::KeyboardPaint
            || event.pressed_button != Some(MouseButton::Left)
            || self.keyboard.busy
            || paint.file_busy
        {
            if paint.finish() {
                cx.notify();
            }
        } else if paint.move_to(event.position) {
            cx.notify();
        }
    }

    pub(crate) fn finish_keyboard_paint(
        &mut self,
        position: Point<Pixels>,
        cx: &mut Context<Self>,
    ) {
        let paint = &mut self.keyboard.paint;
        if self.page == Page::KeyboardPaint && !self.keyboard.busy && !paint.file_busy {
            paint.move_to(position);
        }
        if paint.finish() {
            cx.notify();
        }
    }

    pub(crate) fn keyboard_painter(&self, width: f32, cx: &mut Context<Self>) -> Div {
        let p = &self.keyboard.paint;
        let enabled = !self.keyboard.busy && !p.file_busy;
        let ready = self.keyboard.live().is_some();
        let scale = ((width - INSET * 2. - 2.) / BOARD.width).clamp(0.35, 1.5);
        let board_width = BOARD.width * scale + INSET * 2.;
        let board_height = (LAYOUT.len() as f32 * (KEY + GAP) - GAP) * scale + INSET * 2.;
        let frame = Rc::new(Cell::new(None));
        let layout = frame.clone();
        let mut board = div()
            .id("mk770-paint-board")
            .relative()
            .flex_none()
            .w(px(board_width))
            .h(px(board_height))
            .rounded(px(17.))
            .bg(alpha(ui::INK, 0.045))
            .shadow(ui::soft_shadow())
            .child(
                canvas(
                    move |bounds, _, _| layout.set(Some(PaintFrame { bounds, scale })),
                    |_, _, _, _| {},
                )
                .absolute()
                .top_0()
                .left_0()
                .size_full(),
            )
            .when(enabled && !p.before, |d| {
                d.on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |this, event: &gpui::MouseDownEvent, _, cx| {
                        if !this.keyboard.busy
                            && !this.keyboard.paint.file_busy
                            && let Some(frame) = frame.get()
                            && this.keyboard.paint.begin(frame, event.position)
                        {
                            cx.notify();
                        }
                    }),
                )
            });
        let now = Instant::now();
        for (i, key) in BOARD.keys.iter().enumerate() {
            let led = key.leds[0] as usize;
            let target = p.displayed()[led];
            let colour = p.shown_at(led, target, now);
            let id = format!("mk770-paint-key-{i}");
            let hover = if enabled && !p.before {
                ui::hover_t(&id)
            } else {
                0.
            };
            let ink = ui::raw(label_colour(colour));
            let edge = if target == OFF {
                alpha(ui::INK, 0.13)
            } else {
                ink.opacity(0.24)
            };
            let preview = match p.tool {
                Tool::Brush => ui::raw(p.brush),
                Tool::Erase => rgb(ui::TEXT3),
                Tool::Sample => rgb(ui::ACCENT),
            };
            let cell = div()
                .id(SharedString::from(id.clone()))
                .absolute()
                .left(px(INSET + key.left * scale))
                .top(px(INSET + key.top * scale))
                .w(px(key.width * scale))
                .h(px(KEY * scale))
                .rounded(px(6. * scale))
                .flex()
                .items_center()
                .justify_center()
                .overflow_hidden()
                .border_1()
                .border_color(ui::lerp(edge, preview, 0.78 * hover))
                .bg(ui::raw(colour))
                .child(
                    div()
                        .absolute()
                        .bottom_0()
                        .w_full()
                        .h(px(3. * scale))
                        .bg(ui::raw(0).opacity(0.16)),
                )
                .child(
                    div()
                        .text_size(px(11.6 * scale))
                        .text_color(ink)
                        .font_weight(FontWeight::SEMIBOLD)
                        .whitespace_nowrap()
                        .child(match key.label {
                            "PgPréc" => "Pg ↑",
                            "PgSuiv" => "Pg ↓",
                            label => label,
                        }),
                );
            board = board.child(if enabled && !p.before {
                ui::hoverable(cell.cursor(gpui::CursorStyle::Crosshair), id)
            } else {
                cell
            });
        }

        let tool_label = |tool, label, icon| {
            ui::choice(label, Some(icon), label, p.tool == tool).when(enabled, |d| {
                d.on_click(cx.listener(move |this, _, _, cx| {
                    this.keyboard.paint.tool = tool;
                    this.keyboard.paint.compare(false);
                    cx.notify();
                }))
            })
        };
        let tools = div()
            .flex()
            .flex_wrap()
            .items_center()
            .gap(px(16.))
            .child(tool_label(Tool::Brush, "Pinceau", Icon::Palette))
            .child(tool_label(Tool::Erase, "Gomme", Icon::Close))
            .child(tool_label(Tool::Sample, "Pipette", Icon::Gauge));
        let mut palette = div().flex().flex_wrap().items_center().gap(px(8.));
        for c in PALETTE {
            let colour = u32::from_str_radix(&c[1..], 16).expect("couleur de palette");
            palette = palette.child(
                swatch(
                    SharedString::from(format!("mk770-brush-{c}")),
                    c,
                    p.tool == Tool::Brush && p.brush == colour,
                )
                .when(enabled, |d| {
                    d.on_click(cx.listener(move |this, _, _, cx| {
                        this.keyboard.paint.set_brush(colour);
                        this.keyboard.paint.compare(false);
                        cx.notify();
                    }))
                }),
            );
        }
        let mut zones = div().flex().flex_wrap().gap(px(12.));
        for (zone, label) in Zone::ALL {
            zones = zones.child(ui::choice(label, None, label, p.zone == zone).on_click(
                cx.listener(move |this, _, _, cx| {
                    this.keyboard.paint.zone = zone;
                    cx.notify();
                }),
            ));
        }
        let mut patterns = div().flex().flex_wrap().gap(px(6.));
        for (pattern, label) in [
            (Pattern::Fill, "Remplir"),
            (Pattern::Clear, "Éteindre"),
            (Pattern::Gradient, "Dégradé"),
        ] {
            patterns =
                patterns.child(ui::pill(label, None, label, Tone::Soft).when(enabled, |d| {
                    d.on_click(cx.listener(move |this, _, _, cx| {
                        this.keyboard.paint.pattern(pattern);
                        cx.notify();
                    }))
                }));
        }

        let undo = ui::pill(
            "mk770-undo",
            Some(Icon::ChevronLeft),
            "Annuler",
            Tone::Plain,
        )
        .when(p.undo.is_empty() || !enabled, |d| d.opacity(0.38))
        .when(enabled && !p.undo.is_empty(), |d| {
            d.on_click(cx.listener(|this, _, _, cx| {
                this.keyboard.paint.undo();
                cx.notify();
            }))
        });
        let redo = ui::pill(
            "mk770-redo",
            Some(Icon::ChevronRight),
            "Rétablir",
            Tone::Plain,
        )
        .when(p.redo.is_empty() || !enabled, |d| d.opacity(0.38))
        .when(enabled && !p.redo.is_empty(), |d| {
            d.on_click(cx.listener(|this, _, _, cx| {
                this.keyboard.paint.redo();
                cx.notify();
            }))
        });
        let mut comparison = div().flex().gap(px(12.));
        for (before, label) in [(true, "Enregistré"), (false, "Brouillon")] {
            comparison =
                comparison.child(ui::choice(label, None, label, p.before == before).on_click(
                    cx.listener(move |this, _, _, cx| {
                        this.keyboard.paint.compare(before);
                        cx.notify();
                    }),
                ));
        }

        let can_apply = enabled
            && ready
            && !p.before
            && (p.dirty()
                || self
                    .keyboard
                    .live()
                    .is_some_and(|s| s.active_effect != data::CUSTOM));
        let apply = ui::pill(
            "mk770-apply",
            Some(Icon::Check),
            "Appliquer au clavier",
            Tone::Primary,
        )
        .when(!can_apply, |d| d.opacity(0.5))
        .when(can_apply, |d| {
            d.on_click(cx.listener(|this, _, _, cx| this.keyboard_apply_paint(cx)))
        });
        let changed = p.changed_keys();
        let note = if p.before {
            "Comparaison avec les couleurs enregistrées.".into()
        } else if !ready {
            "Brouillon hors ligne · Branche le clavier en USB pour appliquer.".into()
        } else if changed == 0 {
            "Aucune touche modifiée".into()
        } else if changed == 1 {
            "1 touche modifiée".into()
        } else {
            format!("{changed} touches modifiées")
        };
        let files = div()
            .flex()
            .flex_wrap()
            .items_center()
            .gap(px(8.))
            .child(
                ui::pill(
                    "mk770-pattern-open",
                    Some(Icon::Upload),
                    "Ouvrir un motif",
                    Tone::Plain,
                )
                .when(enabled, |d| {
                    d.on_click(cx.listener(|this, _, _, cx| this.paint_file(false, cx)))
                }),
            )
            .child(
                ui::pill(
                    "mk770-pattern-save",
                    Some(Icon::Download),
                    "Enregistrer le motif",
                    Tone::Plain,
                )
                .when(enabled, |d| {
                    d.on_click(cx.listener(|this, _, _, cx| this.paint_file(true, cx)))
                }),
            )
            .when_some(p.file_note.as_ref(), |d, (ok, note)| {
                d.child(ui::caption(note.clone()).text_color(rgb(if *ok {
                    ui::GREEN_TEXT
                } else {
                    ui::RED_TEXT
                })))
            });
        let fps = ui::pill(
            "mk770-fps",
            Some(Icon::Game),
            "Motif ZQSD · clavier entier",
            Tone::Plain,
        )
        .when(enabled, |d| {
            d.on_click(cx.listener(|this, _, _, cx| {
                this.keyboard.paint.pattern(Pattern::Fps);
                cx.notify();
            }))
        });
        let reset = ui::pill(
            "mk770-reset-paint",
            Some(Icon::History),
            "Repartir de l’enregistré",
            Tone::Plain,
        )
        .when(enabled, |d| {
            d.on_click(cx.listener(|this, _, _, cx| {
                let paint = &mut this.keyboard.paint;
                paint.replace(paint.saved);
                cx.notify();
            }))
        });

        div()
            .max_w(px(1160.))
            .mx_auto()
            .w_full()
            .flex()
            .flex_col()
            .gap(px(17.))
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .items_center()
                    .justify_between()
                    .gap(px(8.))
                    .child(
                        div()
                            .flex()
                            .flex_wrap()
                            .items_center()
                            .gap(px(8.))
                            .child(tools)
                            .child(undo)
                            .child(redo),
                    )
                    .child(comparison),
            )
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .items_center()
                    .justify_between()
                    .gap(px(10.))
                    .child(palette)
                    .child(ui::caption("Clique ou glisse pour peindre.")),
            )
            .child(div().flex().justify_center().child(board))
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .items_center()
                    .justify_between()
                    .gap(px(10.))
                    .child(
                        div()
                            .flex()
                            .flex_wrap()
                            .items_center()
                            .gap(px(6.))
                            .child(ui::hint(note))
                            .when(p.dirty(), |d| d.child(reset)),
                    )
                    .child(apply),
            )
            .child(ui::settings_columns(
                width,
                vec![
                    ui::section(Icon::Palette, ui::ACCENT, "Couleur du pinceau", None)
                        .child(ui::caption(format!("#{:06X}", p.brush)))
                        .child(self.colour_picker(
                            crate::colour::Target::Brush,
                            &format!("#{:06X}", p.brush),
                            cx,
                        ))
                        .into_any_element(),
                ],
                vec![
                    ui::section(Icon::Layers, 0xa255f0, "Zones et motifs", None)
                        .child(zones)
                        .child(patterns)
                        .child(fps)
                        .child(self.colour_picker(
                            crate::colour::Target::Gradient,
                            &format!("#{:06X}", p.second),
                            cx,
                        ))
                        .into_any_element(),
                ],
            ))
            .child(files)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn un_trait_rapide_passe_par_les_touches_et_s_annule_en_un_geste() {
        let mut p = PaintEditor::default();
        let frame = PaintFrame {
            bounds: Bounds::new(gpui::point(px(0.), px(0.)), gpui::size(px(800.), px(300.))),
            scale: 1.,
        };
        let from = BOARD.keys.iter().find(|k| k.label == "A").unwrap();
        let to = BOARD.keys.iter().find(|k| k.label == "T").unwrap();
        let point = |k: &KeyBox| {
            gpui::point(
                px(k.left + k.width / 2. + INSET),
                px(k.top + KEY / 2. + INSET),
            )
        };
        p.begin(frame, point(from));
        p.move_to(point(to));
        p.finish();
        assert_eq!(&p.current()[37..42], &[DEFAULT_BRUSH; 5]);
        assert_eq!(p.current()[36], OFF);
        assert_eq!(p.undo.len(), 1);
        p.undo();
        assert!(!p.dirty());
        p.redo();
        assert_eq!(p.current()[41], DEFAULT_BRUSH);
    }

    #[test]
    fn zones_et_touches_a_plusieurs_leds() {
        let mut p = PaintEditor {
            zone: Zone::Arrows,
            ..PaintEditor::default()
        };
        p.pattern(Pattern::Fill);
        assert_eq!(p.current()[98], DEFAULT_BRUSH);
        assert_eq!(p.current()[97], DEFAULT_BRUSH);
        assert_eq!(p.current()[91], OFF);
        assert_eq!(p.changed_keys(), 4);
        p.pattern(Pattern::Fps);
        assert_eq!(&p.current()[91..94], &[DEFAULT_BRUSH; 3]);
        assert_eq!(p.current()[38], DEFAULT_BRUSH);
        assert_eq!(p.current()[39], p.second);
    }

    #[test]
    fn confirmation_et_relecture_preservent_le_brouillon_et_l_historique() {
        let mut p = PaintEditor::default();
        p.pattern(Pattern::Gradient);
        let draft = *p.current();
        assert_ne!(draft[37], draft[44]);
        assert_ne!(draft[37], OFF);
        p.set_saved([0xffffff; data::LEDS], false);
        assert_eq!(p.current(), &draft);
        p.compare(true);
        assert_eq!(p.displayed()[37], 0xffffff);
        assert_eq!(p.current(), &draft);
        p.set_saved(draft, true);
        assert!(!p.dirty());
        p.undo();
        assert!(p.dirty());
        p.pattern(Pattern::Fill);
        assert!(p.redo.is_empty());
    }

    #[test]
    fn motif_json_valide_sans_ecrasement_et_entrees_bornees() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("motif.json");
        let colours = [0xb24dff; data::LEDS];
        Profile::export(&path, &colours).unwrap();
        assert_eq!(Profile::read(&path).unwrap(), colours);
        assert!(Profile::export(&path, &[0; data::LEDS]).is_err());
        assert_eq!(Profile::read(&path).unwrap(), colours);
        let mut bad = write_colours(&colours);
        bad[91] = "#FF00ZZ".into();
        std::fs::write(
            &path,
            serde_json::to_vec(&Profile {
                format: PROFILE_FORMAT.into(),
                keys: bad,
            })
            .unwrap(),
        )
        .unwrap();
        assert!(Profile::read(&path).is_err());
        std::fs::write(&path, vec![b' '; PROFILE_LIMIT as usize + 1]).unwrap();
        assert!(Profile::read(&path).unwrap_err().contains("volumineux"));
    }
}
