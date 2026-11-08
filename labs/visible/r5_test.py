"""Banc de la R5 Ultra native, avec un moteur et des données de démo."""
from pathlib import Path
import os
import socket
import tempfile
import time

from desktop_test import WIDTH, HEIGHT, accent_buttons, click, type_text
from measure import Engine, P, U, find_window, foreground, reveal, running, screenshot_window, until, visible

def main():
    root = Path(__file__).resolve().parent
    app = root / "Rustudio.exe"
    U.SetProcessDpiAwarenessContext.argtypes = [P]
    U.SetProcessDpiAwarenessContext(P(-4))
    with socket.socket() as guard:
        guard.setsockopt(socket.SOL_SOCKET, socket.SO_EXCLUSIVEADDRUSE, 1)
        guard.bind(("127.0.0.1", 14566))
    engine = Engine()
    def mouse():
        try:
            view = engine.call("/api/modules/attack-shark-r5-ultra/state")
            if not view["device"]["simulated"]:
                raise RuntimeError("Mauvais moteur.")
            return view
        except (OSError, ConnectionError):
            return {}
    def status():
        return mouse().get("device", {}).get("status")

    with tempfile.TemporaryDirectory(dir=root) as folder:
        flag = Path(folder) / "sleep"
        os.environ["RUSTUDIO_SIM_MOUSE_SLEEP_FILE"] = str(flag)
        with running(root / "rustudio-engine-bench.exe",
                     ["run", "--child", "--simulate", "--port", "14566", "--data-dir", folder], root):
            until(lambda: status() == "ready", 8, "La souris simulée ne répond pas.")
            time.sleep(1.1)
            old_focus = foreground()
            with running(app, ["--demo", "--hidden", "--native-osd", "--profiles-dir", str(Path(folder) / "profiles")], root) as p:
                hwnd = until(lambda: find_window(p.pid, "Zed::Window", by_class=True), 10, "Fenêtre GPUI absente.")
                reveal(hwnd, WIDTH, HEIGHT)
                time.sleep(1)
                opens = accent_buttons(hwnd, 400, 700)
                if len(opens) < 2:
                    raise RuntimeError(f"Boutons Ouvrir introuvables : {opens}")
                click(hwnd, *opens[-1])  # R5 Ultra
                time.sleep(.8)
                screenshot_window(hwnd, root / "r5.png")
                baseline = mouse()["state"]
                click(hwnd, 434, 484)  # DPI X, palier 1
                time.sleep(.15)
                type_text(hwnd, "850")
                click(hwnd, 872, 479)  # liste Fréquence (sous le bandeau du brouillon)
                time.sleep(.35)
                click(hwnd, 872, 662)  # 2 000 Hz
                time.sleep(.3)
                if mouse()["state"] != baseline:
                    raise RuntimeError("La saisie du brouillon a envoyé une commande.")
                screenshot_window(hwnd, root / "r5-brouillon.png")
                save = accent_buttons(hwnd, 0, 200)
                if not save:
                    raise RuntimeError("Bouton Enregistrer introuvable.")
                click(hwnd, *save[-1])
                until(lambda: mouse()["state"]["polling_hz"] == 2000
                      and mouse()["state"]["stages"][0]["x"] == 850, 5,
                      f"Le bouton Enregistrer n’a pas appliqué le brouillon : {mouse()['state']['polling_hz']} Hz, {mouse()['state']['stages'][0]}.")
                screenshot_window(hwnd, root / "r5-enregistree.png")
                flag.write_bytes(b"")
                until(lambda: status() == "asleep", 14, "La veille n’est pas détectée.")
                time.sleep(.9)
                screenshot_window(hwnd, root / "r5-veille.png")
                flag.unlink()
                until(lambda: status() == "ready", 7, "Le réveil n’est pas détecté.")
                time.sleep(.9)
                click(hwnd, 100, 127)  # Mes appareils
                time.sleep(.5)
                click(hwnd, *accent_buttons(hwnd, 400, 700)[0])  # GoXLR Mini
                time.sleep(.6)
                click(hwnd, 100, 465)  # Routage
                time.sleep(.9)
                screenshot_window(hwnd, root / "routage.png")
                engine.mute(False)
                time.sleep(.3)
                engine.mute(True)
                def osd():
                    candidate = find_window(p.pid, "RustudioNativeOsd", by_class=True)
                    return candidate if candidate and visible(candidate) else None
                bubble = until(osd, 3, "La bulle micro GPUI n’apparaît pas.")
                until(lambda: not visible(bubble), 4, "La bulle micro ne se masque pas.")
                engine.mute(False)
                until(osd, 3, "La réactivation ne déclenche pas la bulle.")
                until(lambda: not visible(bubble), 4, "La bulle micro ne se masque pas.")
                if foreground() != old_focus:
                    raise RuntimeError("Le test a changé le focus.")
            with running(app, ["--demo", "--self-test"], root) as p:
                if p.wait(timeout=40) != 0:
                    raise RuntimeError("Échec du test natif des réglages R5 Ultra.")
    print("R5 Ultra : lecture, veille, réveil et écritures par lot vérifiés en simulation.")

if __name__ == "__main__":
    main()
