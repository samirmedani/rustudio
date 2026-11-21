//! Édition locale et programmation des macros embarquées du MK770.
use crate::{
    mixer::Mixer,
    ui::{self, Icon, Tone, rgb},
};
use gpui::{
    Context, Div, FocusHandle, KeyDownEvent, PathPromptOptions, SharedString, Window, div,
    prelude::*, px,
};
use rustudio_core::keyboard_macro::{self as data, Assignment, Event, Macro, Mode, Slot};
use rustudio_windows_desktop::keyboard as keys;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::BTreeSet,
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
    time::Instant,
};

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct File {
    format: u8,
    module: String,
    data: Macro,
}
impl File {
    fn new(data: Macro) -> Self {
        Self {
            format: 1,
            module: "cooler-master-mk770".into(),
            data,
        }
    }
    fn read(path: &Path) -> Result<Macro, String> {
        let mut bytes = Vec::new();
        fs::File::open(path)
            .map_err(|e| e.to_string())?
            .take(32769)
            .read_to_end(&mut bytes)
            .map_err(|e| e.to_string())?;
        if bytes.len() > 32768 {
            return Err("Fichier de macro trop volumineux.".into());
        }
        let file: Self = serde_json::from_slice(&bytes)
            .map_err(|_| "Fichier de macro Rustudio illisible.".to_owned())?;
        if file.format != 1 || file.module != "cooler-master-mk770" {
            return Err("Format de macro incompatible.".into());
        }
        file.data.validate()?;
        Ok(file.data)
    }
}
fn write_json(path: &Path, value: &impl Serialize, replace: bool) -> Result<(), String> {
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    let mut file = tempfile::NamedTempFile::new_in(parent).map_err(|e| e.to_string())?;
    serde_json::to_writer_pretty(&mut file, value).map_err(|e| e.to_string())?;
    file.write_all(b"\n").map_err(|e| e.to_string())?;
    file.as_file().sync_all().map_err(|e| e.to_string())?;
    if replace {
        file.persist(path)
    } else {
        file.persist_noclobber(path)
    }
    .map_err(|e| format!("Enregistrement impossible : {e}"))?;
    Ok(())
}

pub struct Editor {
    path: PathBuf,
    saved: Vec<Macro>,
    pub draft: Macro,
    selected_saved: Option<usize>,
    slots: Option<Vec<Slot>>,
    slot: u8,
    source: u16,
    busy: bool,
    notice: Option<(bool, String)>,
    name_focus: FocusHandle,
    delay_focus: FocusHandle,
    capture_focus: FocusHandle,
    selected_event: Option<usize>,
    delay: String,
    manual_key: u16,
    recording: Option<Instant>,
    #[cfg(windows)]
    capture: Option<keys::Capture>,
    #[cfg(windows)]
    native_time: Option<u32>,
    held: BTreeSet<u16>,
    dirty: bool,
    library_blocked: bool,
    name_selected: bool,
    delay_selected: bool,
}
impl Editor {
    pub fn new(dir: PathBuf, cx: &mut Context<Mixer>) -> Self {
        let path = dir.join("macros.json");
        let mut notice = None;
        let mut library_blocked = false;
        let saved = if path.exists() {
            let result = (|| -> Result<Vec<Macro>, String> {
                let mut bytes = Vec::new();
                fs::File::open(&path)
                    .map_err(|e| e.to_string())?
                    .take(1048577)
                    .read_to_end(&mut bytes)
                    .map_err(|e| e.to_string())?;
                if bytes.len() > 1048576 {
                    return Err("Bibliothèque de macros trop volumineuse.".into());
                }
                let items: Vec<Macro> = serde_json::from_slice(&bytes)
                    .map_err(|_| "Bibliothèque de macros illisible.".to_owned())?;
                if items.len() > 64 {
                    return Err("La bibliothèque dépasse 64 macros.".into());
                }
                for m in &items {
                    m.validate()?;
                }
                Ok(items)
            })();
            match result {
                Ok(v) => v,
                Err(e) => {
                    library_blocked = true;
                    notice = Some((false, e));
                    vec![]
                }
            }
        } else {
            vec![]
        };
        Self {
            path,
            saved,
            draft: Macro {
                name: String::new(),
                mode: Mode::Once,
                events: vec![],
            },
            selected_saved: None,
            slots: None,
            slot: 0,
            source: 69,
            busy: false,
            notice,
            name_focus: cx.focus_handle(),
            delay_focus: cx.focus_handle(),
            capture_focus: cx.focus_handle(),
            selected_event: None,
            delay: "50".into(),
            manual_key: 4,
            recording: None,
            #[cfg(windows)]
            capture: None,
            #[cfg(windows)]
            native_time: None,
            held: BTreeSet::new(),
            dirty: false,
            library_blocked,
            name_selected: false,
            delay_selected: false,
        }
    }
    pub fn stop(&mut self) {
        if self.recording.is_none() {
            return;
        }
        #[cfg(windows)]
        {
            self.capture_events();
            self.capture = None;
        }
        let held: Vec<_> = self.held.iter().copied().collect();
        for key in held {
            self.append(key, false);
        }
        self.recording = None;
        self.held.clear();
    }
    fn append(&mut self, key: u16, down: bool) {
        if down && self.held.contains(&key) || !down && !self.held.contains(&key) {
            return;
        }
        // Garder une place pour chaque relâchement, même à la limite du clavier.
        if down && self.draft.events.len() + self.held.len() + 2 > data::MAX_EVENTS {
            self.notice = Some((
                false,
                "Nombre maximal d’événements atteint ; termine l’enregistrement.".into(),
            ));
            return;
        }
        let now = Instant::now();
        let elapsed = self
            .recording
            .map(|t| now.duration_since(t).as_millis())
            .unwrap_or(0);
        if elapsed > 32767 {
            self.notice = Some((
                false,
                "Pause supérieure à 32 767 ms : son délai a été limité. Tu peux le modifier."
                    .into(),
            ));
        }
        let delay_ms = if self.draft.events.is_empty() {
            0
        } else {
            elapsed.min(32767) as u16
        };
        self.draft.events.push(Event {
            key,
            down,
            delay_ms,
        });
        if down {
            self.held.insert(key);
        } else {
            self.held.remove(&key);
        }
        self.recording = Some(now);
        self.dirty = true;
    }
    #[cfg(not(windows))]
    fn modifiers(&mut self) {
        let current = keys::modifiers();
        let old: Vec<_> = self.held.iter().copied().filter(|k| *k >= 224).collect();
        for k in old {
            if !current.contains(&k) {
                self.append(k, false);
            }
        }
        for k in current {
            if !self.held.contains(&k) {
                self.append(k, true);
            }
        }
    }
    #[cfg(windows)]
    fn capture_events(&mut self) {
        let events = self
            .capture
            .as_ref()
            .map(|c| c.events())
            .unwrap_or_default();
        for e in events {
            if e.key == 41 {
                continue;
            }
            let before = self.draft.events.len();
            self.append(e.key, e.down);
            if self.draft.events.len() > before {
                let ms = self
                    .native_time
                    .map(|t| e.time.wrapping_sub(t))
                    .unwrap_or(0);
                if ms > 32767 {
                    self.notice=Some((false,"Pause supérieure à 32 767 ms : son délai a été limité. Tu peux le modifier.".into()));
                }
                self.draft.events.last_mut().unwrap().delay_ms = ms.min(32767) as u16;
                self.native_time = Some(e.time);
            }
        }
    }
    fn load(&mut self, data: Macro) {
        self.stop();
        self.draft = data;
        self.selected_event = None;
        self.dirty = false;
        self.notice = None;
        self.name_selected = false;
        self.delay_selected = false;
    }
    fn save(&mut self) -> Result<(), String> {
        self.stop();
        if self.library_blocked {
            return Err("La bibliothèque existante est illisible. Exporte ta macro pour la conserver sans écraser ce fichier.".into());
        }
        self.draft.validate()?;
        let mut next = self.saved.clone();
        if let Some(i) = self.selected_saved {
            next[i] = self.draft.clone();
        } else {
            if next.len() >= 64 {
                return Err("La bibliothèque contient déjà 64 macros.".into());
            }
            next.push(self.draft.clone());
        }
        write_json(&self.path, &next, true)?;
        self.selected_saved = Some(self.selected_saved.unwrap_or(next.len() - 1));
        self.saved = next;
        self.dirty = false;
        Ok(())
    }
    fn result(&mut self, result: Result<(), String>, success: &str) {
        self.notice = Some(match result {
            Ok(()) => (true, success.into()),
            Err(e) => (false, e),
        });
    }
}
fn label(key: u16) -> String {
    if key == 88 {
        return "Entrée (pavé numérique)".into();
    }
    keys::label(key).unwrap_or_else(|| data::key_name(key))
}
fn mode_name(mode: Mode) -> &'static str {
    match mode {
        Mode::Once => "Une fois",
        Mode::Hold => "Maintien",
        Mode::Toggle => "Bascule",
    }
}

impl Mixer {
    pub(crate) fn macro_command(
        &mut self,
        action: &'static str,
        body: Value,
        cx: &mut Context<Self>,
    ) {
        if self.macros.busy {
            return;
        }
        self.macros.stop();
        self.macros.busy = true;
        self.macros.notice = None;
        cx.notify();
        let client = self.client.clone();
        let work = cx
            .background_executor()
            .spawn(async move { client.keyboard_macros(action, &body) });
        cx.spawn(async move |this, cx| {
            let result = work.await;
            let _ = this.update(cx, |this, cx| {
                this.macros.busy = false;
                match result {
                    Ok(slots) => {
                        this.macros.slots = Some(slots);
                        this.macros.notice = Some((
                            true,
                            match action {
                                "macro-assign" => "Macro enregistrée et relue dans le clavier.",
                                "macro-remove" => "Macro retirée ; touche d’origine rétablie.",
                                _ => "Macros du clavier relues.",
                            }
                            .into(),
                        ));
                    }
                    Err(e) => {
                        this.macros.slots = None;
                        this.macros.notice = Some((false, e));
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }
    fn macro_import(&mut self, cx: &mut Context<Self>) {
        self.macros.stop();
        let picker = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: None,
        });
        cx.spawn(async move |this, cx| {
            let result = match picker.await {
                Ok(Ok(Some(p))) if !p.is_empty() => {
                    let path = p[0].clone();
                    cx.background_executor()
                        .spawn(async move { File::read(&path) })
                        .await
                }
                Ok(Ok(_)) => return,
                _ => Err("Sélecteur de macros indisponible.".into()),
            };
            let _ = this.update(cx, |this, cx| {
                match result {
                    Ok(data) => {
                        this.macros.load(data);
                        this.macros.selected_saved = None;
                        this.macros.dirty = true;
                        this.macros.notice = Some((
                            true,
                            "Macro importée dans l’éditeur ; enregistre-la pour la conserver."
                                .into(),
                        ));
                    }
                    Err(e) => this.macros.notice = Some((false, e)),
                }
                cx.notify();
            });
        })
        .detach();
    }
    fn macro_export(&mut self, cx: &mut Context<Self>) {
        self.macros.stop();
        if let Err(e) = self.macros.draft.validate() {
            self.macros.notice = Some((false, e));
            cx.notify();
            return;
        }
        let file = File::new(self.macros.draft.clone());
        let picker = cx.prompt_for_new_path(&std::env::temp_dir(), Some("macro-rustudio.json"));
        cx.spawn(async move |this, cx| {
            let result = match picker.await {
                Ok(Ok(Some(p))) => {
                    let path = if p.extension().is_none() {
                        p.with_extension("json")
                    } else {
                        p
                    };
                    cx.background_executor()
                        .spawn(async move { write_json(&path, &file, false) })
                        .await
                }
                Ok(Ok(None)) => return,
                _ => Err("Dossier d’export indisponible.".into()),
            };
            let _ = this.update(cx, |this, cx| {
                this.macros.result(result, "Macro exportée en JSON.");
                cx.notify();
            });
        })
        .detach();
    }
    pub(crate) fn macros_view(&self, window: &mut Window, cx: &mut Context<Self>) -> Div {
        let m = &self.macros;
        let idle = !m.busy && m.recording.is_none();
        let connected = self
            .keyboard
            .status
            .as_ref()
            .is_some_and(|v| v.device.status == "ready");
        let mut root = div()
            .w_full()
            .min_w(px(0.))
            .whitespace_normal()
            .flex()
            .flex_col()
            .gap(px(16.));
        let mut controls = div()
            .flex()
            .flex_wrap()
            .items_center()
            .gap(px(8.))
            .child(
                ui::pill("macro-new", Some(Icon::Plus), "Nouvelle", Tone::Plain).when(idle, |d| {
                    d.on_click(cx.listener(|this, _, _, cx| {
                        this.macros.load(Macro {
                            name: String::new(),
                            mode: Mode::Once,
                            events: vec![],
                        });
                        this.macros.selected_saved = None;
                        cx.notify();
                    }))
                }),
            )
            .child(
                ui::pill("macro-import", Some(Icon::Upload), "Importer", Tone::Plain)
                    .when(idle, |d| {
                        d.on_click(cx.listener(|this, _, _, cx| this.macro_import(cx)))
                    }),
            )
            .child(
                ui::pill(
                    "macro-export",
                    Some(Icon::Download),
                    "Exporter",
                    Tone::Plain,
                )
                .when(idle, |d| {
                    d.on_click(cx.listener(|this, _, _, cx| this.macro_export(cx)))
                }),
            )
            .child(
                ui::pill(
                    "macro-refresh",
                    Some(Icon::Refresh),
                    if m.busy {
                        "Lecture / envoi…"
                    } else {
                        "Relire le clavier"
                    },
                    Tone::Plain,
                )
                .when(idle && connected, |d| {
                    d.on_click(
                        cx.listener(|this, _, _, cx| this.macro_command("macros", json!({}), cx)),
                    )
                }),
            );
        if let Some((ok, text)) = &m.notice {
            controls = controls.child(
                div()
                    .text_size(px(13.))
                    .text_color(rgb(if *ok { ui::GREEN_TEXT } else { ui::RED_TEXT }))
                    .child(text.clone()),
            );
        }
        root = root
            .child(ui::notice(
                false,
                "Fonction expérimentale : l’envoi des macros dans le clavier n’a pas encore été validé sur un vrai MK770. Garde une copie de tes macros sur ce PC ou en JSON avant de les envoyer.",
            ))
            .child(controls);
        let mut saved = ui::card().w_full().min_w(px(0.)).child(ui::card_title(
            Some((Icon::Bookmark, ui::ACCENT)),
            "Sur ce PC",
            Some(format!("{} / 64 macros", m.saved.len()).into()),
        ));
        if m.saved.is_empty() {
            saved = saved.child(ui::hint(
                "Crée ou importe une macro, puis enregistre-la ici.",
            ));
        }
        for (i, item) in m.saved.iter().enumerate() {
            let selected = m.selected_saved == Some(i);
            saved = saved.child(
                ui::pill(
                    SharedString::from(format!("macro-local-{i}")),
                    Some(Icon::Keyboard),
                    item.name.clone(),
                    if selected { Tone::Primary } else { Tone::Plain },
                )
                .when(idle, |d| {
                    d.on_click(cx.listener(move |this, _, _, cx| {
                        let data = this.macros.saved[i].clone();
                        this.macros.load(data);
                        this.macros.selected_saved = Some(i);
                        cx.notify();
                    }))
                }),
            );
        }
        let mut board=ui::card().w_full().min_w(px(0.)).child(ui::card_title(Some((Icon::Keyboard,ui::ACCENT)),"Dans le clavier",None)).child(ui::caption("Profil actif · 12 emplacements"))
            .child(ui::hint("Les macros envoyées restent disponibles quand Rustudio est fermé. Programme-les par câble USB."));
        if let Some(slots) = &m.slots {
            if slots.is_empty() {
                board = board.child(ui::caption("Aucune macro affectée dans le profil actif."));
            }
            for item in slots {
                let id = item.id;
                let detail = format!(
                    "{} · {}",
                    item.data.name,
                    item.bindings
                        .iter()
                        .map(|b| label(b.key))
                        .collect::<Vec<_>>()
                        .join(", ")
                );
                board = board.child(
                    ui::pill(
                        SharedString::from(format!("macro-board-{id}")),
                        None,
                        format!("{} · {detail}", id + 1),
                        Tone::Plain,
                    )
                    .when(idle, |d| {
                        d.on_click(cx.listener(move |this, _, _, cx| {
                            if let Some(slot) = this
                                .macros
                                .slots
                                .as_ref()
                                .and_then(|s| s.iter().find(|s| s.id == id))
                                .cloned()
                            {
                                this.macros.load(slot.data);
                                this.macros.slot = id;
                                this.macros.source = slot.bindings[0].key;
                                this.macros.selected_saved = None;
                            }
                            cx.notify();
                        }))
                    }),
                );
            }
        } else {
            board = board.child(ui::caption(if connected {
                "Relis le clavier pour afficher ses affectations."
            } else {
                "Clavier non détecté : l’éditeur et les fichiers restent disponibles."
            }));
        }
        root = root.child(ui::grid(
            vec![saved.into_any_element(), board.into_any_element()],
            2,
        ));
        let editing = m.name_focus.is_focused(window);
        let name = ui::field(
            "macro-name",
            &m.name_focus,
            &m.draft.name,
            "Nom de la macro",
            editing,
            idle,
        )
        .flex_1()
        .min_w(px(180.))
        .when(editing && m.name_selected, |d| {
            d.bg(ui::alpha(ui::ACCENT, 0.16))
        })
        .on_key_down(cx.listener(|this, event: &KeyDownEvent, _, cx| {
            if this.macros.busy || this.macros.recording.is_some() {
                return;
            }
            if event.keystroke.modifiers.control && event.keystroke.key == "a" {
                this.macros.name_selected = true;
                cx.stop_propagation();
                cx.notify();
                return;
            }
            if this.macros.name_selected
                && (event.keystroke.key_char.is_some()
                    || event.keystroke.key == "backspace"
                    || event.keystroke.modifiers.control && event.keystroke.key == "v")
            {
                this.macros.draft.name.clear();
                this.macros.name_selected = false;
            }
            if !matches!(
                ui::edit_text(&mut this.macros.draft.name, event, 39, |_| true, cx),
                ui::Edit::Ignored
            ) {
                this.macros.dirty = true;
                cx.stop_propagation();
                cx.notify();
            }
        }));
        let modes = [Mode::Once, Mode::Hold, Mode::Toggle];
        let mode = self
            .dropdown(
                "macro-mode",
                mode_name(m.draft.mode).into(),
                modes
                    .iter()
                    .map(|v| (mode_name(*v).into(), *v == m.draft.mode))
                    .collect(),
                idle,
                move |this, i, _| {
                    this.macros.draft.mode = modes[i];
                    this.macros.dirty = true;
                },
                cx,
            )
            .w(px(130.));
        let mut editor=ui::card().child(ui::card_title(Some((Icon::Sliders,ui::ACCENT)),"Éditeur",Some(format!("{} / 70 événements{}",m.draft.events.len(),if m.dirty{" · Modifications non enregistrées"}else{""}).into())))
            .child(div().flex().flex_wrap().gap(px(10.)).child(name).child(mode))
            .child(ui::caption(m.draft.mode.label()))
            .child(ui::caption("Chaque appui et chaque relâchement compte comme un événement. Les délais sont placés avant la touche."));
        let focus = m.capture_focus.clone();
        let is_recording = m.recording.is_some();
        let record = div()
            .id("macro-capture")
            .track_focus(&focus)
            .tab_stop(idle || is_recording)
            .p(px(14.))
            .rounded(px(12.))
            .border_1()
            .border_color(ui::alpha(if is_recording { ui::RED } else { ui::INK }, 0.3))
            .bg(ui::alpha(ui::ACCENT, 0.05))
            .on_key_down(cx.listener(|this, e: &KeyDownEvent, _, cx| {
                if this.macros.recording.is_none() {
                    return;
                }
                if e.keystroke.key == "escape" {
                    this.macros.stop();
                    cx.stop_propagation();
                    cx.notify();
                    return;
                }
                #[cfg(windows)]
                this.macros.capture_events();
                #[cfg(not(windows))]
                if !e.is_held {
                    this.macros.modifiers();
                    if let Some(key) = keys::usage(&e.keystroke.key) {
                        this.macros.append(key, true);
                    } else {
                        this.macros.notice = Some((
                            false,
                            "Cette touche ne peut pas être enregistrée ; ajoute-la dans l’éditeur."
                                .into(),
                        ));
                    }
                }
                cx.stop_propagation();
                cx.notify();
            }))
            .on_key_up(cx.listener(|this, e: &gpui::KeyUpEvent, _, cx| {
                if this.macros.recording.is_none() {
                    return;
                }
                #[cfg(windows)]
                {
                    let _ = e;
                    this.macros.capture_events();
                }
                #[cfg(not(windows))]
                {
                    if let Some(key) = keys::usage(&e.keystroke.key) {
                        this.macros.append(key, false);
                    }
                    this.macros.modifiers();
                }
                cx.stop_propagation();
                cx.notify();
            }))
            .on_modifiers_changed(cx.listener(|this, _, window, cx| {
                if this.macros.recording.is_some() && this.macros.capture_focus.is_focused(window) {
                    #[cfg(windows)]
                    this.macros.capture_events();
                    #[cfg(not(windows))]
                    this.macros.modifiers();
                    cx.stop_propagation();
                    cx.notify();
                }
            }))
            .on_mouse_down_out(cx.listener(|this, _, _, cx| {
                if this.macros.recording.is_some() {
                    this.macros.stop();
                    cx.notify();
                }
            }))
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .items_center()
                    .gap(px(10.))
                    .child(
                        ui::pill(
                            "macro-record",
                            Some(if is_recording {
                                Icon::Check
                            } else {
                                Icon::Play
                            }),
                            if is_recording {
                                "Terminer"
                            } else {
                                "Enregistrer les touches"
                            },
                            if is_recording {
                                Tone::Danger
                            } else {
                                Tone::Plain
                            },
                        )
                        .when(!m.busy, |d| {
                            d.on_click(cx.listener(move |this, _, window, cx| {
                                if this.macros.recording.is_some() {
                                    this.macros.stop();
                                } else {
                                    #[cfg(windows)]
                                    let raw =
                                        raw_window_handle::HasWindowHandle::window_handle(&*window)
                                            .ok()
                                            .map(|h| h.as_raw());
                                    #[cfg(windows)]
                                    let hwnd =
                                        if let Some(raw_window_handle::RawWindowHandle::Win32(h)) =
                                            raw
                                        {
                                            h.hwnd.get()
                                        } else {
                                            0
                                        };
                                    #[cfg(windows)]
                                    match keys::Capture::start(hwnd) {
                                        Ok(capture) => {
                                            this.macros.capture = Some(capture);
                                            this.macros.native_time = None;
                                        }
                                        Err(error) => {
                                            this.macros.notice = Some((false, error));
                                            cx.notify();
                                            return;
                                        }
                                    }
                                    this.macros.recording = Some(Instant::now());
                                    this.macros.held.clear();
                                    this.macros.notice = None;
                                    focus.focus(window);
                                }
                                cx.notify();
                            }))
                        }),
                    )
                    .child(ui::caption(if is_recording {
                        "Tape dans cette zone · Échap termine · Ajout à la fin de la séquence"
                    } else {
                        "La capture reste dans cette fenêtre et ajoute les touches à la fin."
                    })),
            );
        editor = editor.child(record);
        let mut table = div()
            .id("macro-events")
            .max_h(px(270.))
            .overflow_y_scroll()
            .flex()
            .flex_col()
            .gap(px(5.));
        if m.draft.events.is_empty() {
            table = table.child(ui::hint(
                "Aucun événement. Enregistre tes touches ou ajoute un appui et son relâchement.",
            ));
        }
        for (i, e) in m.draft.events.iter().enumerate() {
            let selected = m.selected_event == Some(i);
            table = table.child(
                ui::pill(
                    SharedString::from(format!("macro-event-{i}")),
                    None,
                    format!(
                        "{:02}    +{} ms    {}    {}",
                        i + 1,
                        e.delay_ms,
                        if e.down { "Appuyer" } else { "Relâcher" },
                        label(e.key)
                    ),
                    if selected { Tone::Primary } else { Tone::Plain },
                )
                .when(idle, |d| {
                    d.on_click(cx.listener(move |this, _, _, cx| {
                        let e = &this.macros.draft.events[i];
                        this.macros.selected_event = Some(i);
                        this.macros.manual_key = e.key;
                        this.macros.delay = e.delay_ms.to_string();
                        cx.notify();
                    }))
                }),
            );
        }
        editor = editor.child(table);
        let options: Vec<_> = (4..=231).filter(|k| data::valid_key(*k)).collect();
        let selected = m.manual_key;
        let key_select = self
            .dropdown(
                "macro-event-key",
                label(selected),
                options
                    .iter()
                    .map(|k| (format!("{} · {:02X}", label(*k), k), *k == selected))
                    .collect(),
                idle,
                move |this, i, _| {
                    this.macros.manual_key = options[i];
                },
                cx,
            )
            .w(px(180.));
        let delay = ui::field(
            "macro-delay",
            &m.delay_focus,
            &m.delay,
            "Délai (ms)",
            m.delay_focus.is_focused(window),
            idle,
        )
        .w(px(100.))
        .on_key_down(cx.listener(|this, e: &KeyDownEvent, _, cx| {
            if this.macros.busy || this.macros.recording.is_some() {
                return;
            }
            if e.keystroke.modifiers.control && e.keystroke.key == "a" {
                this.macros.delay_selected = true;
            } else {
                if this.macros.delay_selected
                    && (e.keystroke.key_char.is_some()
                        || e.keystroke.key == "backspace"
                        || e.keystroke.modifiers.control && e.keystroke.key == "v")
                {
                    this.macros.delay.clear();
                    this.macros.delay_selected = false;
                }
                ui::edit_text(&mut this.macros.delay, e, 5, |c| c.is_ascii_digit(), cx);
            }
            cx.stop_propagation();
            cx.notify();
        }));
        let mut row = div()
            .flex()
            .flex_wrap()
            .items_center()
            .gap(px(8.))
            .child(key_select)
            .child(ui::caption("Délai (ms)"))
            .child(delay);
        for down in [true, false] {
            row = row.child(
                ui::pill(
                    if down {
                        "macro-add-down"
                    } else {
                        "macro-add-up"
                    },
                    Some(Icon::Plus),
                    if down { "Appui" } else { "Relâchement" },
                    Tone::Plain,
                )
                .when(idle, |d| {
                    d.on_click(cx.listener(move |this, _, _, cx| {
                        let m = &mut this.macros;
                        if m.draft.events.len() >= 70 {
                            m.notice =
                                Some((false, "La macro contient déjà 70 événements.".into()));
                        } else if let Ok(delay_ms) = m
                            .delay
                            .parse::<u16>()
                            .map_err(|_| ())
                            .and_then(|v| if v <= 32767 { Ok(v) } else { Err(()) })
                        {
                            m.draft.events.push(Event {
                                key: m.manual_key,
                                down,
                                delay_ms,
                            });
                            m.dirty = true;
                        } else {
                            m.notice = Some((false, "Choisis un délai de 0 à 32 767 ms.".into()));
                        }
                        cx.notify();
                    }))
                }),
            );
        }
        editor = editor.child(row);
        if let Some(i) = m.selected_event {
            let mut edits = div().flex().flex_wrap().gap(px(8.));
            edits = edits.child(
                ui::pill("macro-edit", None, "Modifier l’événement", Tone::Plain).when(idle, |d| {
                    d.on_click(cx.listener(move |this, _, _, cx| {
                        let m = &mut this.macros;
                        if let Some(ms) = m.delay.parse::<u16>().ok().filter(|v| *v <= 32767) {
                            if let Some(e) = m.draft.events.get_mut(i) {
                                e.key = m.manual_key;
                                e.delay_ms = ms;
                                m.dirty = true;
                            }
                        } else {
                            m.notice = Some((false, "Délai invalide.".into()));
                        }
                        cx.notify();
                    }))
                }),
            );
            for up in [true, false] {
                edits = edits.child(
                    ui::pill(
                        if up { "macro-up" } else { "macro-down" },
                        None,
                        if up { "Monter" } else { "Descendre" },
                        Tone::Plain,
                    )
                    .when(idle, |d| {
                        d.on_click(cx.listener(move |this, _, _, cx| {
                            let m = &mut this.macros;
                            let j = if up {
                                i.checked_sub(1)
                            } else {
                                Some(i + 1).filter(|j| *j < m.draft.events.len())
                            };
                            if let Some(j) = j {
                                m.draft.events.swap(i, j);
                                m.selected_event = Some(j);
                                m.dirty = true;
                            }
                            cx.notify();
                        }))
                    }),
                );
            }
            edits = edits.child(
                ui::pill(
                    "macro-delete-event",
                    Some(Icon::Trash),
                    "Supprimer l’événement",
                    Tone::Danger,
                )
                .when(idle, |d| {
                    d.on_click(cx.listener(move |this, _, _, cx| {
                        this.macros.draft.events.remove(i);
                        this.macros.selected_event = None;
                        this.macros.dirty = true;
                        cx.notify();
                    }))
                }),
            );
            editor = editor.child(edits);
        }
        if let Err(error) = m.draft.validate() {
            editor = editor.child(ui::caption(error).text_color(rgb(ui::ORANGE_TEXT)));
        }
        editor = editor.child(
            ui::pill(
                "macro-save",
                Some(Icon::Check),
                "Enregistrer sur ce PC",
                Tone::Primary,
            )
            .when(idle, |d| {
                d.on_click(cx.listener(|this, _, _, cx| {
                    let result = this.macros.save();
                    this.macros.result(result, "Macro enregistrée sur ce PC.");
                    cx.notify();
                }))
            }),
        );
        if m.selected_saved.is_some() {
            editor=editor.child(div().flex().gap(px(8.)).child(ui::pill("macro-duplicate",Some(Icon::Plus),"Créer une copie",Tone::Plain).when(idle,|d|d.on_click(cx.listener(|this,_,_,cx|{this.macros.selected_saved=None;this.macros.dirty=true;cx.notify();})))).child(ui::pill("macro-delete-local",Some(Icon::Trash),"Supprimer du PC",Tone::Danger).when(idle,|d|d.on_click(cx.listener(|this,_,_,cx|{let m=&mut this.macros;if let Some(i)=m.selected_saved{let mut next=m.saved.clone();next.remove(i);match write_json(&m.path,&next,true){Ok(())=>{m.saved=next;m.selected_saved=None;m.dirty=true;m.notice=Some((true,"Macro supprimée du PC ; celle du clavier est conservée.".into()));}Err(e)=>m.notice=Some((false,e))}}cx.notify();})))));
        }
        root = root.child(editor);
        let slot_options = (0..12)
            .map(|id| {
                (
                    format!(
                        "Emplacement {}{}",
                        id + 1,
                        m.slots
                            .as_ref()
                            .and_then(|s| s.iter().find(|s| s.id == id))
                            .map(|s| format!(" · {}", s.data.name))
                            .unwrap_or_default()
                    ),
                    id == m.slot,
                )
            })
            .collect();
        let slot = self
            .dropdown(
                "macro-slot",
                format!("Emplacement {}", m.slot + 1),
                slot_options,
                idle,
                |this, i, _| {
                    this.macros.slot = i as u8;
                },
                cx,
            )
            .w(px(280.));
        let sources: Vec<_> = data::MATRIX
            .iter()
            .copied()
            .filter(|k| data::valid_key(*k))
            .collect();
        let source = self
            .dropdown(
                "macro-source",
                label(m.source),
                sources
                    .iter()
                    .map(|k| (label(*k), *k == m.source))
                    .collect(),
                idle,
                move |this, i, _| {
                    this.macros.source = sources[i];
                },
                cx,
            )
            .w(px(200.));
        let program=ui::card().child(ui::card_title(Some((Icon::Keyboard,ui::ACCENT)),"Affecter au clavier",None))
            .child(ui::hint("La touche choisie déclenchera la macro à la place de son action habituelle, dans le profil actif. Reste sur ce profil pendant l’envoi."))
            .child(div().flex().flex_wrap().items_center().gap(px(10.)).child(slot).child(ui::caption("Touche")).child(source))
            .child(ui::caption("Remplacer un emplacement déplace aussi sa précédente affectation. « Retirer » rétablit la touche d’origine."))
            .child(div().flex().flex_wrap().gap(px(8.))
                .child(ui::pill("macro-program",Some(Icon::Check),"Envoyer au clavier",Tone::Primary).when(!connected||!idle||m.slots.is_none(),|d|d.opacity(0.5)).when(connected&&idle&&m.slots.is_some(),|d|d.on_click(cx.listener(|this,_,_,cx|{let a=Assignment{slot:this.macros.slot,key:this.macros.source,data:this.macros.draft.clone()};match a.validate(){Ok(_)=>this.macro_command("macro-assign",json!(a),cx),Err(e)=>{this.macros.notice=Some((false,e));cx.notify();}}}))))
                .child(ui::pill("macro-remove",Some(Icon::Trash),"Retirer du clavier",Tone::Danger).when(connected&&idle&&m.slots.as_ref().is_some_and(|s|s.iter().any(|s|s.id==m.slot)),|d|d.on_click(cx.listener(|this,_,_,cx|this.macro_command("macro-remove",json!({"slot":this.macros.slot}),cx))))));
        root.child(program)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn import_export_strict_and_no_overwrite() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("macro.json");
        let m = Macro {
            name: "Copier".into(),
            mode: Mode::Once,
            events: vec![
                Event {
                    key: 6,
                    down: true,
                    delay_ms: 0,
                },
                Event {
                    key: 6,
                    down: false,
                    delay_ms: 50,
                },
            ],
        };
        write_json(&path, &File::new(m.clone()), false).unwrap();
        assert_eq!(File::read(&path).unwrap(), m);
        assert!(write_json(&path, &File::new(m), false).is_err());
        fs::write(
            &path,
            r#"{"format":2,"module":"cooler-master-mk770","data":{}}"#,
        )
        .unwrap();
        assert!(File::read(&path).is_err());
        fs::write(&path, vec![b' '; 32769]).unwrap();
        assert!(File::read(&path).is_err());
    }
}

/// Parcours complet à travers l'API du moteur de démonstration, jamais sur le vrai clavier.
pub fn verify(client: &crate::backend::Client) -> Result<(), String> {
    if client.is_live() {
        return Err("Le test des macros exige le moteur simulé.".into());
    }
    let before = client
        .keyboard_view()?
        .state
        .ok_or("Clavier simulé absent")?;
    if !client.keyboard_macros("macros", &json!({}))?.is_empty() {
        return Err("Le test exige un profil simulé vide.".into());
    }
    for (id, mode) in [Mode::Once, Mode::Hold, Mode::Toggle]
        .into_iter()
        .enumerate()
    {
        let a = Assignment {
            slot: id as u8,
            key: 67 + id as u16,
            data: Macro {
                name: format!("Test {}", id + 1),
                mode,
                events: vec![
                    Event {
                        key: 224,
                        down: true,
                        delay_ms: 0,
                    },
                    Event {
                        key: 6,
                        down: true,
                        delay_ms: 15,
                    },
                    Event {
                        key: 6,
                        down: false,
                        delay_ms: 35,
                    },
                    Event {
                        key: 224,
                        down: false,
                        delay_ms: 5,
                    },
                ],
            },
        };
        let slots = client.keyboard_macros("macro-assign", &json!(a))?;
        if !slots.iter().any(|s| {
            s.id == a.slot
                && s.data == a.data
                && s.bindings.len() == 1
                && s.bindings[0].key == a.key
        }) {
            return Err("Macro ou affectation non relue.".into());
        }
    }
    if client.keyboard_macros("macros", &json!({}))?.len() != 3 {
        return Err("Macros précédentes perdues.".into());
    }
    if client
        .keyboard_macros(
            "macro-assign",
            &json!({"slot":12,"key":69,"data":{"name":"Invalide","mode":"once","events":[]}}),
        )
        .is_ok()
    {
        return Err("Macro invalide acceptée.".into());
    }
    for id in 0..3 {
        client.keyboard_macros("macro-remove", &json!({"slot":id}))?;
    }
    if !client.keyboard_macros("macros", &json!({}))?.is_empty()
        || client.keyboard_view()?.state != Some(before)
    {
        return Err("Le retrait des macros a changé les autres réglages.".into());
    }
    println!(
        "OK : macros natives, trois répétitions, affectations, relecture, refus et retrait via l’API."
    );
    Ok(())
}
