"""Banc Windows : deux fenêtres visibles, un simulateur, aucun focus demandé."""
import argparse
import contextlib
from collections import Counter
from concurrent.futures import ThreadPoolExecutor
import ctypes as c
from ctypes import wintypes as t
from http.client import HTTPConnection
import json
from pathlib import Path
import random
import socket
import subprocess
import struct
import time
import zlib


def bind(dll, name, restype, *args):
    f = getattr(dll, name)
    f.restype, f.argtypes = restype, args
    return f


U, K, G, DWM = (c.WinDLL(name, use_last_error=True) for name in ("user32", "kernel32", "gdi32", "dwmapi"))
P, D, B, I = c.c_void_p, t.DWORD, t.BOOL, c.c_int
H = t.HANDLE
RECT, POINT = t.RECT, t.POINT
ENUM = c.WINFUNCTYPE(B, H, t.LPARAM)
enum_windows = bind(U, "EnumWindows", B, ENUM, t.LPARAM)
window_pid = bind(U, "GetWindowThreadProcessId", D, H, c.POINTER(D))
title_text = bind(U, "GetWindowTextW", I, H, t.LPWSTR, I)
class_text = bind(U, "GetClassNameW", I, H, t.LPWSTR, I)
foreground = bind(U, "GetForegroundWindow", H)
visible = bind(U, "IsWindowVisible", B, H)
client_rect = bind(U, "GetClientRect", B, H, c.POINTER(RECT))
window_rect = bind(U, "GetWindowRect", B, H, c.POINTER(RECT))
client_to_screen = bind(U, "ClientToScreen", B, H, c.POINTER(POINT))
place = bind(U, "SetWindowPos", B, H, H, I, I, I, I, D)
redraw = bind(U, "RedrawWindow", B, H, P, H, D)
dwm_flush = bind(DWM, "DwmFlush", c.c_long)
keep_display = bind(K, "SetThreadExecutionState", D, D)
post_message = bind(U, "PostMessageW", B, H, t.UINT, t.WPARAM, t.LPARAM)
get_style = bind(U, "GetWindowLongPtrW", c.c_ssize_t, H, I)
set_style = bind(U, "SetWindowLongPtrW", c.c_ssize_t, H, I, c.c_ssize_t)
screen_size = bind(U, "GetSystemMetrics", I, I)
get_dpi = bind(U, "GetDpiForWindow", D, H)
get_dc = bind(U, "GetDC", H, H)
release_dc = bind(U, "ReleaseDC", I, H, H)
compatible_dc = bind(G, "CreateCompatibleDC", H, H)
select_object = bind(G, "SelectObject", H, H, H)
delete_object = bind(G, "DeleteObject", B, H)
delete_dc = bind(G, "DeleteDC", B, H)
bit_blt = bind(G, "BitBlt", B, H, I, I, I, I, H, I, I, D)
create_dib = bind(G, "CreateDIBSection", H, H, P, t.UINT, c.POINTER(P), H, D)
open_process = bind(K, "OpenProcess", H, D, B, D)
close_handle = bind(K, "CloseHandle", B, H)
process_times = bind(K, "GetProcessTimes", B, H, *[c.POINTER(t.FILETIME)] * 4)
memory_info = bind(K, "K32GetProcessMemoryInfo", B, H, P, D)


class BitmapInfo(c.Structure):
    _fields_ = [("size", D), ("width", t.LONG), ("height", t.LONG), ("planes", t.WORD),
                ("bits", t.WORD), ("unused", D * 6)]


class Capture:
    """Lit une petite zone de pixels composée à l'écran, sans capturer le reste du bureau."""
    def __init__(self, x, y, width, height):
        self.x, self.y, self.w, self.h = x, y, width, height
        self.screen = get_dc(None)
        self.dc = compatible_dc(self.screen)
        info = BitmapInfo(c.sizeof(BitmapInfo), width, -height, 1, 32)
        data = P()
        self.dib = create_dib(self.screen, c.byref(info), 0, c.byref(data), None, 0)
        if not (self.screen and self.dc and self.dib and data):
            raise c.WinError(c.get_last_error())
        self.prior = select_object(self.dc, self.dib)
        self.pixels = (c.c_ubyte * (width * height * 4)).from_address(data.value)

    def grab(self):
        if not bit_blt(self.dc, 0, 0, self.w, self.h, self.screen, self.x, self.y, 0x40CC0020):
            raise c.WinError(c.get_last_error())
        return self.pixels

    def close(self):
        select_object(self.dc, self.prior)
        delete_object(self.dib)
        delete_dc(self.dc)
        release_dc(None, self.screen)

    def __enter__(self):
        return self

    def __exit__(self, *_):
        self.close()


class WindowCapture:
    def __init__(self, hwnd, x, y, width, height):
        self.x, self.y, self.w, self.h = x, y, width, height
        self.handle = P()
        self.pixels = (c.c_ubyte * (width * height * 4))()
        self.check(self.open(hwnd, x, y, width, height, c.byref(self.handle)))

    @staticmethod
    def check(result):
        if result < 0:
            raise RuntimeError(f"Capture de fenêtre : HRESULT 0x{result & 0xffffffff:08x}.")

    def grab(self):
        self.check(self.copy(self.handle, self.pixels))
        return self.pixels

    def __enter__(self):
        return self

    def __exit__(self, *_):
        self.release(self.handle)


capture_dll = Path(__file__).with_name("capture.dll")
if capture_dll.is_file():
    dll = c.CDLL(str(capture_dll.resolve()))
    WindowCapture.open = staticmethod(bind(dll, "capture_open", c.c_long, H, I, I, D, D, c.POINTER(P)))
    WindowCapture.copy = staticmethod(bind(dll, "capture_grab", c.c_long, P, P))
    WindowCapture.release = staticmethod(bind(dll, "capture_close", None, P))


def capture_region(hwnd, x, y, width, height):
    if capture_dll.is_file():
        return WindowCapture(hwnd, x, y, width, height)
    return Capture(x, y, width, height)


class ProcessEntry(c.Structure):
    _fields_ = [("size", D), ("usage", D), ("pid", D), ("heap", c.c_size_t), ("module", D),
                ("threads", D), ("parent", D), ("base", t.LONG), ("flags", D), ("exe", t.WCHAR * 260)]


class Memory(c.Structure):
    _fields_ = [("size", D), ("faults", D), ("values", c.c_size_t * 9), ("private_ws", c.c_size_t),
                ("shared_commit", c.c_uint64)]


toolhelp = bind(K, "CreateToolhelp32Snapshot", H, D, D)
process_first = bind(K, "Process32FirstW", B, H, c.POINTER(ProcessEntry))
process_next = bind(K, "Process32NextW", B, H, c.POINTER(ProcessEntry))


def snapshot_tree(root):
    snap = toolhelp(2, 0)
    if snap == P(-1).value:
        raise c.WinError(c.get_last_error())
    try:
        p = ProcessEntry()
        p.size = c.sizeof(p)
        valid, edges = process_first(snap, c.byref(p)), {}
        while valid:
            edges.setdefault(p.parent, []).append((p.pid, p.exe.lower()))
            valid = process_next(snap, c.byref(p))
    finally:
        close_handle(snap)
    found = {root: ""}
    stack = [root]
    while stack:
        for child, name in edges.get(stack.pop(), []):
            if child not in found:
                found[child] = name
                stack.append(child)
    samples = {}
    for pid, name in found.items():
        proc = open_process(0x1000, False, pid)
        if not proc:
            continue
        try:
            times = [t.FILETIME() for _ in range(4)]
            mem = Memory()
            mem.size = c.sizeof(mem)
            if not (process_times(proc, *map(c.byref, times)) and
                    memory_info(proc, c.byref(mem), c.sizeof(mem))):
                continue
            secs = sum((v.dwHighDateTime << 32) + v.dwLowDateTime for v in times[2:]) / 10_000_000
            samples[pid] = (secs, mem.private_ws / 2**20, name)
        finally:
            close_handle(proc)
    if root not in samples:
        raise RuntimeError("Processus principal non mesurable.")
    return samples


def find_window(pid, match, by_class=False):
    result = []

    @ENUM
    def callback(hwnd, _):
        owner, buf = D(), c.create_unicode_buffer(256)
        window_pid(hwnd, c.byref(owner))
        if owner.value == pid:
            (class_text if by_class else title_text)(hwnd, buf, len(buf))
            if match in buf.value:
                result.append(hwnd)
                return False
        return True

    enum_windows(callback, 0)
    return result[0] if result else None


def until(fn, seconds, error, interval=0.006):
    deadline = time.perf_counter() + seconds
    while time.perf_counter() < deadline:
        value = fn()
        if value is not None and value is not False:
            return value
        time.sleep(interval)
    raise RuntimeError(error)


def reveal(hwnd, client_width=900, client_height=890, right=True):
    # WS_EX_NOACTIVATE évite aussi qu'un clic accidentel vole le focus.
    set_style(hwnd, -20, get_style(hwnd, -20) | 0x08000000)
    rect, client = RECT(), RECT()
    if not (window_rect(hwnd, c.byref(rect)) and client_rect(hwnd, c.byref(client))):
        raise c.WinError(c.get_last_error())
    scale = (get_dpi(hwnd) or 96) / 96
    width = round(client_width * scale) + (rect.right - rect.left) - client.right
    height = round(client_height * scale) + (rect.bottom - rect.top) - client.bottom
    x = max(0, screen_size(0) - width - 24) if right else 24
    if not place(hwnd, H(-1), x, 40, width, height, 0x0010 | 0x0040):
        raise c.WinError(c.get_last_error())
    redraw(hwnd, None, None, 0x0101)  # RDW_INVALIDATE | RDW_UPDATENOW


def fader_capture(hwnd):
    rect, origin = RECT(), POINT()
    if not (client_rect(hwnd, c.byref(rect)) and client_to_screen(hwnd, c.byref(origin))):
        raise c.WinError(c.get_last_error())
    scale = (get_dpi(hwnd) or 96) / 96
    width = rect.right
    center = (width - 96 * scale) / 8 + 24 * scale
    half = round(15 * scale)
    start_y = round(102 * scale)
    return capture_region(hwnd, origin.x + round(center) - half, origin.y + start_y, 2 * half + 1,
                   min(rect.bottom, round(375 * scale)) - start_y), scale


def knob(cap, scale):
    pixels = cap.grab()
    bright = lambda i: min(pixels[i:i + 3]) >= 150
    runs, start = [], None
    for y in range(cap.h + 1):
        i = y * cap.w * 4
        if y < cap.h and bright(i) and bright(i + (cap.w - 1) * 4):
            if start is None:
                start = y
        elif start is not None:
            if 13 * scale <= y - start <= 30 * scale:
                runs.append((y - start, (start + y) / 2))
            start = None
    return max(runs)[1] if runs else None


class Engine:
    def __init__(self):
        self.conn = HTTPConnection("127.0.0.1", 14566, timeout=2)

    def call(self, path, payload=None):
        if payload is None:
            self.conn.request("GET", path)
        else:
            self.conn.request("POST", path, json.dumps(payload),
                              {"Content-Type": "application/json", "X-Rustudio": "1"})
        response = self.conn.getresponse()
        body = json.loads(response.read())
        if response.status != 200 or body.get("ok") is False:
            raise RuntimeError(f"API du simulateur: {path}, HTTP {response.status}")
        return body

    def call_with_header(self, path):
        self.conn.request("GET", path, headers={"X-Rustudio": "1"})
        response = self.conn.getresponse()
        body = json.loads(response.read())
        if response.status != 200:
            raise RuntimeError(f"Vue native indisponible : HTTP {response.status}.")
        return body

    def set(self, index, value):
        self.call("/api/sim/fader", {"index": index, "value": value})

    def fader(self, index):
        snap = self.call_with_header("/api/modules/goxlr-mini/mixer")
        fader = snap["faders"][index]
        volume = next(ch["volume"] for ch in snap["channels"] if ch["id"] == fader["channel"])
        return fader["position"], volume

    def set_and_wait(self, index, value):
        # La première lecture USB établit la référence sans modifier le volume.
        if self.fader(index)[0] in (None, value):
            prime = 252 if value < 128 else 3
            self.set(index, prime)
            until(lambda: self.fader(index)[0] == prime, 4, "Position de référence absente.", 0.02)
        self.set(index, value)
        def reached():
            return self.fader(index) == (value, value)
        try:
            until(reached, 4, "Le simulateur n’a pas appliqué la position.", 0.02)
        except RuntimeError as exc:
            raise RuntimeError(f"{exc} Attendu : {value}, obtenu : {self.fader(index)}.") from exc

    def mute(self, muted):
        self.call("/api/modules/goxlr-mini/mute",
                  {"channel": "Mic", "muted": muted, "target": "all"})


@contextlib.contextmanager
def running(path, args, root):
    with open(root / (path.stem + ".log"), "ab") as log:
        p = subprocess.Popen([str(path), *args], cwd=root, stdout=log, stderr=log,
                             creationflags=subprocess.CREATE_NO_WINDOW)
        try:
            yield p
        finally:
            if p.poll() is None:
                p.terminate()
            p.wait(timeout=5)


def cpu_percent(before, after, seconds):
    return 100 * sum(after[pid][0] - old[0] for pid, old in before.items() if pid in after) / seconds


def trial(root, engine, name, path, count, idle_seconds, drive_seconds, seed, observe_engine):
    for idx in range(4):
        engine.set_and_wait(idx, 40 + 30 * idx)
    focus = foreground()
    start = time.perf_counter()
    args = ["--demo", "--hidden", "--mixer"] if name == "GPUI" else []
    with running(path, args, root) as p:
        def available():
            if p.poll() is not None:
                raise RuntimeError(f"{name} a quitté ({p.returncode}).")
            return find_window(p.pid, "Zed::Window", by_class=True) if name == "GPUI" else find_window(p.pid, "Mixeur")

        hwnd = until(available, 12, f"Fenêtre {name} absente.")
        reveal(hwnd, right=name == "GPUI")
        if foreground() == hwnd:
            raise RuntimeError(f"{name} a pris le focus.")
        time.sleep(0.025)
        cap, scale = fader_capture(hwnd)
        with cap:
            low = until(lambda: knob(cap, scale), 10,
                        f"Fader de {name} non visible : x={cap.x}, y={cap.y}, échelle={scale}.")
            opening = (time.perf_counter() - start) * 1000
            time.sleep(1.5)
            idle_start = time.perf_counter()
            before = snapshot_tree(p.pid)
            time.sleep(idle_seconds)
            after = snapshot_tree(p.pid)
            idle_cpu = cpu_percent(before, after, time.perf_counter() - idle_start)
            memory = sum(v[1] for v in after.values())
            webviews = sum(v[2] == "msedgewebview2.exe" for v in after.values())
            if (name == "Tauri") != (webviews > 0):
                raise RuntimeError(f"Nombre inattendu de processus WebView2 pour {name}: {webviews}")
            if abs(until(lambda: knob(cap, scale), 2, "Fenêtre masquée.") - low) > 8 * scale:
                raise RuntimeError(f"Le fader {name} n’est pas stable au repos.")
            rand = random.Random(seed)
            latencies, acknowledgements, grabs, engine_delays, ui_delays = [], [], [], [], []
            with ThreadPoolExecutor(max_workers=1) as sender:
                for idx in range(count):
                    time.sleep(rand.uniform(0.065, 0.135))
                    high = idx % 2 == 0
                    sent = time.perf_counter()

                    def move():
                        target = 220 if high else 40
                        engine.set(0, target)
                        ack = time.perf_counter()
                        if not observe_engine:
                            return ack, None

                        def published():
                            positions = engine.call("/api/live")["live"]["fader_positions"]
                            return time.perf_counter() if positions[0] == target else None

                        return ack, until(published, 1.0, "Position non publiée par le moteur.", 0.003)

                    acknowledgement = sender.submit(move)

                    positions = set()
                    def moved():
                        begin_grab = time.perf_counter()
                        y = knob(cap, scale)
                        end_grab = time.perf_counter()
                        grabs.append(round((end_grab - begin_grab) * 1000, 2))
                        positions.add(y)
                        if y is not None and ((y < low - 95 * scale) if high else abs(y - low) < 8 * scale):
                            return end_grab

                    try:
                        displayed = until(moved, 1.2, f"Mouvement {idx + 1} de {name} non observé.", 0.003)
                    except RuntimeError as exc:
                        acknowledgement.result(timeout=2)
                        raise RuntimeError(f"{exc} Départ : {low}; captures : {positions}; moteur : {engine.fader(0)}.") from exc
                    latencies.append(round((displayed - sent) * 1000, 2))
                    ack, engine_ready = acknowledgement.result(timeout=2)
                    acknowledgements.append(round((ack - sent) * 1000, 2))
                    if engine_ready is not None:
                        engine_delays.append(round((engine_ready - sent) * 1000, 2))
                        ui_delays.append(round((displayed - engine_ready) * 1000, 2))
            # Même séquence soutenue pour les deux fenêtres : 20 commandes/s, quatre voies.
            active_start = time.perf_counter()
            before = snapshot_tree(p.pid)
            n = int(drive_seconds * 20)
            for idx in range(n):
                engine.set(idx % 4, 40 + (idx * 29) % 180)
                time.sleep(max(0, active_start + (idx + 1) / 20 - time.perf_counter()))
            after = snapshot_tree(p.pid)
            active_cpu = cpu_percent(before, after, time.perf_counter() - active_start)
    return {"app": name, "opening_ms": round(opening, 2), "latency_ms": latencies,
            "ram_mib": round(memory, 3), "cpu_idle_one_core_pct": round(idle_cpu, 4),
            "cpu_active_one_core_pct": round(active_cpu, 4), "webviews": webviews,
            "api_ack_ms": acknowledgements, "capture_ms": grabs, "engine_ms": engine_delays,
            "after_engine_ms": ui_delays,
            "foreground_unchanged": foreground() == focus}


def verify_osd(root, engine, tauri):
    engine.mute(False)
    focus = foreground()
    reports = []
    with running(tauri, ["--tray"], root) as p:
        time.sleep(2.4)
        for muted in (True, False) * 3:
            sent = time.perf_counter()
            engine.mute(muted)

            def showing():
                hwnd = find_window(p.pid, "RustudioNativeOsd", by_class=True)
                return hwnd if hwnd and visible(hwnd) else None

            hwnd = until(showing, 3, "Bulle micro non visible.")
            appeared = time.perf_counter()
            rect = RECT()
            window_rect(hwnd, c.byref(rect))
            scale = (get_dpi(hwnd) or 96) / 96
            with Capture(rect.left + round(17 * scale), rect.top + round(19 * scale),
                         round(38 * scale), round(36 * scale)) as cap:
                def accent_drawn():
                    pixels = cap.grab()
                    for i in range(0, len(pixels), 4):
                        b, g, r = pixels[i:i + 3]
                        if (r > g + 45 and r > b + 45) if muted else (g > r + 45 and g > b + 45):
                            return True

                until(accent_drawn, 0.4, "La couleur attendue n’a pas été rendue.")
            same_focus = foreground() == focus
            until(lambda: not visible(hwnd), 3, "La bulle ne se masque pas.")
            hidden_at = time.perf_counter()
            reports.append({"muted": muted, "appear_ms": round((appeared - sent) * 1000, 2),
                            "visible_ms": round((hidden_at - appeared) * 1000, 2),
                            "accent_verified": True, "foreground_unchanged": same_focus and foreground() == focus})
    return reports


def screenshot_window(hwnd, destination):
    rect, origin = RECT(), POINT()
    if not (client_rect(hwnd, c.byref(rect)) and client_to_screen(hwnd, c.byref(origin))):
        raise c.WinError(c.get_last_error())
    if not capture_dll.is_file():
        raise RuntimeError("La capture PNG du mixeur nécessite capture.dll.")
    with capture_region(hwnd, origin.x, origin.y, rect.right, rect.bottom) as capture:
        bgra = until(lambda: raw if any((raw := bytes(capture.grab()))[3::4]) else None, 2,
                     "Aucune image du mixeur disponible.")
    rows = []
    for y in range(rect.bottom):
        start = y * rect.right * 4
        row = bgra[start:start + rect.right * 4]
        rgb = bytearray(rect.right * 3)
        rgb[0::3], rgb[1::3], rgb[2::3] = row[2::4], row[1::4], row[0::4]
        rows.append(b"\0" + rgb)

    def chunk(tag, data):
        return struct.pack(">I", len(data)) + tag + data + struct.pack(">I", zlib.crc32(tag + data))

    header = struct.pack(">IIBBBBB", rect.right, rect.bottom, 8, 2, 0, 0, 0)
    destination.write_bytes(b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", header)
                            + chunk(b"IDAT", zlib.compress(b"".join(rows))) + chunk(b"IEND", b""))


def verify_gpui(root, engine, gpui, screenshot):
    with running(gpui, ["--demo", "--self-test"], root) as client:
        if client.wait(timeout=10) != 0:
            raise RuntimeError("Le test des commandes natives n’a pas abouti.")
    for idx, level in enumerate((55, 110, 165, 220)):
        engine.set_and_wait(idx, level)
    focus = foreground()
    with running(gpui, ["--demo", "--hidden", "--mixer"], root) as client:
        hwnd = until(lambda: find_window(client.pid, "Zed::Window", by_class=True), 10, "Fenêtre GPUI absente.")
        reveal(hwnd)
        time.sleep(0.1)
        dwm_flush()
        cap, scale = fader_capture(hwnd)
        with cap:
            try:
                low = until(lambda: knob(cap, scale), 5, "Fader initial invisible.")
            except RuntimeError as exc:
                pixels = bytes(cap.grab())
                colors = Counter(tuple(pixels[i:i + 3]) for i in range(0, len(pixels), 4))
                raise RuntimeError(f"{exc} Coordonnées {cap.x}, {cap.y} ; couleurs : {colors.most_common(5)}.") from exc
            origin = POINT()
            if not client_to_screen(hwnd, c.byref(origin)):
                raise c.WinError(c.get_last_error())
            x = cap.x + cap.w // 2 - origin.x
            y = round(cap.y + low - origin.y)

            def pointer(message, buttons, offset):
                packed = (round(y - offset * scale) << 16) | x
                # Messages adressés à la fenêtre de test, sans curseur système ni activation.
                if not post_message(hwnd, message, buttons, packed):
                    raise c.WinError(c.get_last_error())

            pointer(0x0201, 1, 0)  # WM_LBUTTONDOWN
            pointer(0x0200, 1, 55)  # WM_MOUSEMOVE, MK_LBUTTON

            def volume_at_least(level):
                _, volume = engine.fader(0)
                return volume if volume >= level else None

            during = until(lambda: volume_at_least(105), 2,
                           "Le volume ne suit pas le glissement avant le relâchement.")
            pointer(0x0202, 0, 95)  # WM_LBUTTONUP
            released = until(lambda: volume_at_least(during + 35), 2,
                             "Le relâchement n’a pas envoyé la position finale.")
            engine.set_and_wait(0, 205)
            observed = []
            def moved():
                y2 = knob(cap, scale)
                if y2 is not None:
                    observed.append(round((low - y2) / scale, 1))
                return y2 is not None and y2 < low - 92 * scale
            try:
                until(moved, 3, "Le fader physique n’a pas repris la main à l’écran.")
            except RuntimeError as exc:
                raise RuntimeError(f"{exc} Déplacement observé : {sorted(set(observed))}.") from exc
        if screenshot:
            screenshot_window(hwnd, screenshot)
    return {"commands_ok": True, "drag_volume": during, "released_volume": released, "physical_follow": True,
            "foreground_unchanged": foreground() == focus}


def main():
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--rounds", type=int, default=3)
    ap.add_argument("--changes", type=int, default=22)
    ap.add_argument("--idle", type=float, default=8)
    ap.add_argument("--drive", type=float, default=8)
    ap.add_argument("--observe-engine", action="store_true")
    ap.add_argument("--skip-osd", action="store_true")
    ap.add_argument("--verify-gpui", action="store_true")
    ap.add_argument("--screenshot", type=Path)
    ap.add_argument("--result", type=Path, required=True)
    args = ap.parse_args()
    U.SetProcessDpiAwarenessContext.argtypes = [P]
    U.SetProcessDpiAwarenessContext(P(-4))
    root = Path(__file__).resolve().parent
    tauri, gpui = root / "RustudioTauriBench.exe", root / "rustudio-gpui-mixer.exe"
    engine = Engine()

    def ready():
        try:
            state = engine.call("/api/state")
            if not state["sim"]:
                raise RuntimeError("L’API n’est pas celle du simulateur.")
            return state["connected"]
        except (OSError, ConnectionError):
            return False

    with socket.socket() as guard:
        guard.setsockopt(socket.SOL_SOCKET, socket.SO_EXCLUSIVEADDRUSE, 1)
        guard.bind(("127.0.0.1", 14566))
    with running(root / "rustudio-engine-bench.exe",
                 ["run", "--child", "--simulate", "--port", "14566", "--data-dir", str(root / "state")], root) as p:
        until(ready, 8, "Le simulateur ne répond pas.")
        if p.poll() is not None:
            raise RuntimeError("Le port n’a pas été pris par notre simulateur.")
        result = {"trials": [], "osd": [], "display_size": [screen_size(0), screen_size(1)],
                  "mixer_capture": "Windows Graphics Capture" if capture_dll.is_file() else "GDI"}
        for i in range(args.rounds):
            pair = [("Tauri", tauri), ("GPUI", gpui)]
            for name, path in pair if i % 2 == 0 else reversed(pair):
                result["trials"].append(trial(root, engine, name, path, args.changes, args.idle, args.drive, i + 33, args.observe_engine))
                args.result.write_text(json.dumps(result, indent=2), encoding="utf-8")
                time.sleep(0.8)
        if args.verify_gpui:
            result["gpui_controls"] = verify_gpui(root, engine, gpui, args.screenshot)
        if not args.skip_osd:
            result["osd"] = verify_osd(root, engine, tauri)
        args.result.write_text(json.dumps(result, indent=2), encoding="utf-8")


if __name__ == "__main__":
    # Pendant le test seulement : les pixels du bureau sont figés si l’écran dort.
    keep_display(0x80000002)  # ES_CONTINUOUS | ES_DISPLAY_REQUIRED
    try:
        main()
    finally:
        keep_display(0x80000000)  # ES_CONTINUOUS
