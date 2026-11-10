"""Test Windows du raccourci clavier du micro, en démonstration (moteur simulé, port 14566, sans focus).

La combinaison d'essai (Ctrl + Alt + Maj + F24) n'existe sur aucun clavier courant : elle ne gêne aucune appli.
Elle est envoyée à Windows comme une vraie frappe, puis le micro simulé doit changer d'état à chaque appui.
"""
import ctypes
from pathlib import Path
import tempfile
import time

from measure import Engine, find_window, foreground, running, until

KEYS = (0x11, 0x12, 0x10, 0x87)  # Ctrl, Alt, Maj, F24


def press():
    user32 = ctypes.WinDLL("user32")
    for key in KEYS:
        user32.keybd_event(key, 0, 0, 0)
    for key in reversed(KEYS):
        user32.keybd_event(key, 0, 2, 0)


def main():
    root = Path(__file__).resolve().parent
    engine = Engine()

    def ready():
        try:
            state = engine.call("/api/state")
            if not state["sim"]:
                raise RuntimeError("Le moteur n’est pas celui du banc.")
            return state["connected"]
        except (OSError, ConnectionError):
            return False

    def muted():
        return engine.call("/api/live")["mic"]["muted"]

    with tempfile.TemporaryDirectory(dir=root) as folder:
        with running(root / "rustudio-engine-bench.exe",
                     ["run", "--child", "--simulate", "--port", "14566", "--data-dir", folder], root):
            until(ready, 10, "Le moteur de démonstration ne répond pas.")
            engine.mute(False)
            before = foreground()
            args = ["--demo", "--hidden", "--hotkey", "ctrl+alt+shift+f24", "--seconds", "20",
                    "--profiles-dir", str(Path(folder) / "profiles")]
            with running(root / "Rustudio.exe", args, root) as app:
                until(lambda: find_window(app.pid, "Zed::Window", by_class=True), 10, "Fenêtre absente.")
                time.sleep(1.0)
                for expected in (True, False, True, False):
                    press()
                    until(lambda: muted() is expected, 3,
                          f"Le raccourci n’a pas {'coupé' if expected else 'rétabli'} le micro.", 0.05)
                if foreground() != before:
                    raise RuntimeError("Le test a changé la fenêtre active.")
            # Appli fermée : le raccourci est rendu à Windows et ne touche plus au micro.
            press()
            time.sleep(0.6)
            if muted():
                raise RuntimeError("Le raccourci agit encore après la fermeture de l’appli.")
    print("OK : quatre appuis, quatre bascules du micro, raccourci libéré à la fermeture, focus inchangé.")


if __name__ == "__main__":
    main()
