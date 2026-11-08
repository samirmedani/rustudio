"""Vérifie l’accueil, le mixeur et les profils GPUI dans une fenêtre Windows sans prendre le focus."""
from pathlib import Path
import ctypes as c
import json
import socket
import tempfile
import time

from measure import (Engine, P, POINT, RECT, U, capture_region, client_rect, client_to_screen, find_window,
                     foreground, get_dpi, post_message, reveal, running, screenshot_window, until)

WIDTH, HEIGHT = 1240, 800

def click(hwnd, x, y):
    scale = (get_dpi(hwnd) or 96) / 96
    packed = (round(y * scale) << 16) | round(x * scale)
    if not (post_message(hwnd, 0x0201, 1, packed) and post_message(hwnd, 0x0202, 0, packed)):
        raise c.WinError(c.get_last_error())

def type_text(hwnd, text):
    for ch in text.upper():
        # chiffres par le pavé numérique : identiques en AZERTY et en QWERTY
        key = 0x60 + int(ch) if ch.isdigit() else ord(ch)
        packed = 1 | (U.MapVirtualKeyW(key, 0) << 16)
        if not (post_message(hwnd, 0x0100, key, packed) and post_message(hwnd, 0x0101, key, packed | 0xC0000000)):
            raise c.WinError(c.get_last_error())

def grab(hwnd):
    """Pixels BGRA de la zone cliente, sa largeur, sa hauteur et l’échelle d’affichage."""
    rect, origin = RECT(), POINT()
    if not (client_rect(hwnd, c.byref(rect)) and client_to_screen(hwnd, c.byref(origin))):
        raise c.WinError(c.get_last_error())
    with capture_region(hwnd, origin.x, origin.y, rect.right, rect.bottom) as cap:
        raw = until(lambda: b if any((b := bytes(cap.grab()))[3::4]) else None, 2, "Aucune image de la fenêtre.")
    return raw, rect.right, rect.bottom, (get_dpi(hwnd) or 96) / 96

def accent_buttons(hwnd, y_min=0, y_max=10_000):
    """Boutons pleins bleu Rustudio (#0071E3) : centres en points logiques, triés de haut en bas."""
    raw, w, h, scale = grab(hwnd)
    step = 2
    cells = set()
    for y in range(0, h, step):
        if not (y_min * scale <= y <= y_max * scale):
            continue
        row = y * w * 4
        for x in range(0, w, step):
            i = row + x * 4
            if abs(raw[i] - 227) < 8 and abs(raw[i + 1] - 113) < 8 and raw[i + 2] < 10:
                cells.add((x, y))
    seen, found = set(), []
    for cell in cells:
        if cell in seen:
            continue
        stack, group = [cell], []
        seen.add(cell)
        while stack:
            x, y = stack.pop()
            group.append((x, y))
            for nx, ny in ((x + step, y), (x - step, y), (x, y + step), (x, y - step)):
                if (nx, ny) in cells and (nx, ny) not in seen:
                    seen.add((nx, ny))
                    stack.append((nx, ny))
        xs, ys = [p[0] for p in group], [p[1] for p in group]
        if max(xs) - min(xs) >= 40 * scale and max(ys) - min(ys) >= 20 * scale:
            found.append(((min(xs) + max(xs)) / 2 / scale, (min(ys) + max(ys)) / 2 / scale))
    return sorted(found, key=lambda p: (round(p[1] / 10), p[0]))

def fill_top(hwnd):
    """Haut du remplissage rouge du premier fader (voie Micro), en points logiques."""
    raw, w, h, scale = grab(hwnd)
    top = None
    for y in range(int(238 * scale), min(h, int(470 * scale))):
        row = y * w * 4
        for x in range(int(300 * scale), min(w, int(520 * scale)), 2):
            i = row + x * 4
            b, g, r = raw[i], raw[i + 1], raw[i + 2]
            if r > 200 and g < 130 and b < 130:
                top = y
                break
        if top is not None:
            break
    return None if top is None else top / scale

def main():
    root = Path(__file__).resolve().parent
    app = root / "Rustudio.exe"
    U.SetProcessDpiAwarenessContext.argtypes = [P]
    U.SetProcessDpiAwarenessContext(P(-4))
    with socket.socket() as guard:
        guard.setsockopt(socket.SOL_SOCKET, socket.SO_EXCLUSIVEADDRUSE, 1)
        guard.bind(("127.0.0.1", 14566))
    engine = Engine()
    def ready():
        try:
            state = engine.call("/api/state")
            if not state["sim"]:
                raise RuntimeError("Mauvais moteur.")
            return state["connected"]
        except (OSError, ConnectionError):
            return False
    with tempfile.TemporaryDirectory(dir=root) as folder:
        with running(root / "rustudio-engine-bench.exe",
                     ["run","--child","--simulate","--port","14566","--data-dir",folder],root):
            until(ready, 8, "Le moteur simulé ne répond pas.")
            time.sleep(1)
            with running(app, ["--demo", "--self-test"], root) as p:
                if p.wait(timeout=45) != 0:
                    raise RuntimeError("Échec du test natif des profils.")
            old_focus = foreground()
            with running(app, ["--demo", "--hidden", "--profiles-dir", str(Path(folder) / "profiles")], root) as p:
                hwnd = until(lambda: find_window(p.pid, "Zed::Window", by_class=True), 10, "Fenêtre GPUI absente.")
                reveal(hwnd, WIDTH, HEIGHT)
                time.sleep(1.2)
                screenshot_window(hwnd, root / "devices.png")
                def status():
                    return next(x["status"] for x in engine.call("/api/modules")["devices"] if x["id"] == "goxlr-mini")
                engine.call("/api/sim/unplug", {})
                until(lambda: status() == "disconnected", 6, "La déconnexion n’est pas détectée.")
                time.sleep(.9)
                screenshot_window(hwnd, root / "disconnected.png")
                if (root / "devices.png").read_bytes() == (root / "disconnected.png").read_bytes():
                    raise RuntimeError("L’accueil n’a pas changé après déconnexion.")
                engine.call("/api/sim/plug", {})
                until(lambda: status() == "ready", 6, "La reconnexion n’est pas détectée.")
                engine.set_and_wait(0, 55)
                time.sleep(.6)
                opens = [b for b in accent_buttons(hwnd, 400, 700)]
                if len(opens) < 2:
                    raise RuntimeError(f"Boutons Ouvrir introuvables : {opens}")
                click(hwnd, *opens[0])
                low = until(lambda: fill_top(hwnd), 4, "Le mixeur ne s’ouvre pas depuis Mes appareils.")
                engine.set_and_wait(0, 205)
                until(lambda: (pos := fill_top(hwnd)) is not None and pos < low - 85, 3, "Le mixeur ne suit pas le mouvement.")
                screenshot_window(hwnd, root / "mixer.png")
                click(hwnd, 100, 170)  # Profils
                time.sleep(.5)
                click(hwnd, 420, 230)  # champ du nom
                time.sleep(.15)
                type_text(hwnd, "demo")
                time.sleep(.15)
                save = [b for b in accent_buttons(hwnd, 200, 260)]
                if not save:
                    raise RuntimeError("Bouton Enregistrer introuvable.")
                click(hwnd, *save[0])
                file = Path(folder) / "profiles" / "profil-01.json"
                until(file.is_file, 4, "Le bouton Enregistrer n’a pas écrit le profil.")
                saved = json.loads(file.read_text(encoding="utf-8"))
                assert set(saved) == {"format", "module", "name", "mix"}
                assert set(saved["mix"]) == {"volumes", "faders", "cuts", "mute_modes"}
                if saved["name"].casefold() != "demo":
                    raise RuntimeError(f"Le champ de nom du profil n’a pas reçu la saisie : {saved['name']!r}.")
                time.sleep(.5)
                screenshot_window(hwnd, root / "profiles.png")
                channel = saved["mix"]["faders"]["A"]
                target = saved["mix"]["volumes"][channel]
                value = 1 if target == 0 else 0
                engine.call("/api/modules/goxlr-mini/volume", {"channel": channel, "value": value})
                until(lambda: engine.fader(0)[1] == value, 3, "Le volume de test n’a pas changé.")
                apply = [b for b in accent_buttons(hwnd, 270)]
                if not apply:
                    raise RuntimeError("Bouton Appliquer introuvable.")
                click(hwnd, *apply[0])
                time.sleep(.5)
                screenshot_window(hwnd, root / "confirm.png")
                confirm = [b for b in accent_buttons(hwnd, apply[0][1] + 20)]
                if not confirm:
                    raise RuntimeError("Bouton Confirmer introuvable.")
                click(hwnd, *confirm[0])
                until(lambda: engine.fader(0)[1] == target, 4, "La confirmation n’a pas restauré le volume.")
                if foreground() != old_focus:
                    now = foreground()
                    buf = c.create_unicode_buffer(256)
                    U.GetWindowTextW(now, buf, 256)
                    pid = c.c_ulong()
                    U.GetWindowThreadProcessId(now, c.byref(pid))
                    raise RuntimeError(f"Le test a changé le focus : « {buf.value} » (pid {pid.value}, Rustudio {p.pid}).")
            print("Fenêtre GPUI, mixeur et profils natifs vérifiés.")

if __name__ == "__main__":
    main()
