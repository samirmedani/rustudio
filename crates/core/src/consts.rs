//! Constantes du protocole GoXLR Mini (reprises du projet libre goxlr-utility, licence MIT) et de l'interface.

pub const CHANNELS: [&str; 11] = [
    "Mic", "LineIn", "Console", "System", "Game", "Chat", "Sample", "Music", "Headphones", "MicMonitor", "LineOut",
];
pub const CHANNEL_LABELS: [(&str, &str); 11] = [
    ("Mic", "Micro"), ("LineIn", "Entrée ligne"), ("Console", "Console"), ("System", "Système"), ("Game", "Jeu"),
    ("Chat", "Chat"), ("Sample", "Samples"), ("Music", "Musique"), ("Headphones", "Casque"),
    ("MicMonitor", "Retour micro"), ("LineOut", "Sortie ligne"),
];
pub const CHANNEL_COLOURS: [(&str, &str); 11] = [
    ("Mic", "FF453A"), ("LineIn", "BF5AF2"), ("Console", "FFD60A"), ("System", "64D2FF"), ("Game", "30D158"),
    ("Chat", "5E5CE6"), ("Sample", "FF9F0A"), ("Music", "FF375F"), ("Headphones", "F2F2F7"),
    ("MicMonitor", "FF6961"), ("LineOut", "AC8E68"),
];
pub const FADERS: [&str; 4] = ["A", "B", "C", "D"];
/// entree -> (id gauche, id droit)
pub const INPUTS: [(&str, u32, u32); 8] = [
    ("Microphone", 0x02, 0x03), ("LineIn", 0x04, 0x05), ("Console", 0x06, 0x07), ("System", 0x08, 0x09),
    ("Game", 0x0A, 0x0B), ("Chat", 0x0C, 0x0D), ("Music", 0x0E, 0x0F), ("Samples", 0x10, 0x11),
];
/// entree -> voie
pub const IN_CHANNEL: [(&str, &str); 8] = [
    ("Microphone", "Mic"), ("LineIn", "LineIn"), ("Console", "Console"), ("System", "System"), ("Game", "Game"),
    ("Chat", "Chat"), ("Music", "Music"), ("Samples", "Sample"),
];
/// sortie -> (position gauche, position droite)
pub const OUTPUTS: [(&str, usize, usize); 6] = [
    ("Headphones", 1, 3), ("BroadcastMix", 5, 7), ("ChatMic", 9, 11), ("Sampler", 13, 15), ("LineOut", 17, 19),
    ("StreamMix2", 21, 23),
];
pub const MIX_OUTPUT_ORDER: [&str; 6] = ["Headphones", "BroadcastMix", "ChatMic", "Sampler", "LineOut", "StreamMix2"];
/// carte couleurs Mini >= 1.1.8 (520 octets) : cible -> (index de depart, nombre de couleurs)
pub const COLOUR_TARGETS: [(&str, usize, usize); 12] = [
    ("Global", 8, 2), ("Accent", 10, 2), ("Fader1Mute", 12, 2), ("Fader2Mute", 14, 2), ("Fader3Mute", 16, 2),
    ("Fader4Mute", 18, 2), ("FadeMeter1", 20, 2), ("FadeMeter2", 34, 2), ("FadeMeter3", 48, 2),
    ("FadeMeter4", 62, 2), ("Bleep", 126, 2), ("MicrophoneMute", 128, 2),
];
pub const COLOUR_ALIASES: [(&str, &str); 4] =
    [("global", "Global"), ("InternalLight", "Global"), ("LogoX", "Accent"), ("Cough", "MicrophoneMute")];
pub const BUTTON_BITS: [(&str, u32); 6] = [
    ("Fader1Mute", 4), ("Fader2Mute", 9), ("Fader3Mute", 14), ("Fader4Mute", 19), ("Bleep", 22),
    ("MicrophoneMute", 23),
];
pub const FADER_BUTTONS: [&str; 4] = ["Fader1Mute", "Fader2Mute", "Fader3Mute", "Fader4Mute"];
pub const BTN_ON: u8 = 0x01;
pub const BTN_DIM: u8 = 0x02;
pub const BTN_FLASH: u8 = 0x03;
pub const STATUS_STALL: u32 = 4009754628;
pub const MIC_TYPES: [&str; 3] = ["dynamic", "condenser", "jack"];
pub const EQ_BANDS: [(&str, u32, u32); 6] = [
    ("90", 0x40000, 0x40001), ("250", 0x40003, 0x40004), ("500", 0x40006, 0x40007), ("1000", 0x50000, 0x50001),
    ("3000", 0x50003, 0x50004), ("8000", 0x50006, 0x50007),
];
pub const RATIOS: [f64; 15] = [1.0, 1.1, 1.2, 1.4, 1.6, 1.8, 2.0, 2.5, 3.2, 4.0, 5.6, 8.0, 16.0, 32.0, 64.0];
pub const GATE_ATTENUATION: [i32; 26] = [
    -6, -7, -8, -9, -10, -11, -12, -13, -14, -15, -16, -17, -18, -19, -20, -21, -22, -23, -24, -25, -26, -27, -28,
    -30, -32, -61,
];
pub const FADER_STYLES: [(&str, bool, bool); 4] =
    [("two-colour", false, false), ("gradient", true, false), ("meter", false, true), ("gradient-meter", true, true)];
pub const ANIMATIONS: [(&str, u8); 6] =
    [("none", 5), ("retro-rainbow", 0), ("rainbow-dark", 1), ("rainbow-bright", 2), ("simple", 3), ("ripple", 4)];
pub const WATERFALL: [(&str, u8); 3] = [("down", 0), ("up", 1), ("off", 2)];
pub const RECOMMENDED_GAIN: [(&str, i32); 3] = [("dynamic", 50), ("condenser", 30), ("jack", 25)];
pub const VOICE_TARGET_DB: f64 = -12.0;
pub const CLIP_DB: f64 = -1.5;
pub const USB_MINI: &str = "USB\\VID_1220&PID_8FE4\\";
pub const USB_FULL: &str = "USB\\VID_1220&PID_8FE0\\";
pub const MUTE_TARGETS: [(&str, &[&str]); 5] = [
    ("all", &[]), ("stream", &["BroadcastMix", "StreamMix2"]), ("chat", &["ChatMic"]), ("phones", &["Headphones"]),
    ("lineout", &["LineOut"]),
];
pub const MUTE_TARGET_LABELS: [(&str, &str); 5] = [
    ("all", "partout"), ("stream", "du stream"), ("chat", "de Discord"), ("phones", "du casque"),
    ("lineout", "des enceintes"),
];
pub const MIC_BUTTON_MODES: [&str; 3] = ["toggle", "hold", "ptt"];
pub const VOD_MODES: [&str; 2] = ["off", "no-music"];

pub struct Theme {
    pub key: &'static str,
    pub label: &'static str,
    pub per_channel: bool,
    pub top: &'static str,
    pub bottom: &'static str,
    pub faders: Option<[&'static str; 4]>,
    pub accent: &'static str,
}
pub const THEMES: [Theme; 7] = [
    Theme { key: "voies", label: "Couleur des voies", per_channel: true, top: "", bottom: "", faders: None, accent: "F2F2F7" },
    Theme { key: "glacier", label: "Glacier", per_channel: false, top: "00E5FF", bottom: "00303A", faders: None, accent: "00E5FF" },
    Theme { key: "aurore", label: "Aurore", per_channel: false, top: "", bottom: "", faders: Some(["00F5D4", "7B61FF", "FF4FD8", "FFB800"]), accent: "7B61FF" },
    Theme { key: "sunset", label: "Coucher de soleil", per_channel: false, top: "FF7A00", bottom: "4A0020", faders: None, accent: "FF2D78" },
    Theme { key: "neon", label: "Néon", per_channel: false, top: "FF00E5", bottom: "12003A", faders: None, accent: "00FFF0" },
    Theme { key: "blanc", label: "Blanc pur", per_channel: false, top: "FFFFFF", bottom: "2A2A2A", faders: None, accent: "FFFFFF" },
    Theme { key: "nuit", label: "Nuit", per_channel: false, top: "3A5BFF", bottom: "00000C", faders: None, accent: "1E2E80" },
];

/// Applis reconnues pour les scenes automatiques (processus -> nom affiche)
pub const KNOWN_APPS: &[(&str, &str)] = &[
    ("obs64.exe", "OBS Studio"), ("streamlabs obs.exe", "Streamlabs Desktop"), ("twitchstudio.exe", "Twitch Studio"),
    ("discord.exe", "Discord"), ("teamspeak.exe", "TeamSpeak"), ("ts3client_win64.exe", "TeamSpeak 3"),
    ("spotify.exe", "Spotify"), ("vlc.exe", "VLC"), ("steam.exe", "Steam"), ("steamwebhelper.exe", "Steam (navigateur)"),
    ("epicgameslauncher.exe", "Epic Games"), ("battle.net.exe", "Battle.net"), ("riotclientservices.exe", "Riot Client"),
    ("valorant-win64-shipping.exe", "Valorant"), ("cs2.exe", "Counter-Strike 2"), ("league of legends.exe", "League of Legends"),
    ("fortniteclient-win64-shipping.exe", "Fortnite"), ("r5apex.exe", "Apex Legends"), ("r5apex_dx12.exe", "Apex Legends"),
    ("rocketleague.exe", "Rocket League"), ("gta5.exe", "GTA V"), ("gta5_enhanced.exe", "GTA V"), ("minecraft.windows.exe", "Minecraft"),
    ("javaw.exe", "Minecraft Java"), ("eldenring.exe", "Elden Ring"), ("overwatch.exe", "Overwatch"),
    ("zoom.exe", "Zoom"), ("ms-teams.exe", "Teams"), ("chrome.exe", "Chrome"), ("msedge.exe", "Edge"), ("firefox.exe", "Firefox"),
    ("rustudio.exe", "Rustudio"), ("wavelink.exe", "Elgato Wave Link"), ("nvidia broadcast.exe", "NVIDIA Broadcast"),
    ("voicemeeter.exe", "Voicemeeter"),
];
pub const SYSTEM_PROCESSES: &[&str] = &[
    "[system process]", "system", "registry", "smss.exe", "csrss.exe", "wininit.exe", "winlogon.exe", "services.exe",
    "lsass.exe", "svchost.exe", "fontdrvhost.exe", "dwm.exe", "memory compression", "spoolsv.exe", "sihost.exe",
    "taskhostw.exe", "ctfmon.exe", "explorer.exe", "searchhost.exe", "startmenuexperiencehost.exe", "runtimebroker.exe",
    "conhost.exe", "dllhost.exe", "wmiprvse.exe", "audiodg.exe", "securityhealthservice.exe", "msmpeng.exe",
    "nissrv.exe", "smartscreen.exe", "textinputhost.exe", "lockapp.exe", "shellexperiencehost.exe", "sshd.exe",
    "sshd-session.exe", "powershell.exe", "pwsh.exe", "cmd.exe", "python.exe", "pythonw.exe", "rustudio.exe",
    "rustudio-engine.exe", "msedgewebview2.exe", "searchindexer.exe", "wudfhost.exe", "useroobebroker.exe",
    "applicationframehost.exe", "goxlr mini.exe", "rustudio-setup.exe", "wscript.exe",
];

pub fn channel_index(name: &str) -> Option<usize> {
    CHANNELS.iter().position(|c| *c == name)
}
/// voie connue, sans tenir compte de la casse
pub fn channel_named(name: &str) -> Option<&'static str> {
    CHANNELS.iter().copied().find(|c| c.eq_ignore_ascii_case(name.trim()))
}
pub fn channel_label(ch: &str) -> &'static str {
    CHANNEL_LABELS.iter().find(|(c, _)| *c == ch).map(|(_, l)| *l).unwrap_or("?")
}
pub fn channel_colour(ch: &str) -> &'static str {
    CHANNEL_COLOURS.iter().find(|(c, _)| *c == ch).map(|(_, l)| *l).unwrap_or("FFFFFF")
}
pub fn is_fader(f: &str) -> bool {
    FADERS.contains(&f)
}
pub fn fader_index(f: &str) -> Option<usize> {
    FADERS.iter().position(|x| *x == f)
}
pub fn is_input(i: &str) -> bool {
    INPUTS.iter().any(|(n, _, _)| *n == i)
}
pub fn input_ids(i: &str) -> Option<(u32, u32)> {
    INPUTS.iter().find(|(n, _, _)| *n == i).map(|(_, l, r)| (*l, *r))
}
pub fn is_output(o: &str) -> bool {
    OUTPUTS.iter().any(|(n, _, _)| *n == o)
}
pub fn output_pos(o: &str) -> Option<(usize, usize)> {
    OUTPUTS.iter().find(|(n, _, _)| *n == o).map(|(_, l, r)| (*l, *r))
}
pub fn in_channel(inp: &str) -> Option<&'static str> {
    IN_CHANNEL.iter().find(|(i, _)| *i == inp).map(|(_, c)| *c)
}
/// voie -> entree (seulement les voies qui sont des entrees)
pub fn channel_input(ch: &str) -> Option<&'static str> {
    IN_CHANNEL.iter().find(|(_, c)| *c == ch).map(|(i, _)| *i)
}
pub fn colour_target(t: &str) -> Option<(usize, usize)> {
    COLOUR_TARGETS.iter().find(|(n, _, _)| *n == t).map(|(_, s, c)| (*s, *c))
}
pub fn colour_alias(t: &str) -> &str {
    COLOUR_ALIASES.iter().find(|(a, _)| *a == t).map(|(_, b)| *b).unwrap_or(t)
}
pub fn button_bit(b: &str) -> Option<u32> {
    BUTTON_BITS.iter().find(|(n, _)| *n == b).map(|(_, v)| *v)
}
pub fn is_mic_type(t: &str) -> bool {
    MIC_TYPES.contains(&t)
}
pub fn mic_gain_key(t: &str) -> u32 {
    match t {
        "condenser" => 0x002,
        "jack" => 0x003,
        _ => 0x001,
    }
}
pub fn recommended_gain(t: &str) -> i32 {
    RECOMMENDED_GAIN.iter().find(|(k, _)| *k == t).map(|(_, v)| *v).unwrap_or(30)
}
pub fn is_eq_band(b: &str) -> bool {
    EQ_BANDS.iter().any(|(n, _, _)| *n == b)
}
pub fn fader_style(s: &str) -> Option<(bool, bool)> {
    FADER_STYLES.iter().find(|(n, _, _)| *n == s).map(|(_, g, m)| (*g, *m))
}
pub fn animation(a: &str) -> Option<u8> {
    ANIMATIONS.iter().find(|(n, _)| *n == a).map(|(_, v)| *v)
}
pub fn waterfall(w: &str) -> Option<u8> {
    WATERFALL.iter().find(|(n, _)| *n == w).map(|(_, v)| *v)
}
pub fn mute_target(t: &str) -> Option<&'static [&'static str]> {
    MUTE_TARGETS.iter().find(|(n, _)| *n == t).map(|(_, o)| *o)
}
pub fn mute_target_label(t: &str) -> &'static str {
    MUTE_TARGET_LABELS.iter().find(|(n, _)| *n == t).map(|(_, l)| *l).unwrap_or("partout")
}
pub fn theme(name: &str) -> Option<&'static Theme> {
    THEMES.iter().find(|t| t.key == name)
}
pub fn known_app(exe: &str) -> Option<&'static str> {
    KNOWN_APPS.iter().find(|(e, _)| *e == exe).map(|(_, l)| *l)
}
