//! Transitions des pages et des onglets, avec les courbes de Wispad.
use crate::{
    desktop::Page, macros::Tab as MacroTab, mic_filters::MicTab, mixer::Mixer, webcam::WebcamTab,
};
use gpui::{AnyElement, Bounds, Context, Div, Pixels, Window, canvas, prelude::*, px};
use std::time::{Duration, Instant};
use std::{cell::RefCell, rc::Rc};

const OUT: Duration = Duration::from_millis(85);
const IN: Duration = Duration::from_millis(140);
pub(crate) const INDICATOR: Duration = Duration::from_millis(280);

fn bezier(t: f32, x1: f32, x2: f32) -> f32 {
    let axis = |u: f32, p1: f32, p2: f32| {
        let v = 1. - u;
        3. * v * v * u * p1 + 3. * v * u * u * p2 + u * u * u
    };
    if t <= 0. || t >= 1. {
        return t.clamp(0., 1.);
    }
    let (mut low, mut high) = (0., 1.);
    for _ in 0..12 {
        let mid = (low + high) * 0.5;
        if axis(mid, x1, x2) < t {
            low = mid;
        } else {
            high = mid;
        }
    }
    axis((low + high) * 0.5, 1., 1.)
}

pub(crate) fn indicator(t: f32) -> f32 {
    bezier(t, 0.25, 0.4)
}
fn ease(t: f32) -> f32 {
    bezier(t, 0.22, 0.36)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Surface {
    Page,
    Microphone,
    Camera,
    Macros,
    Profiles,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Destination {
    Page(Page),
    Back,
    Microphone(MicTab),
    Camera(WebcamTab),
    Macros(MacroTab),
    Profiles(bool),
}

struct Leaving {
    target: Destination,
    surface: Surface,
    start: Instant,
    from: f32,
}

impl Destination {
    fn surface(self) -> Surface {
        match self {
            Self::Page(_) | Self::Back => Surface::Page,
            Self::Microphone(_) => Surface::Microphone,
            Self::Camera(_) => Surface::Camera,
            Self::Macros(_) => Surface::Macros,
            Self::Profiles(_) => Surface::Profiles,
        }
    }
}

#[derive(Default)]
pub(crate) struct Motion {
    pub(crate) reduced: bool,
    leaving: Option<Leaving>,
    entering: Option<(Surface, Instant)>,
    window: Option<(bool, Instant)>,
    tabs: [Rc<RefCell<Underline>>; 5],
}

impl Motion {
    pub(crate) fn new() -> Self {
        Self {
            reduced: rustudio_windows_desktop::reduced_motion(),
            ..Self::default()
        }
    }

    /// Pendant la sortie, un nouveau clic remplace la destination et garde l'échéance.
    fn request(&mut self, target: Destination, now: Instant) -> bool {
        if let Some(leaving) = &mut self.leaving {
            leaving.target = target;
            return false;
        }
        let surface = target.surface();
        let from = self.opacity(surface, now).0;
        self.entering = None;
        self.leaving = Some(Leaving {
            target,
            surface,
            start: now,
            from,
        });
        true
    }

    pub(crate) fn target(&self) -> Option<Destination> {
        self.leaving.as_ref().map(|step| step.target)
    }

    pub(crate) fn cancel_pending(&mut self) {
        self.leaving = None;
    }

    pub(crate) fn tabs(&self, surface: Surface, selected: usize, tabs: Vec<AnyElement>) -> Div {
        let state = self.tabs[surface as usize].clone();
        let marker = state.clone();
        let reduced = self.reduced;
        crate::ui::tabs()
            .relative()
            .children(tabs)
            .child(
                canvas(
                    |_, _, _| (),
                    move |bounds, _, window, _| {
                        let Some((rect, moving)) = marker.borrow().frame(Instant::now()) else {
                            return;
                        };
                        if moving {
                            crate::ui::animation_frame(window);
                        }
                        window.paint_quad(gpui::fill(
                            Bounds::new(bounds.origin + rect.origin, rect.size),
                            crate::ui::rgb(crate::ui::ACCENT),
                        ));
                    },
                )
                .absolute()
                .inset_0(),
            )
            .on_children_prepainted(move |children, _, _| {
                let (Some(button), Some(area)) = (children.get(selected), children.last()) else {
                    return;
                };
                #[cfg(debug_assertions)]
                crate::layout_check::tabs(*area, &children[..children.len() - 1]);
                let rect = Bounds::new(
                    gpui::point(
                        button.left() - area.left(),
                        button.bottom() - area.top() - px(2.),
                    ),
                    gpui::size(button.size.width, px(2.)),
                );
                state
                    .borrow_mut()
                    .retarget(selected, rect, reduced, Instant::now());
            })
    }

    fn arrived(&mut self, start: Instant, now: Instant) -> Option<Destination> {
        if self.leaving.as_ref()?.start != start {
            return None;
        }
        let target = self.leaving.take()?.target;
        self.entering = Some((target.surface(), now));
        Some(target)
    }

    fn opacity(&self, surface: Surface, now: Instant) -> (f32, bool) {
        if let Some(leaving) = self.leaving.as_ref()
            && leaving.surface == surface
        {
            let t = now.saturating_duration_since(leaving.start).as_secs_f32() / OUT.as_secs_f32();
            return (0.34 + (leaving.from - 0.34) * ease(1. - t), t < 1.);
        }
        if let Some((s, start)) = self.entering
            && s == surface
        {
            let t = now.saturating_duration_since(start).as_secs_f32() / IN.as_secs_f32();
            return (0.34 + 0.66 * ease(t), t < 1.);
        }
        (1., false)
    }

    pub(crate) fn window_opened(&mut self, now: Instant) {
        self.window = (!self.reduced).then_some((true, now));
    }

    pub(crate) fn window_opacity(&self, window: &Window) -> f32 {
        let Some((opening, start)) = self.window else {
            return 1.;
        };
        let duration = if opening { IN } else { OUT };
        let t = start.elapsed().as_secs_f32() / duration.as_secs_f32();
        if t < 1. {
            crate::ui::animation_frame(window);
        }
        if opening {
            0.42 + 0.58 * ease(t)
        } else {
            1. - 0.58 * (1. - ease(1. - t))
        }
    }
}

#[derive(Default)]
struct Underline {
    target: Option<(usize, Bounds<Pixels>)>,
    slide: Option<(Bounds<Pixels>, Instant)>,
}

impl Underline {
    fn retarget(&mut self, index: usize, to: Bounds<Pixels>, reduced: bool, at: Instant) {
        if self.target == Some((index, to)) {
            return;
        }
        let from = self.frame(at).map(|(bounds, _)| bounds);
        let changed = self.target.is_some_and(|(old, _)| old != index);
        self.target = Some((index, to));
        self.slide = from
            .filter(|rect| changed && !reduced && rect.origin.y == to.origin.y)
            .map(|rect| (rect, at));
    }

    fn frame(&self, now: Instant) -> Option<(Bounds<Pixels>, bool)> {
        let (_, to) = self.target?;
        let Some((from, start)) = self.slide else {
            return Some((to, false));
        };
        let elapsed = now.saturating_duration_since(start);
        if elapsed >= INDICATOR {
            return Some((to, false));
        }
        let progress = indicator(elapsed.as_secs_f32() / INDICATOR.as_secs_f32());
        let x = from.left() + (to.left() - from.left()) * progress;
        let width = from.size.width + (to.size.width - from.size.width) * progress;
        Some((
            Bounds::new(gpui::point(x, to.top()), gpui::size(width, to.size.height)),
            true,
        ))
    }
}

impl Mixer {
    pub(crate) fn navigate(&mut self, page: Page, cx: &mut Context<Self>) {
        self.navigate_to(Destination::Page(page), cx);
    }

    pub(crate) fn navigate_to(&mut self, target: Destination, cx: &mut Context<Self>) {
        let current = match target {
            Destination::Page(page) => {
                page.destination() == self.page
                    && page != Page::Go(crate::goxlr::Section::Microphone)
            }
            Destination::Back => self.history.is_empty(),
            Destination::Microphone(tab) => tab == self.mic_tab,
            Destination::Camera(tab) => tab == self.webcam.tab,
            Destination::Macros(tab) => tab == self.macros.tab,
            Destination::Profiles(apps) => apps == self.application_profiles_tab,
        };
        if current && self.motion.leaving.is_none() {
            return;
        }
        if self.motion.reduced {
            self.apply_destination(target, cx);
            return;
        }
        let start = Instant::now();
        if self.motion.request(target, start) {
            cx.spawn(async move |this, cx| {
                cx.background_executor().timer(OUT).await;
                let _ = this.update(cx, |this, cx| {
                    if let Some(target) = this.motion.arrived(start, Instant::now()) {
                        this.apply_destination(target, cx);
                        cx.notify();
                    }
                });
            })
            .detach();
        }
        cx.notify();
    }

    fn apply_destination(&mut self, target: Destination, cx: &mut Context<Self>) {
        self.select_open = None;
        self.param_drag = None;
        match target {
            Destination::Page(page) => self.go_to(page, cx),
            Destination::Back => self.go_back(cx),
            Destination::Microphone(tab) => self.mic_tab = tab,
            Destination::Camera(tab) => self.webcam.tab = tab,
            Destination::Macros(tab) => self.macro_tab(tab, cx),
            Destination::Profiles(apps) => {
                self.application_profiles_tab = apps;
                self.reset_page_scroll();
            }
        }
        cx.notify();
    }

    pub(crate) fn surface_opacity(&self, surface: Surface, window: &Window) -> f32 {
        let (value, moving) = self.motion.opacity(surface, Instant::now());
        if moving {
            crate::ui::animation_frame(window);
        }
        value
    }

    pub(crate) fn minimize_gently(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.motion.reduced {
            window.minimize_window();
            return;
        }
        if matches!(self.motion.window, Some((false, _))) {
            return;
        }
        let start = Instant::now();
        self.motion.window = Some((false, start));
        let Some(handle) = window.window_handle().downcast::<Mixer>() else {
            return;
        };
        cx.spawn(async move |_, cx| {
            cx.background_executor().timer(OUT).await;
            let _ = handle.update(cx, |this, window, cx| {
                if this.motion.window == Some((false, start)) {
                    this.motion.window = None;
                    window.minimize_window();
                    cx.notify();
                }
            });
        })
        .detach();
        cx.notify();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn les_courbes_de_wispad_atteignent_les_bornes_sans_rebond() {
        for curve in [ease, indicator] {
            let values = (0..=200)
                .map(|n| curve(n as f32 / 200.))
                .collect::<Vec<_>>();
            assert_eq!((values[0], values[200]), (0., 1.));
            assert!(values.windows(2).all(|w| w[0] <= w[1]));
            assert!(curve(0.5) > 0.85);
        }
    }

    #[test]
    fn les_clics_rapides_gardent_le_dernier_onglet_sans_relancer_la_sortie() {
        let now = Instant::now();
        let mut motion = Motion::default();
        assert!(motion.request(Destination::Microphone(MicTab::Filters), now));
        let halfway = now + OUT / 2;
        assert!(!motion.request(Destination::Microphone(MicTab::Listen), halfway));
        assert!(motion.opacity(Surface::Microphone, halfway).0 < 1.);
        assert_eq!(motion.opacity(Surface::Camera, halfway), (1., false));
        assert!(motion.arrived(now, now + OUT) == Some(Destination::Microphone(MicTab::Listen)));
        assert!((motion.opacity(Surface::Microphone, now + OUT).0 - 0.34).abs() < 1e-6);
        assert_eq!(
            motion.opacity(Surface::Microphone, now + OUT + IN),
            (1., false)
        );
        assert!(motion.arrived(now, now + OUT + IN).is_none());
    }

    #[test]
    fn un_nouveau_clic_pendant_l_entree_repart_de_l_opacite_affichee() {
        let now = Instant::now();
        let mut motion = Motion::default();
        motion.request(Destination::Microphone(MicTab::Filters), now);
        motion.arrived(now, now + OUT).unwrap();
        let opening = now + OUT + IN / 4;
        let value = motion.opacity(Surface::Microphone, opening).0;
        motion.request(Destination::Microphone(MicTab::Input), opening);
        assert!((motion.opacity(Surface::Microphone, opening).0 - value).abs() < 1e-6);
        assert!(motion.arrived(now, opening + OUT).is_none());
    }

    #[test]
    fn le_trait_glisse_sur_une_ligne_et_se_replace_au_retour_a_la_ligne() {
        let now = Instant::now();
        let at = |x, y, w| Bounds::new(gpui::point(px(x), px(y)), gpui::size(px(w), px(2.)));
        let mut line = Underline::default();
        line.retarget(0, at(0., 40., 90.), false, now);
        line.retarget(1, at(120., 40., 100.), false, now);
        let halfway = now + INDICATOR / 2;
        let (between, moving) = line.frame(halfway).unwrap();
        assert!(moving && between.left() > px(0.) && between.left() < px(120.));
        line.retarget(0, at(0., 40., 90.), false, halfway);
        assert_eq!(line.frame(halfway), Some((between, true)));
        line.retarget(2, at(0., 82., 90.), false, halfway);
        assert_eq!(line.frame(halfway), Some((at(0., 82., 90.), false)));
        line.retarget(3, at(125., 82., 90.), true, halfway);
        assert_eq!(line.frame(halfway), Some((at(125., 82., 90.), false)));
    }
}
