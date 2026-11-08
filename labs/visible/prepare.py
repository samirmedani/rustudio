"""Adapte une copie jetable de Rustudio au benchmark. N'exécuter que dans la copie de compilation."""
import json
from pathlib import Path


def change(path, old, new):
    file = Path(path)
    source = file.read_text(encoding="utf-8")
    if source.count(old) != 1:
        raise ValueError(f"Ancre non unique : {path}: {old[:60]}")
    file.write_text(source.replace(old, new), encoding="utf-8")


app = "crates/app/src/main.rs"
change(app, 'const ENGINE: &str = "http://127.0.0.1:14565";',
       'const ENGINE: &str = "http://127.0.0.1:14566";')
change(app, '        let route = LAST_ROUTE.lock().unwrap().clone();\n        if w.navigate(format!("{ENGINE}/#{route}").parse().unwrap()).is_err() {',
       '        if w.navigate(format!("{ENGINE}/lab-visible-mixer").parse().unwrap()).is_err() {')
change(app, "    let _ = w.unminimize();\n    let _ = w.show();\n    webview_visible(w, true);\n    let _ = w.set_focus();",
       "    // La fenêtre est révélée par le banc, avec SWP_NOACTIVATE.\n    webview_visible(w, true);")
change(app, '        .title("Rustudio")\n        .inner_size(1280.0, 800.0)\n        .min_inner_size(900.0, 620.0)',
       '        .title("Rustudio · Mixeur Tauri")\n        .inner_size(780.0, 470.0)\n        .min_inner_size(780.0, 470.0)')
change(app, '                    let _ = hidden("sc").args(["start", "Rustudio"]).spawn();',
       '                    // Le banc ne redémarre jamais le service installé.')
change("crates/app/tauri.conf.json", '"identifier": "app.rustudio"', '"identifier": "app.rustudio.benchmark"')
capability = Path("crates/app/capabilities/engine.json")
cfg = json.loads(capability.read_text(encoding="utf-8"))
cfg["remote"]["urls"] = [url.replace("14565", "14566") for url in cfg["remote"]["urls"]]
capability.write_text(json.dumps(cfg, indent=2) + "\n", encoding="utf-8")
change("crates/engine/src/http.rs", '                "/api/live" =>',
       '                "/lab-visible-mixer" if ctl.sim.is_some() => return respond(req, include_str!("../../../labs/visible/mixer.html"), "text/html; charset=utf-8", 200),\n                "/api/live" =>')
