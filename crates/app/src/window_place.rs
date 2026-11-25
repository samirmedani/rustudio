//! Place de la fenêtre (écran, position, taille, agrandie ou non), gardée d'une ouverture à l'autre : après une
//! mise à jour ou une relance en développement, Rustudio revient exactement où tu l'avais laissé.
use gpui::{App, Bounds, DisplayId, Pixels, WindowBounds, point, px, size};
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
struct Place {
    x: f32,
    y: f32,
    width: f32,
    height: f32,
    maximized: bool,
}

fn path() -> Option<std::path::PathBuf> {
    Some(std::path::PathBuf::from(std::env::var_os("LOCALAPPDATA")?).join("Rustudio").join("fenetre.json"))
}

/// Enregistre la place actuelle (appelée à chaque déplacement ou redimensionnement).
pub fn save(bounds: WindowBounds) {
    let (b, maximized) = match bounds {
        WindowBounds::Windowed(b) => (b, false),
        WindowBounds::Maximized(b) | WindowBounds::Fullscreen(b) => (b, true),
    };
    let place = Place { x: b.origin.x.into(), y: b.origin.y.into(), width: b.size.width.into(), height: b.size.height.into(), maximized };
    if place.width < 200. || place.height < 200. {
        return;
    }
    if let (Some(p), Ok(text)) = (path(), serde_json::to_string(&place)) {
        let _ = std::fs::create_dir_all(p.parent().unwrap_or(&p));
        let _ = std::fs::write(p, text);
    }
}

/// Place enregistrée et l'écran qui la contient ; None si aucune, ou si cet écran n'est plus branché.
pub fn restore(cx: &App) -> Option<(WindowBounds, DisplayId)> {
    let place: Place = serde_json::from_str(&std::fs::read_to_string(path()?).ok()?).ok()?;
    let bounds: Bounds<Pixels> = Bounds { origin: point(px(place.x), px(place.y)), size: size(px(place.width), px(place.height)) };
    let center = bounds.center();
    let display = cx.displays().into_iter().find(|d| d.bounds().contains(&center))?;
    let bounds = if place.maximized { WindowBounds::Maximized(bounds) } else { WindowBounds::Windowed(bounds) };
    Some((bounds, display.id()))
}
