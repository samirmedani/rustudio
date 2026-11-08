"""Captures du survol : cartes, navigation, boutons, faders (démonstration, sans focus)."""
from pathlib import Path
import tempfile, time
from measure import Engine, find_window, get_dpi, post_message, reveal, running, screenshot_window, until

def move(hwnd, x, y):
    s = (get_dpi(hwnd) or 96) / 96
    post_message(hwnd, 0x0200, 0, (round(y * s) << 16) | round(x * s))

def main():
    root = Path(__file__).resolve().parent
    out = root / "shots"
    out.mkdir(exist_ok=True)
    engine = Engine()
    def ready():
        try:
            return engine.call("/api/state")["connected"]
        except (OSError, ConnectionError):
            return False
    with tempfile.TemporaryDirectory(dir=root) as folder:
        with running(root / "rustudio-engine-bench.exe", ["run", "--child", "--simulate", "--port", "14566", "--data-dir", folder], root):
            until(ready, 10, "Moteur simulé absent.")
            time.sleep(1.2)
            for page, spots in (("devices", [("carte", 500, 300), ("nav", 100, 170), ("bouton", 1060, 108)]),
                                ("mixer", [("fader", 395, 360), ("entete", 395, 210), ("couper", 395, 542)]),
                                ("mouse", [("liste", 870, 425), ("interrupteur", 1160, 567)])):
                with running(root / "Rustudio.exe", ["--demo", "--hidden", "--page", page], root) as p:
                    hwnd = until(lambda: find_window(p.pid, "Zed::Window", by_class=True), 10, "Fenêtre absente.")
                    reveal(hwnd, 1240, 800)
                    time.sleep(2)
                    for name, x, y in spots:
                        move(hwnd, x, y)
                        time.sleep(0.06)
                        screenshot_window(hwnd, out / f"hover-{page}-{name}-mi.png")
                        time.sleep(0.35)
                        screenshot_window(hwnd, out / f"hover-{page}-{name}.png")
                        move(hwnd, 5, 790)
                        time.sleep(0.4)
                    screenshot_window(hwnd, out / f"hover-{page}-repos.png")
                    print("ok", page)

if __name__ == "__main__":
    main()
