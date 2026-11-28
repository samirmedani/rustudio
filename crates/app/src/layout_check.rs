//! Matrice de rendu GPUI : fenêtres masquées et données du moteur de démonstration.
use crate::{
    backend::Client,
    desktop::Page,
    goxlr::Section,
    macros::Tab,
    mic_filters::{MicFilter, MicTab},
    mixer::Mixer,
    motion::Destination,
    webcam::WebcamTab,
};
use gpui::{AppContext, AsyncApp, Bounds, Pixels, WindowHandle, px};
use std::{
    cell::RefCell,
    time::{Duration, Instant},
};

#[derive(Default)]
struct Frame {
    content_checked: bool,
    tab_bars: usize,
    errors: Vec<&'static str>,
}

thread_local! { static FRAME: RefCell<Option<Frame>> = const { RefCell::new(None) }; }

pub(crate) fn content(bounds: &[Bounds<Pixels>]) {
    FRAME.with_borrow_mut(|state| {
        let Some(frame) = state else { return };
        let [header, body] = bounds else {
            frame.errors.push("En-tête ou contenu absent");
            return;
        };
        frame.content_checked = true;
        if header.size.width < px(120.) || header.top() + header.size.height > body.top() + px(1.) {
            frame.errors.push("En-tête et contenu se chevauchent");
        }
        if body.left() < header.left() - px(1.) || body.right() > header.right() + px(1.) {
            frame.errors.push("Contenu hors de la colonne centrale");
        }
    });
}

pub(crate) fn tabs(area: Bounds<Pixels>, buttons: &[Bounds<Pixels>]) {
    FRAME.with_borrow_mut(|state| {
        let Some(frame) = state else { return };
        frame.tab_bars += 1;
        if buttons
            .iter()
            .any(|b| b.left() < area.left() - px(1.) || b.right() > area.right() + px(1.))
        {
            frame.errors.push("Onglet hors de la rangée");
        }
        if buttons
            .windows(2)
            .any(|p| p[0].top() == p[1].top() && p[0].right() > p[1].left() + px(1.))
        {
            frame.errors.push("Onglets superposés");
        }
    });
}

fn draw(
    handle: WindowHandle<Mixer>,
    name: &str,
    tabs_expected: bool,
    cx: &mut AsyncApp,
) -> Result<Duration, String> {
    let now = Instant::now();
    FRAME.with_borrow_mut(|f| *f = Some(Frame::default()));
    let result = cx.update_window(handle.into(), |_, window, cx| {
        window.refresh();
        window.draw(cx).clear();
    });
    let frame = FRAME.with_borrow_mut(|f| f.take()).unwrap_or_default();
    result.map_err(|e| e.to_string())?;
    if !frame.errors.is_empty() || !frame.content_checked || (tabs_expected && frame.tab_bars == 0)
    {
        return Err(format!(
            "{name}: vérification du rendu incomplète ({:?}).",
            frame.errors
        ));
    }
    Ok(now.elapsed())
}

pub(crate) async fn run(
    handle: WindowHandle<Mixer>,
    client: Client,
    requested: (f32, f32),
    cx: &mut AsyncApp,
) -> Result<(), String> {
    if client.is_live() {
        return Err("Le rendu de test exige la démonstration.".into());
    }
    #[cfg(windows)]
    {
        let (hwnd, scale) = handle
            .update(cx, |_, window, _| {
                let handle = raw_window_handle::HasWindowHandle::window_handle(window)
                    .map_err(|e| e.to_string())?;
                let raw_window_handle::RawWindowHandle::Win32(hwnd) = handle.as_raw() else {
                    return Err("Fenêtre de test Win32 introuvable.".to_owned());
                };
                Ok((hwnd.hwnd.get(), window.scale_factor()))
            })
            .map_err(|e| e.to_string())??;
        rustudio_windows_desktop::session::resize_hidden_test_window(
            hwnd,
            (requested.0 * scale).round() as i32,
            (requested.1 * scale).round() as i32,
        )?;
    }
    #[cfg(not(windows))]
    handle
        .update(cx, |_, window, _| {
            window.resize(gpui::size(px(requested.0), px(requested.1)));
        })
        .map_err(|e| e.to_string())?;
    let data = cx
        .background_executor()
        .spawn(async move {
            Ok::<_, String>((
                client.snapshot(None)?,
                client.inventory()?,
                client.go_view()?,
                client.mouse_view()?,
                client.keyboard_view()?,
                client.webcam_view()?,
                client.get("/api/modules/windows-mic/state")?,
            ))
        })
        .await?;
    let (mix, inventory, go, mouse, keyboard, webcam, mic) = data;
    handle
        .update(cx, |m, window, cx| {
            m.snapshot = Some(mix);
            m.inventory = Some(inventory);
            m.go.receive(Some(go));
            m.mouse.receive(Some(mouse));
            m.keyboard.receive(Some(keyboard));
            m.webcam.receive(Some(webcam));
            m.application_profiles.load_layout_fixture();
            cx.notify();
            window.refresh();
        })
        .map_err(|e| e.to_string())?;
    let mut size_read = (0., 0.);
    for _ in 0..20 {
        size_read = handle
            .update(cx, |_, window, _| {
                let size = window.viewport_size();
                (f32::from(size.width), f32::from(size.height))
            })
            .map_err(|e| e.to_string())?;
        if (size_read.0 - requested.0).abs() <= 20. && (size_read.1 - requested.1).abs() <= 20. {
            break;
        }
        cx.background_executor()
            .timer(Duration::from_millis(40))
            .await;
    }
    let (width, height) = size_read;
    if (width - requested.0).abs() > 20. || (height - requested.1).abs() > 20. {
        return Err(format!(
            "Taille demandée {:?}, taille réelle {width:.0}×{height:.0}.",
            requested
        ));
    }

    let mut samples = Vec::new();
    for page in [
        Page::Devices,
        Page::Profiles,
        Page::Mixer,
        Page::WinMic,
        Page::WindowsAudio,
        Page::Mouse,
        Page::MouseSensitivity,
        Page::MouseSensor,
        Page::MousePower,
        Page::Keyboard,
        Page::KeyboardSettings,
        Page::KeyboardPaint,
        Page::KeyboardShortcuts,
        Page::KeyboardInfo,
        Page::KeyboardMacros,
        Page::Webcam,
        Page::WebcamInfo,
        Page::Go(Section::Headphones),
        Page::Go(Section::Routing),
        Page::Go(Section::Scenes),
        Page::Go(Section::Light),
        Page::Go(Section::Diagnostic),
    ] {
        handle
            .update(cx, |m, _, cx| {
                m.go_to(page, cx);
                if page == Page::WinMic {
                    m.winmic.load_layout_fixture(mic.clone(), true);
                }
            })
            .map_err(|e| e.to_string())?;
        samples.push(draw(
            handle,
            page.name().unwrap_or("page"),
            matches!(
                page,
                Page::WinMic | Page::Profiles | Page::Webcam | Page::KeyboardMacros
            ),
            cx,
        )?);
    }
    for (page, layouts) in [
        (
            Page::WinMic,
            vec![
                Destination::Microphone(MicTab::Filters),
                Destination::Microphone(MicTab::Presets),
                Destination::Microphone(MicTab::Listen),
            ],
        ),
        (
            Page::Webcam,
            vec![
                Destination::Camera(WebcamTab::Optics),
                Destination::Camera(WebcamTab::Profiles),
            ],
        ),
        (
            Page::KeyboardMacros,
            vec![
                Destination::Macros(Tab::Library),
                Destination::Macros(Tab::Device),
            ],
        ),
        (Page::Profiles, vec![Destination::Profiles(true)]),
    ] {
        handle
            .update(cx, |m, _, cx| {
                m.go_to(page, cx);
                if page == Page::WinMic {
                    m.winmic.load_layout_fixture(mic.clone(), false);
                }
                m.motion.reduced = false;
            })
            .map_err(|e| e.to_string())?;
        for target in layouts {
            handle
                .update(cx, |m, _, cx| m.navigate_to(target, cx))
                .map_err(|e| e.to_string())?;
            samples.push(draw(handle, "Sortie d’onglet", true, cx)?);
            cx.background_executor()
                .timer(Duration::from_millis(110))
                .await;
            let applied = handle
                .update(cx, |m, _, cx| {
                    if target == Destination::Microphone(MicTab::Filters) {
                        m.mic_filters.select(
                            Some(MicFilter::Gate),
                            Instant::now() - Duration::from_millis(200),
                        );
                        cx.notify();
                    }
                    m.motion.target().is_none()
                        && match target {
                            Destination::Microphone(t) => m.mic_tab == t,
                            Destination::Camera(t) => m.webcam.tab == t,
                            Destination::Macros(t) => m.macros.tab == t,
                            Destination::Profiles(t) => m.application_profiles_tab == t,
                            _ => false,
                        }
                })
                .map_err(|e| e.to_string())?;
            if !applied {
                return Err("La destination n’a pas été affichée.".into());
            }
            samples.push(draw(handle, "Entrée d’onglet", true, cx)?);
        }
    }
    samples.sort_unstable();
    let p95 = samples[(samples.len() - 1) * 95 / 100].as_secs_f64() * 1000.;
    println!(
        "LAYOUT_OK {width:.0}x{height:.0} frames={} p95_layout_ms={p95:.2}",
        samples.len()
    );
    Ok(())
}
