"""Résume les données du banc avec des pourcentages relatifs à Tauri."""
import argparse
import json
import math
from pathlib import Path
from statistics import median


def p95(values):
    ordered = sorted(values)
    return ordered[math.ceil(len(ordered) * 0.95) - 1]


def summarize(raw):
    apps = {}
    for app in ("Tauri", "GPUI"):
        rows = [r for r in raw["trials"] if r["app"] == app]
        latencies = [value for r in rows for value in r["latency_ms"]]
        acks = [value for r in rows for value in r["api_ack_ms"]]
        captures = [value for r in rows for value in r["capture_ms"]]
        apps[app] = {
            "rounds": len(rows),
            "samples": len(latencies),
            "open_median_ms": median(r["opening_ms"] for r in rows),
            "ram_median_mib": median(r["ram_mib"] for r in rows),
            "idle_median_one_core_pct": median(r["cpu_idle_one_core_pct"] for r in rows),
            "active_median_one_core_pct": median(r["cpu_active_one_core_pct"] for r in rows),
            "fader_median_ms": median(latencies),
            "fader_p95_ms": p95(latencies),
            "capture_median_ms": median(captures),
            "capture_p95_ms": p95(captures),
            "ack_median_ms": median(acks),
            "foreground_unchanged": all(r["foreground_unchanged"] for r in rows),
        }
        engine = [value for r in rows for value in r.get("engine_ms", [])]
        after_engine = [value for r in rows for value in r.get("after_engine_ms", [])]
        if engine and after_engine:
            apps[app]["engine_median_ms"] = median(engine)
            apps[app]["after_engine_median_ms"] = median(after_engine)
    delta = {}
    for k, value in apps["Tauri"].items():
        if k.endswith(("_ms", "_mib", "_pct")):
            delta[k] = 100 * (apps["GPUI"][k] / value - 1) if value else None
    notices = {}
    for muted in (True, False):
        rows = [r for r in raw["osd"] if r["muted"] == muted]
        if rows:
            notices["muted" if muted else "live"] = {
                "samples": len(rows),
                "appear_median_ms": median(r["appear_ms"] for r in rows),
                "visible_median_ms": median(r["visible_ms"] for r in rows),
                "accent_verified": all(r["accent_verified"] for r in rows),
                "foreground_unchanged": all(r["foreground_unchanged"] for r in rows),
            }
    return {"apps": apps, "gpui_change_pct": delta, "osd": notices}


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("report", type=Path)
    args = parser.parse_args()
    print(json.dumps(summarize(json.loads(args.report.read_text(encoding="utf-8"))),
                     indent=2, ensure_ascii=False))
