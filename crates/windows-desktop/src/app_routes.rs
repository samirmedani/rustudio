//! Chaque appli sur sa voie : Discord vers Chat, Spotify vers Music, un jeu vers Game. Windows garde ce choix
//! par programme (Paramètres > Son > Volume des applications), comme si l'utilisateur l'avait fait. Un choix déjà
//! fait pour une appli n'est jamais remplacé, et la fonction se coupe dans les Réglages de Rustudio.
use std::path::{Path, PathBuf};
use std::time::Duration;

const CHAT: &[&str] = &[
    "discord.exe", "discordcanary.exe", "discordptb.exe", "vesktop.exe", "ts3client_win64.exe", "ts3client_win32.exe",
    "teamspeak.exe", "ms-teams.exe", "teams.exe", "zoom.exe", "skype.exe", "slack.exe", "mumble.exe", "telegram.exe",
    "whatsapp.exe", "signal.exe", "guilded.exe", "element.exe", "revolt.exe",
];
const MUSIC: &[&str] = &[
    "spotify.exe", "deezer.exe", "applemusic.exe", "itunes.exe", "tidal.exe", "foobar2000.exe", "musicbee.exe",
    "aimp.exe", "winamp.exe", "amazon music.exe", "qobuz.exe", "youtube music.exe", "youtube music desktop app.exe",
    "ytmdesktop.exe", "cider.exe", "soundcloud.exe", "audirvana.exe",
];
/// Dossiers d'installation des jeux des boutiques ; leurs lanceurs vivent ailleurs.
const GAME_FOLDERS: &[&str] = &[
    "\\steamapps\\common\\", "\\epic games\\", "\\riot games\\", "\\xboxgames\\", "\\gog galaxy\\games\\",
    "\\ubisoft game launcher\\games\\", "\\ea games\\",
];
/// Programmes des dossiers de jeux qui ne sont pas des jeux : lanceurs et services.
const NOT_GAMES: &[&str] = &[
    "riotclientservices.exe", "riotclientux.exe", "riotclientuxrender.exe", "epicgameslauncher.exe", "crashreportclient.exe",
    "unrealeditor.exe",
];

/// Voie d'un programme d'après son chemin, ou None : il reste sur la sortie par défaut (System).
pub fn channel_for(exe_path: &str) -> Option<&'static str> {
    let path = exe_path.to_lowercase();
    let name = path.rsplit(['\\', '/']).next().unwrap_or(&path);
    if CHAT.contains(&name) {
        Some("Chat")
    } else if MUSIC.contains(&name) {
        Some("Music")
    } else if GAME_FOLDERS.iter().any(|f| path.contains(f)) && !NOT_GAMES.contains(&name) && !path.contains("\\epic games\\ue_") {
        Some("Game")
    } else {
        None
    }
}

/// Préférence de l'appli installée : %LOCALAPPDATA%\Rustudio\sorties-auto.txt.
pub fn default_pref() -> PathBuf {
    std::env::var_os("LOCALAPPDATA").map(PathBuf::from).unwrap_or_else(std::env::temp_dir).join("Rustudio").join("sorties-auto.txt")
}

/// Active sauf si le fichier de préférence contient « 0 ».
pub fn enabled(pref: &Path) -> bool {
    std::fs::read_to_string(pref).map(|t| t.trim() != "0").unwrap_or(true)
}

pub fn set_enabled(pref: &Path, on: bool) -> Result<(), String> {
    if let Some(dir) = pref.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    std::fs::write(pref, if on { "1" } else { "0" }).map_err(|e| format!("Préférence non enregistrée : {e}"))
}

/// Vérifie toutes les 10 s les programmes lancés (appli installée seulement). Chaque réglage fait est noté dans
/// sorties-auto.log, à côté de la préférence (les 200 dernières lignes).
pub fn start(pref: PathBuf) {
    #[cfg(windows)]
    std::thread::spawn(move || {
        let mut state = win::RouteState::default();
        let journal = pref.with_extension("log");
        note(&journal, "démarrage de la vérification des sorties par appli");
        loop {
            if enabled(&pref) {
                for line in win::route_running(&mut state) {
                    note(&journal, &line);
                }
            }
            std::thread::sleep(Duration::from_secs(10));
        }
    });
    #[cfg(not(windows))]
    {
        let _ = (pref, Duration::ZERO);
    }
}

/// Une passe immédiate, détaillée (option « --sorties-auto » de l'appli, pour vérifier sur un PC).
pub fn run_once() -> Vec<String> {
    #[cfg(windows)]
    {
        let mut lines = vec![format!("préférence {} (active : {})", default_pref().display(), enabled(&default_pref()))];
        lines.extend(win::describe());
        lines.extend(win::route_running(&mut win::RouteState::default()));
        lines
    }
    #[cfg(not(windows))]
    vec!["Windows seulement".into()]
}

#[cfg(windows)]
fn note(journal: &Path, line: &str) {
    let mut lines: Vec<String> = std::fs::read_to_string(journal).unwrap_or_default().lines().map(str::to_owned).collect();
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    lines.push(format!("{now} {line}"));
    let start = lines.len().saturating_sub(200);
    if let Some(dir) = journal.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let _ = std::fs::write(journal, lines[start..].join("\n") + "\n");
}

#[cfg(windows)]
#[allow(non_snake_case)]
mod win {
    use super::channel_for;
    use std::collections::HashSet;
    use windows::core::{interface, IUnknown, IUnknown_Vtbl, HRESULT, HSTRING};
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::System::Com::{CoInitializeEx, CoUninitialize, COINIT_MULTITHREADED};
    use windows::Win32::System::Diagnostics::ToolHelp::{CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W, TH32CS_SNAPPROCESS};
    use windows::Win32::System::Threading::{OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION};
    use windows::Win32::System::WinRT::RoGetActivationFactory;

    /// Fabrique de « Windows.Media.Internal.AudioPolicyConfig » (Windows 10 21H2 et plus récents), celle du
    /// panneau « Volume des applications ». Déclarée sur IUnknown : i0 à i2 tiennent la place des trois méthodes
    /// d'IInspectable, puis viennent 19 méthodes qui ne servent pas ici.
    #[interface("ab3d4648-e242-459f-b02f-541c70306324")]
    unsafe trait IAudioPolicyConfigFactory: IUnknown {
        fn i0(&self) -> HRESULT;
        fn i1(&self) -> HRESULT;
        fn i2(&self) -> HRESULT;
        fn m00(&self) -> HRESULT;
        fn m01(&self) -> HRESULT;
        fn m02(&self) -> HRESULT;
        fn m03(&self) -> HRESULT;
        fn m04(&self) -> HRESULT;
        fn m05(&self) -> HRESULT;
        fn m06(&self) -> HRESULT;
        fn m07(&self) -> HRESULT;
        fn m08(&self) -> HRESULT;
        fn m09(&self) -> HRESULT;
        fn m10(&self) -> HRESULT;
        fn m11(&self) -> HRESULT;
        fn m12(&self) -> HRESULT;
        fn m13(&self) -> HRESULT;
        fn m14(&self) -> HRESULT;
        fn m15(&self) -> HRESULT;
        fn m16(&self) -> HRESULT;
        fn m17(&self) -> HRESULT;
        fn m18(&self) -> HRESULT;
        fn SetPersistedDefaultAudioEndpoint(&self, process: u32, flow: i32, role: i32, device: *mut core::ffi::c_void) -> HRESULT;
        fn GetPersistedDefaultAudioEndpoint(&self, process: u32, flow: i32, role: i32, device: *mut *mut core::ffi::c_void) -> HRESULT;
    }

    const RENDER: i32 = 0;
    /// Identifiant d'interface de périphérique attendu par la fabrique pour une sortie MMDevice.
    fn device_path(endpoint: &str) -> String {
        format!("\\\\?\\SWD#MMDEVAPI#{endpoint}#{{e6327cad-dcec-4949-ae8a-991e976a79d2}}")
    }

    #[derive(Default)]
    pub struct RouteState {
        checked: HashSet<u32>,
        targets: Vec<(String, String)>,
    }

    fn running_unseen(skip: &HashSet<u32>) -> (HashSet<u32>, Vec<(u32, String)>) {
        let mut live = HashSet::new();
        let mut out = vec![];
        unsafe {
            let Ok(snap) = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) else { return (live, out) };
            let mut entry = PROCESSENTRY32W { dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32, ..Default::default() };
            let mut ok = Process32FirstW(snap, &mut entry).is_ok();
            while ok {
                let pid = entry.th32ProcessID;
                live.insert(pid);
                if !skip.contains(&pid) {
                    if let Ok(h) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) {
                        let mut buf = [0u16; 1024];
                        let mut len = buf.len() as u32;
                        if QueryFullProcessImageNameW(h, PROCESS_NAME_WIN32, windows::core::PWSTR(buf.as_mut_ptr()), &mut len).is_ok() {
                            out.push((pid, String::from_utf16_lossy(&buf[..len as usize])));
                        }
                        let _ = CloseHandle(h);
                    }
                }
                ok = Process32NextW(snap, &mut entry).is_ok();
            }
            let _ = CloseHandle(snap);
        }
        (live, out)
    }

    /// Ce que voit une passe : sorties de la GoXLR et programmes reconnus.
    pub fn describe() -> Vec<String> {
        let mut lines: Vec<String> = crate::audio::active_endpoints(false).into_iter().filter(|e| e.goxlr).map(|e| format!("sortie {} = {}", e.name, e.id)).collect();
        let (_, all) = running_unseen(&HashSet::new());
        lines.push(format!("{} programmes lisibles", all.len()));
        lines.extend(all.into_iter().filter_map(|(pid, path)| channel_for(&path).map(|c| format!("programme {pid} {path} -> {c}"))));
        lines
    }

    /// Une passe : chaque programme connu, lancé, sans sortie choisie, part sur la sortie de sa voie. Rend une
    /// ligne de journal par programme traité.
    pub fn route_running(state: &mut RouteState) -> Vec<String> {
        let mut journal = vec![];
        let outputs = crate::audio::active_endpoints(false);
        // un choix qui vise une sortie encore branchée est gardé ; un choix vers un ancien périphérique est remplacé
        let present: Vec<HSTRING> = outputs.iter().map(|e| HSTRING::from(device_path(&e.id))).collect();
        let mut targets: Vec<(String, String)> = outputs.into_iter().filter(|e| e.goxlr).map(|e| (e.name, e.id)).collect();
        targets.sort();
        if targets != state.targets {
            state.checked.clear();
            state.targets = targets.clone();
        }
        if targets.is_empty() {
            return journal;
        }
        let (live, processes) = running_unseen(&state.checked);
        state.checked.retain(|pid| live.contains(pid));
        let mut todo: Vec<(u32, String, &str, String)> = vec![];
        for (pid, path) in processes {
            if let Some(channel) = channel_for(&path) {
                if let Some((_, endpoint)) = targets.iter().find(|(name, _)| name == channel) {
                    todo.push((pid, endpoint.clone(), channel, path));
                    continue;
                }
            }
            // Programme deja inspecte (hors liste ou voie absente) : inutile de rouvrir son handle toutes les 10 s.
            state.checked.insert(pid);
        }
        if todo.is_empty() {
            return journal;
        }
        unsafe {
            let initialised = CoInitializeEx(None, COINIT_MULTITHREADED).is_ok();
            if let Ok(factory) = RoGetActivationFactory::<IAudioPolicyConfigFactory>(&HSTRING::from("Windows.Media.Internal.AudioPolicyConfig")) {
                let read = |pid: u32| -> Option<HSTRING> {
                    let mut current: *mut core::ffi::c_void = std::ptr::null_mut();
                    factory.GetPersistedDefaultAudioEndpoint(pid, RENDER, 0, &mut current).ok().ok()?;
                    Some(std::mem::transmute::<*mut core::ffi::c_void, HSTRING>(current))
                };
                for (pid, endpoint, channel, path) in todo {
                    state.checked.insert(pid);
                    let name = path.rsplit('\\').next().unwrap_or(&path).to_owned();
                    // Windows répond par une erreur quand aucun choix n'est mémorisé pour ce programme.
                    // Un choix existant (fait par l'utilisateur, ou par nous) est gardé.
                    let replaced = match read(pid).filter(|c| !c.is_empty()) {
                        Some(current) if present.iter().any(|p| p.to_string().eq_ignore_ascii_case(&current.to_string())) => {
                            journal.push(format!("{name} : sortie déjà choisie, gardée ({current})"));
                            continue;
                        }
                        Some(_) => " (ancienne sortie débranchée remplacée)",
                        None => "",
                    };
                    let target = HSTRING::from(device_path(&endpoint));
                    // HSTRING est un pointeur transparent : sa valeur est le descripteur attendu par l'ABI
                    let raw: *mut core::ffi::c_void = std::mem::transmute_copy(&target);
                    let results: Vec<HRESULT> = [0, 1, 2].iter().map(|&role| factory.SetPersistedDefaultAudioEndpoint(pid, RENDER, role, raw)).collect();
                    let confirmed = read(pid).is_some_and(|now| now == target);
                    journal.push(format!(
                        "{name} -> {channel}{replaced} : {}",
                        if confirmed { "fait".to_owned() } else { format!("non confirmé ({:?})", results.iter().map(|r| r.0).collect::<Vec<_>>()) }
                    ));
                }
            } else {
                journal.push("Fabrique AudioPolicyConfig indisponible sur ce Windows".into());
            }
            if initialised {
                CoUninitialize();
            }
        }
        journal
    }

}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn voie_selon_le_programme() {
        assert_eq!(channel_for(r"D:\Apps\Discord\app-1.0.9\Discord.exe"), Some("Chat"));
        assert_eq!(channel_for(r"D:\Apps\Spotify\Spotify.exe"), Some("Music"));
        assert_eq!(channel_for(r"D:\SteamLibrary\steamapps\common\Counter-Strike Global Offensive\game\bin\win64\cs2.exe"), Some("Game"));
        assert_eq!(channel_for(r"C:\Riot Games\VALORANT\live\VALORANT.exe"), Some("Game"));
        assert_eq!(channel_for(r"C:\Riot Games\Riot Client\RiotClientServices.exe"), None, "lanceur, pas un jeu");
        assert_eq!(channel_for(r"C:\Program Files (x86)\Steam\steam.exe"), None, "Steam lui-même reste sur Système");
        assert_eq!(channel_for(r"C:\Program Files\Google\Chrome\Application\chrome.exe"), None);
        assert_eq!(channel_for(r"C:\Program Files\Epic Games\UE_5.8\Engine\Binaries\Win64\UnrealEditor.exe"), None, "éditeur, pas un jeu");
        assert_eq!(channel_for(r"C:\Program Files\Epic Games\Fortnite\FortniteGame\Binaries\Win64\FortniteClient-Win64-Shipping.exe"), Some("Game"));
    }

    #[test]
    fn preference_active_par_defaut() {
        let dir = std::env::temp_dir().join(format!("rustudio-routes-{}", std::process::id()));
        let pref = dir.join("sorties-auto.txt");
        assert!(enabled(&pref), "sans fichier : active");
        set_enabled(&pref, false).unwrap();
        assert!(!enabled(&pref));
        set_enabled(&pref, true).unwrap();
        assert!(enabled(&pref));
        let _ = std::fs::remove_dir_all(dir);
    }
}
