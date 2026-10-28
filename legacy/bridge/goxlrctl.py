# -*- coding: utf-8 -*-
"""GoXLR Mini 5.0.0 : passage a Rustudio.

Lance par la tache « GoXLR Mini Update » de GoXLR Mini 4.x (compte systeme), avec le Python embarque de cette
archive, quand tu cliques sur « Mettre à jour ». Installe Rustudio (Rustudio-Setup.exe --quiet) : Rustudio reprend
tes reglages et ne retire GoXLR Mini qu'une fois en marche (et la GoXLR reprise si Windows la voit). Sinon, tout
revient comme avant et GoXLR Mini affiche l'echec.
"""
import json
import os
import subprocess
import sys
import time

BASE = os.path.dirname(os.path.abspath(__file__))       # <donnees de GoXLR Mini>/update/new
CREATE_NO_WINDOW = 0x08000000


def owners():
    """Comptes dont l'appli GoXLR Mini est ouverte : Rustudio s'ouvrira dans leur session."""
    ps = ("Get-CimInstance Win32_Process -Filter \"Name='GoXLR Mini.exe'\" | ForEach-Object { "
          "$o = Invoke-CimMethod -InputObject $_ -MethodName GetOwner; '{0}\\{1}' -f $o.Domain, $o.User }")
    try:
        out = subprocess.run(["powershell.exe", "-NoProfile", "-NonInteractive", "-Command", ps], capture_output=True,
                             text=True, timeout=90, creationflags=CREATE_NO_WINDOW).stdout
    except Exception:
        return []
    users = []
    for line in out.splitlines():
        u = line.strip()
        if u and u != "\\" and u not in users:
            users.append(u)
    return users


def installed_for():
    """Compte enregistre a l'installation de GoXLR Mini (si l'appli n'est pas ouverte)."""
    try:
        import winreg
        with winreg.OpenKey(winreg.HKEY_LOCAL_MACHINE, r"Software\Microsoft\Windows\CurrentVersion\Uninstall\GoXLRMini") as k:
            v = str(winreg.QueryValueEx(k, "InstalledFor")[0]).strip()
            return [v] if v else []
    except OSError:
        return []


def main():
    if sys.argv[1:2] != ["apply-update"]:
        print("GoXLR Mini est devenu Rustudio : https://github.com/samirmedani/rustudio")
        return 0
    work = os.path.dirname(BASE)
    with open(os.path.join(work, "plan.json"), encoding="utf-8") as f:
        plan = json.load(f)
    data = plan["data_dir"]

    def log(msg):
        try:
            with open(os.path.join(data, "goxlrctl.log"), "a", encoding="utf-8") as f:
                f.write(time.strftime("%Y-%m-%d %H:%M:%S") + " [passage à Rustudio] " + msg + "\n")
        except OSError:
            pass

    users = owners() or installed_for()
    log("Installation de Rustudio " + plan["version"])
    status = os.path.join(work, "rustudio-setup.txt")
    args = [os.path.join(BASE, "Rustudio-Setup.exe"), "--quiet"]
    for u in users:
        args += ["--for-user", u]
    try:
        rc = subprocess.run(args, env=dict(os.environ, RUSTUDIO_SETUP_STATUS=status), timeout=900,
                            creationflags=CREATE_NO_WINDOW).returncode
    except Exception as e:
        rc = 1
        log("Installeur introuvable ou bloqué : " + type(e).__name__)
    err = ""
    try:
        with open(status, encoding="utf-8", errors="replace") as f:
            for line in f:
                if "|error|" in line:
                    err = line.split("|error|", 1)[1].strip()
    except OSError:
        pass
    result = {"version": plan["version"], "from_version": plan["from_version"], "t": time.time(), "ok": rc == 0}
    if rc != 0:
        result["error"] = err or "Rustudio n’a pas pu s’installer"
    os.makedirs(os.path.join(data, "update"), exist_ok=True)
    with open(os.path.join(data, "update", "result.json"), "w", encoding="utf-8") as f:
        json.dump(result, f, ensure_ascii=False)
    log("Rustudio installé : GoXLR Mini retiré, réglages repris" if rc == 0 else "Échec, GoXLR Mini reste en place : " + result["error"])
    return 0 if rc == 0 else 1


if __name__ == "__main__":
    sys.exit(main())
