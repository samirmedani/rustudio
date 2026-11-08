"""Capture du panneau près de l'horloge et de la bulle micro (démonstration)."""
from pathlib import Path
import subprocess, tempfile, time
from measure import Engine, find_window, running, screenshot_window, until, visible

def main():
    root = Path(__file__).resolve().parent
    engine = Engine()
    def ready():
        try:
            return engine.call("/api/state")["connected"]
        except (OSError, ConnectionError):
            return False
    with tempfile.TemporaryDirectory(dir=root) as folder:
        with running(root / "rustudio-engine-bench.exe", ["run", "--child", "--simulate", "--port", "14566", "--data-dir", folder], root):
            until(ready, 10, "Moteur simulé absent.")
            engine.mute(False)
            p = subprocess.Popen([str(root / "Rustudio.exe"), "--demo", "--tray", "--native-osd", "--panel", "--seconds", "14"], cwd=root,
                                 creationflags=subprocess.CREATE_NO_WINDOW)
            try:
                panel = until(lambda: find_window(p.pid, "Zed::Window", by_class=True), 8, "Panneau absent.")
                time.sleep(2.5)
                screenshot_window(panel, root / "shots" / "panel.png")
                print("panneau ok")
                engine.mute(True)
                def bubble():
                    h = find_window(p.pid, "RustudioNativeOsd", by_class=True)
                    return h if h and visible(h) else None
                h = until(bubble, 4, "Bulle absente.")
                time.sleep(0.25)
                screenshot_window(h, root / "shots" / "bubble.png")
                print("bulle ok")
            finally:
                p.terminate()
                p.wait(5)

if __name__ == "__main__":
    main()
