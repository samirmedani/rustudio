"""Captures de chaque page de Rustudio en démonstration (moteur simulé, sans focus)."""
from pathlib import Path
import sys, tempfile, time
from measure import Engine, find_window, reveal, running, screenshot_window, until

def main():
    root = Path(__file__).resolve().parent
    app = root / "Rustudio.exe"
    # Options : --theme=sombre (apparence imposée), --kernel-mb=6200 (alerte mémoire noyau simulée),
    # --sheet=update|downloading|failed|done|current (fenêtre de mise à jour ouverte dans cet état),
    # --dossier=nom (sous-dossier de shots/, par défaut le nom du thème).
    options = {a.split("=", 1)[0]: a.split("=", 1)[1] for a in sys.argv[1:] if a.startswith("--") and "=" in a}
    theme = options.get("--theme", "clair")
    if "--kernel-mb" in options:
        import os
        os.environ["RUSTUDIO_SIM_KERNEL_MB"] = options["--kernel-mb"]
    if "--sheet" in options:
        import os
        os.environ["RUSTUDIO_PREVIEW_SHEET"] = options["--sheet"]
    pages = [a for a in sys.argv[1:] if not a.startswith("--")] or ["devices", "mixer", "mic", "mouse", "profiles", "headphones", "routing", "lighting", "scenes", "diagnostic", "audio", "modules", "versions", "settings", "help"]
    out = root / "shots" / options.get("--dossier", theme)
    out.mkdir(parents=True, exist_ok=True)
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
                # page Micro : la voix simulée parle, pour montrer le niveau d'entrée en direct
                engine.call("/api/sim/talk", {"on": page == "mic"})
                with running(app, ["--demo", "--hidden", "--theme", theme, "--page", page, "--profiles-dir", str(Path(folder) / "profiles")], root) as p:
                    hwnd = until(lambda: find_window(p.pid, "Zed::Window", by_class=True), 10, "Fenêtre absente.")
                    reveal(hwnd, 1240, 800)
                    time.sleep(2.2)
                    screenshot_window(hwnd, out / f"{page}.png")
                    print("capture", page)

if __name__ == "__main__":
    main()
