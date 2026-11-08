"""Captures de chaque page de Rustudio en démonstration (moteur simulé, sans focus)."""
from pathlib import Path
import sys, tempfile, time
from measure import Engine, find_window, reveal, running, screenshot_window, until

def main():
    root = Path(__file__).resolve().parent
    app = root / "Rustudio.exe"
    pages = sys.argv[1:] or ["devices", "mixer", "mic", "mouse", "profiles", "headphones", "routing", "lighting", "scenes", "diagnostic", "audio", "modules", "versions", "settings", "help"]
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
            until(ready, 10, "Le moteur simulé ne répond pas.")
            time.sleep(1.5)
            for page in pages:
                with running(app, ["--demo", "--hidden", "--page", page, "--profiles-dir", str(Path(folder) / "profiles")], root) as p:
                    hwnd = until(lambda: find_window(p.pid, "Zed::Window", by_class=True), 10, "Fenêtre absente.")
                    reveal(hwnd, 1240, 800)
                    time.sleep(2.2)
                    screenshot_window(hwnd, out / f"{page}.png")
                    print("capture", page)

if __name__ == "__main__":
    main()
