//! Panneau près de l'horloge (clic gauche sur l'icône) : état, micro, scènes et raccourcis.
use crate::{
    backend::Client,
    desktop::Page,
    goxlr::Section,
    ui::{self, Icon, alpha, rgb},
};
use gpui::{
    App, Bounds, Context, CursorStyle, Div, ElementId, FontWeight, Render, SharedString, Stateful, Window,
    WindowBounds, WindowKind, WindowOptions, div, point, prelude::*, px, size,
};
use rustudio_windows_desktop::session;
use serde_json::{Value, json};
use std::rc::Rc;

pub type Opener = Rc<dyn Fn(&mut App, Option<Page>)>;

const WIDTH: f32 = 340.;
const HEIGHT: f32 = 400.;
const MODULE: &str = "/api/modules/goxlr-mini";

pub struct TrayPanel {
    client: Client,
    open: Opener,
    live_mode: bool,
    state: Option<Value>,
    scenes: bool,
}

/// Ouvre le panneau, ou le referme s'il est déjà affiché.
pub fn toggle(cx: &mut App, client: Client, live_mode: bool, open: Opener) {
    if let Some(handle) = cx.windows().iter().find_map(|w| w.downcast::<TrayPanel>()) {
        let _ = handle.update(cx, |_, window, _| window.remove_window());
        return;
    }
    let bounds = match cx.primary_display() {
        Some(display) => {
            let b = display.bounds();
            // au-dessus de la barre des tâches, collé au coin de l'horloge
            Bounds::new(
                point(b.origin.x + b.size.width - px(WIDTH + 12.), b.origin.y + b.size.height - px(HEIGHT + 60.)),
                size(px(WIDTH), px(HEIGHT)),
            )
        }
        None => Bounds::centered(None, size(px(WIDTH), px(HEIGHT)), cx),
    };
    let options = WindowOptions {
        titlebar: None,
        kind: WindowKind::PopUp,
        focus: true,
        show: true,
        is_movable: false,
        is_resizable: false,
        is_minimizable: false,
        window_bounds: Some(WindowBounds::Windowed(bounds)),
        ..Default::default()
    };
    let _ = cx.open_window(options, move |window, cx| {
        if let Ok(handle) = raw_window_handle::HasWindowHandle::window_handle(&*window) {
            if let raw_window_handle::RawWindowHandle::Win32(w) = handle.as_raw() {
                session::round_corners(w.hwnd.get());
            }
        }
        cx.new(|cx| {
            // le panneau se referme dès qu'on clique ailleurs
            cx.observe_window_activation(window, |_, window, _| {
                if !window.is_window_active() {
                    window.remove_window();
                }
            })
            .detach();
            let mut panel = TrayPanel { client, open, live_mode, state: None, scenes: false };
            panel.refresh(cx);
            panel
        })
    });
}

impl TrayPanel {
    fn refresh(&mut self, cx: &mut Context<Self>) {
        let client = self.client.clone();
        let job = cx.background_executor().spawn(async move { client.get("/api/state").ok() });
        cx.spawn(async move |this, cx| {
            let state = job.await;
            let _ = this.update(cx, |this, cx| {
                this.state = state;
                cx.notify();
            });
        })
        .detach();
    }

    fn post(&mut self, action: &'static str, body: Value, cx: &mut Context<Self>) {
        let client = self.client.clone();
        let job = cx.background_executor().spawn(async move {
            let _ = client.post_module(MODULE, action, body);
            client.get("/api/state").ok()
        });
        cx.spawn(async move |this, cx| {
            let state = job.await;
            let _ = this.update(cx, |this, cx| {
                this.state = state;
                cx.notify();
            });
        })
        .detach();
    }

    fn go(&self, page: Option<Page>, window: &mut Window, cx: &mut Context<Self>) {
        let open = self.open.clone();
        window.remove_window();
        cx.defer(move |cx| open(cx, page));
    }

    fn connected(&self) -> bool {
        self.state.as_ref().is_some_and(|s| s["connected"] == true)
    }
    fn mic_muted(&self) -> bool {
        self.state.as_ref().is_some_and(|s| s["state"]["mutes"]["Mic"] == true || s["state"]["partial"]["Mic"].is_string())
    }

    fn quick(&self, id: &'static str, glyph: Icon, label: &'static str, on: bool, tone: u32) -> Stateful<Div> {
        let t = ui::hover_t(id);
        let el = div()
            .id(id)
            .relative()
            .top(px(-1.5 * t))
            .flex_1()
            .flex()
            .flex_col()
            .items_center()
            .gap(px(6.))
            .py(px(12.))
            .rounded(px(14.))
            .cursor(CursorStyle::PointingHand)
            .text_size(px(12.5))
            .font_weight(FontWeight::MEDIUM)
            .shadow(vec![gpui::BoxShadow { color: alpha(if on { tone } else { 0x111827 }, if on { 0.18 + 0.14 * t } else { 0.08 * t }), offset: gpui::point(px(0.), px(4. + 4. * t)), blur_radius: px(10. + 8. * t), spread_radius: px(0.) }])
            .when(on, |d| d.bg(ui::lerp(rgb(tone), ui::tint(tone, 0.14), t)).text_color(rgb(ui::WHITE)))
            .when(!on, |d| d.bg(ui::lerp(alpha(ui::WHITE, 0.75), rgb(ui::WHITE), t)).border_1().border_color(ui::lerp(alpha(0x000000, 0.07), alpha(ui::ACCENT, 0.28), t)))
            .child(ui::icon(glyph, 20., if on { rgb(ui::WHITE) } else { rgb(ui::TEXT) }))
            .child(label);
        ui::hoverable(el, id.to_owned())
    }

    fn row(id: impl Into<ElementId>, glyph: Icon, label: impl Into<SharedString>) -> Stateful<Div> {
        let id: ElementId = id.into();
        let key = id.to_string();
        let t = ui::hover_t(&key);
        let el = div()
            .id(id)
            .flex()
            .items_center()
            .gap(px(12.))
            .h(px(40.))
            .px(px(10.))
            .rounded(px(11.))
            .text_size(px(14.))
            .font_weight(FontWeight::MEDIUM)
            .cursor(CursorStyle::PointingHand)
            .bg(ui::lerp(alpha(ui::ACCENT, 0.), alpha(ui::ACCENT, 0.08), t))
            .child(ui::icon(glyph, 18., ui::lerp(rgb(ui::TEXT2), rgb(ui::ACCENT), t)))
            .child(label.into());
        ui::hoverable(el, key)
    }

    fn scene_list(&self, cx: &mut Context<Self>) -> Div {
        let mut list = div().flex().flex_col().gap(px(2.)).child(
            Self::row("back", Icon::ChevronLeft, "Scènes").on_click(cx.listener(|this, _, _, cx| {
                this.scenes = false;
                cx.notify();
            })),
        );
        if let Some(s) = &self.state {
            let active = s["state"]["scene"].as_str().unwrap_or("");
            let mut names: Vec<(String, String)> = s["meta"]["scenes"]
                .as_object()
                .into_iter()
                .flatten()
                .map(|(k, v)| (k.clone(), v["label"].as_str().unwrap_or(k).to_owned()))
                .collect();
            names.extend(s["state"]["user_scenes"].as_object().into_iter().flatten().map(|(k, _)| (k.clone(), k.clone())));
            let mut scroll = div().id("scenes").flex().flex_col().max_h(px(250.)).overflow_y_scroll();
            for (i, (key, label)) in names.into_iter().enumerate() {
                scroll = scroll.child(ui::menu_item(("scene", i), None, label, key == active).h(px(38.)).on_click(cx.listener(move |this, _, _, cx| {
                    this.post("scene", json!({"name": key.clone()}), cx);
                })));
            }
            list = list.child(scroll);
        }
        list
    }
}

impl Render for TrayPanel {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let view = self.view(cx);
        ui::keep_animating(window);
        view
    }
}

impl TrayPanel {
    fn view(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let connected = self.connected();
        let muted = self.mic_muted();
        let status = match &self.state {
            None => ("Lecture…", ui::State::Idle),
            Some(_) if connected => ("GoXLR Mini connectée", ui::State::Ok),
            Some(_) => ("GoXLR Mini débranchée", ui::State::Idle),
        };
        let active_scene = self.state.as_ref().and_then(|s| s["state"]["scene"].as_str()).filter(|s| !s.is_empty()).unwrap_or("").to_owned();
        let mut body = div().flex().flex_col().gap(px(2.));
        if self.scenes {
            body = body.child(self.scene_list(cx));
        } else {
            body = body
                .child(Self::row("open", Icon::Plug, "Ouvrir Rustudio").on_click(cx.listener(|this, _, window, cx| this.go(None, window, cx))))
                .when(connected, |d| {
                    d.child(
                        Self::row("scenes", Icon::Layers, "Scènes")
                            .child(div().flex_1().flex().justify_end().flex().items_center().gap(px(4.)).text_size(px(13.)).text_color(rgb(ui::TEXT2)).child(active_scene.clone()).child(ui::icon(Icon::ChevronRight, 15., rgb(ui::TEXT3))))
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.scenes = true;
                                cx.notify();
                            })),
                    )
                    .child(Self::row("reconnect", Icon::Refresh, "Reconnecter la GoXLR").on_click(cx.listener(|this, _, _, cx| this.post("reconnect", json!({}), cx))))
                })
                .child(div().h(px(1.)).mx(px(4.)).my(px(5.)).bg(alpha(0x000000, 0.07)))
                .when(self.live_mode, |d| {
                    let on = session::startup_enabled();
                    d.child(Self::row("startup", Icon::Power, "Démarrer avec Windows").child(div().flex_1().flex().justify_end().child(ui::toggle("startup-toggle", on, true))).on_click(cx.listener(move |_, _, _, cx| {
                        let _ = session::set_startup(!on);
                        cx.notify();
                    })))
                })
                .child(Self::row("quit", Icon::Close, "Quitter Rustudio").on_click(cx.listener(|_, _, _, cx| cx.quit())));
        }
        div()
            .size_full()
            .font_family(ui::FONT)
            .text_color(rgb(ui::TEXT))
            .bg(rgb(0xf7f7f9))
            .child(
                div()
                    .size_full()
                    .flex()
                    .flex_col()
                    .gap(px(10.))
                    .p(px(16.))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(11.))
                            .px(px(4.))
                            .child(ui::logo(36.))
                            .child(div().flex().flex_col().child(div().text_size(px(15.)).font_weight(FontWeight::SEMIBOLD).child("Rustudio")).child(ui::status(status.0, status.1).text_color(rgb(ui::TEXT2)).font_weight(FontWeight::NORMAL).text_size(px(12.5)))),
                    )
                    .when(connected && !self.scenes, |d| {
                        d.child(
                            div()
                                .flex()
                                .gap(px(8.))
                                .child(self.quick("q-mic", if muted { Icon::MicOff } else { Icon::Mic }, if muted { "Micro coupé" } else { "Micro" }, true, if muted { ui::RED } else { ui::ACCENT }).on_click(cx.listener(move |this, _, _, cx| {
                                    this.post("mute", json!({"channel": "Mic", "muted": !muted, "target": "all"}), cx);
                                })))
                                .child(self.quick("q-scenes", Icon::Layers, "Scène", false, ui::ACCENT).on_click(cx.listener(|this, _, _, cx| {
                                    this.scenes = true;
                                    cx.notify();
                                })))
                                .child(self.quick("q-mixer", Icon::Sliders, "Mixeur", false, ui::ACCENT).on_click(cx.listener(|this, _, window, cx| this.go(Some(Page::Mixer), window, cx)))),
                        )
                    })
                    .child(body)
                    .when(connected && !self.scenes, |d| {
                        d.child(div().mt_auto().flex().justify_center().child(ui::hoverable(
                            div()
                                .id("diag")
                                .text_size(px(12.))
                                .text_color(rgb(ui::TEXT3))
                                .cursor(CursorStyle::PointingHand)
                                .text_color(ui::lerp(rgb(ui::TEXT3), rgb(ui::ACCENT), ui::hover_t("diag")))
                                .child("Diagnostic de la GoXLR")
                                .on_click(cx.listener(|this, _, window, cx| this.go(Some(Page::Go(Section::Diagnostic)), window, cx))),
                            "diag".to_owned(),
                        )))
                    }),
            )
    }
}
