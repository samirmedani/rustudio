"""Test Windows de l’icône et de la bulle GPUI sans fenêtre principale."""
from pathlib import Path
import socket
import subprocess
import tempfile
import time

from measure import Engine, find_window, foreground, running, until, visible

def main():
    root = Path(__file__).resolve().parent
    app = root / "Rustudio.exe"
    with socket.socket() as guard:
        guard.setsockopt(socket.SOL_SOCKET, socket.SO_EXCLUSIVEADDRUSE, 1)
        guard.bind(("127.0.0.1", 14566))
    engine = Engine()
    def ready():
        try:
            s = engine.call("/api/state")
            if not s["sim"]:
                raise RuntimeError("Le moteur n’est pas celui du banc.")
            return s["connected"]
        except (OSError, ConnectionError):
            return False
    with tempfile.TemporaryDirectory(dir=root) as folder:
        with running(root / "rustudio-engine-bench.exe",
                     ["run", "--child", "--simulate", "--port", "14566", "--data-dir", folder], root):
            until(ready, 8, "Le moteur de démonstration ne répond pas.")
            engine.mute(False)
            previous = foreground()
            with (root / "tray-test.log").open("w+b") as output:
                p = subprocess.Popen([str(app), "--demo", "--tray", "--native-osd", "--seconds", "9"],
                                     cwd=root, stdout=output, stderr=output,
                                     creationflags=subprocess.CREATE_NO_WINDOW)
                try:
                    until(lambda: b"tray_ready" in (root / "tray-test.log").read_bytes(),
                          5, "L’icône Rustudio n’a pas démarré.")
                    if find_window(p.pid, "Zed::Window", by_class=True):
                        raise RuntimeError("Le mode icône a ouvert une fenêtre principale.")
                    time.sleep(.5)
                    engine.mute(True)
                    def bubble():
                        h = find_window(p.pid, "RustudioNativeOsd", by_class=True)
                        return h if h and visible(h) else None
                    hwnd = until(bubble, 3, "Pas de bulle micro en mode icône.")
                    until(lambda: not visible(hwnd), 4, "La bulle reste affichée.")
                    if foreground() != previous:
                        raise RuntimeError("Le mode icône a changé le focus.")
                    if p.wait(timeout=12) != 0:
                        raise RuntimeError("Le mode icône n’a pas quitté correctement.")
                finally:
                    if p.poll() is None:
                        p.terminate()
                        p.wait(timeout=5)
    print("Icône GPUI et bulle vérifiées en démo, sans fenêtre principale ni prise de focus.")

if __name__ == "__main__":
    main()
