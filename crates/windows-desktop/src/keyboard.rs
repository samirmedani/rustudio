//! Touches de la fenêtre en usages HID ; capture limitée au fil graphique, sans hook global.
// Utilisés sous Windows (capture clavier, libellés) et dans les tests ; ailleurs, rien ne les appelle.
#[cfg_attr(not(windows), allow(dead_code))]
const SCANS: &[(u32, u16)] = &[
    (1, 41),
    (2, 30),
    (3, 31),
    (4, 32),
    (5, 33),
    (6, 34),
    (7, 35),
    (8, 36),
    (9, 37),
    (10, 38),
    (11, 39),
    (12, 45),
    (13, 46),
    (14, 42),
    (15, 43),
    (16, 20),
    (17, 26),
    (18, 8),
    (19, 21),
    (20, 23),
    (21, 28),
    (22, 24),
    (23, 12),
    (24, 18),
    (25, 19),
    (26, 47),
    (27, 48),
    (28, 40),
    (29, 224),
    (30, 4),
    (31, 22),
    (32, 7),
    (33, 9),
    (34, 10),
    (35, 11),
    (36, 13),
    (37, 14),
    (38, 15),
    (39, 51),
    (40, 52),
    (41, 53),
    (42, 225),
    (43, 49),
    (44, 29),
    (45, 27),
    (46, 6),
    (47, 25),
    (48, 5),
    (49, 17),
    (50, 16),
    (51, 54),
    (52, 55),
    (53, 56),
    (54, 229),
    (55, 85),
    (56, 226),
    (57, 44),
    (58, 57),
    (59, 58),
    (60, 59),
    (61, 60),
    (62, 61),
    (63, 62),
    (64, 63),
    (65, 64),
    (66, 65),
    (67, 66),
    (68, 67),
    (69, 83),
    (70, 71),
    (71, 95),
    (72, 96),
    (73, 97),
    (74, 86),
    (75, 92),
    (76, 93),
    (77, 94),
    (78, 87),
    (79, 89),
    (80, 90),
    (81, 91),
    (82, 98),
    (83, 99),
    (86, 100),
    (87, 68),
    (88, 69),
    (0xe01c, 88),
    (0xe045, 83),
    (0xe01d, 228),
    (0xe035, 84),
    (0xe038, 230),
    (0xe047, 74),
    (0xe048, 82),
    (0xe049, 75),
    (0xe04b, 80),
    (0xe04d, 79),
    (0xe04f, 77),
    (0xe050, 81),
    (0xe051, 78),
    (0xe052, 73),
    (0xe053, 76),
    (0xe05b, 227),
    (0xe05c, 231),
    (0xe05d, 101),
];

#[cfg_attr(not(windows), allow(dead_code))]
fn physical(scan: u32, vkey: u32) -> Option<u16> {
    match vkey {
        0x13 => Some(72), // Pause : séquence E1, distincte du verrou numérique.
        0x2c => Some(70), // Impr. écran : séquence étendue avec Maj synthétique.
        0x70..=0x7b => Some(58 + (vkey - 0x70) as u16),
        0x7c..=0x87 => Some(104 + (vkey - 0x7c) as u16),
        _ => SCANS.iter().find(|(s, _)| *s == scan).map(|(_, key)| *key),
    }
}

pub fn usage(key: &str) -> Option<u16> {
    let fixed = match key {
        "space" => Some(44),
        "enter" => Some(40),
        "escape" => Some(41),
        "backspace" => Some(42),
        "tab" => Some(43),
        "up" => Some(82),
        "down" => Some(81),
        "left" => Some(80),
        "right" => Some(79),
        "home" => Some(74),
        "end" => Some(77),
        "pageup" => Some(75),
        "pagedown" => Some(78),
        "insert" => Some(73),
        "delete" => Some(76),
        "menu" => Some(101),
        _ => None,
    };
    if fixed.is_some() {
        return fixed;
    }
    if let Some(n) = key
        .strip_prefix('f')
        .and_then(|s| s.parse::<u16>().ok())
        .filter(|n| (1..=12).contains(n))
    {
        return Some(57 + n);
    }
    #[cfg(windows)]
    {
        use windows::Win32::UI::Input::KeyboardAndMouse::{
            MAPVK_VK_TO_VSC_EX, MapVirtualKeyW, VkKeyScanW,
        };
        let mut chars = key.chars();
        let c = chars.next()?;
        if chars.next().is_some() || c as u32 > 65535 {
            return None;
        }
        let vk = unsafe { VkKeyScanW(c as u16) };
        if vk == -1 {
            return None;
        }
        let scan = unsafe { MapVirtualKeyW((vk as u32) & 255, MAPVK_VK_TO_VSC_EX) };
        SCANS.iter().find(|(s, _)| *s == scan).map(|(_, u)| *u)
    }
    #[cfg(not(windows))]
    {
        let c = key.as_bytes();
        if c.len() == 1 && c[0].is_ascii_lowercase() {
            Some((c[0] - b'a') as u16 + 4)
        } else {
            None
        }
    }
}

pub fn label(usage: u16) -> Option<String> {
    #[cfg(windows)]
    {
        use windows::Win32::UI::Input::KeyboardAndMouse::GetKeyNameTextW;
        let scan = SCANS.iter().find(|(_, u)| *u == usage)?.0;
        let mut name = [0u16; 64];
        let extended = if scan & 0xff00 != 0 { 1 << 24 } else { 0 };
        let n = unsafe { GetKeyNameTextW((((scan & 255) << 16) | extended) as i32, &mut name) };
        if n > 0 {
            Some(String::from_utf16_lossy(&name[..n as usize]))
        } else {
            None
        }
    }
    #[cfg(not(windows))]
    {
        let _ = usage;
        None
    }
}

/// État des modificateurs de la fenêtre, côtés gauche/droit conservés (AltGr compris).
pub fn modifiers() -> Vec<u16> {
    #[cfg(windows)]
    {
        use windows::Win32::UI::Input::KeyboardAndMouse::GetKeyState;
        [
            (0xa2, 224),
            (0xa0, 225),
            (0xa4, 226),
            (0x5b, 227),
            (0xa3, 228),
            (0xa1, 229),
            (0xa5, 230),
            (0x5c, 231),
        ]
        .into_iter()
        .filter(|(vk, _)| unsafe { GetKeyState(*vk) } < 0)
        .map(|(_, hid)| hid)
        .collect()
    }
    #[cfg(not(windows))]
    {
        vec![]
    }
}

#[cfg(windows)]
mod capture {
    use super::physical;
    use std::cell::RefCell;
    use windows::Win32::{
        Foundation::{HWND, LPARAM, LRESULT, WPARAM},
        System::Threading::GetCurrentThreadId,
        UI::WindowsAndMessaging::*,
    };
    #[derive(Clone, Copy)]
    pub struct Stroke {
        pub key: u16,
        pub down: bool,
        pub time: u32,
    }
    thread_local! { static INPUT: RefCell<(HWND,Vec<Stroke>)> = const {RefCell::new((HWND(std::ptr::null_mut()),Vec::new()))}; }
    unsafe extern "system" fn message(code: i32, w: WPARAM, l: LPARAM) -> LRESULT {
        if code >= 0 && w.0 == PM_REMOVE.0 as usize && l.0 != 0 {
            let msg = unsafe { &*(l.0 as *const MSG) };
            INPUT.with(|input| {
                let Ok(mut input) = input.try_borrow_mut() else {
                    return;
                };
                if msg.hwnd != input.0 {
                    return;
                }
                let down = matches!(msg.message, WM_KEYDOWN | WM_SYSKEYDOWN);
                if !down && !matches!(msg.message, WM_KEYUP | WM_SYSKEYUP) {
                    return;
                }
                let bits = msg.lParam.0 as u32;
                if down && bits & (1 << 30) != 0 {
                    return;
                }
                let mut scan = (bits >> 16) & 255 | if bits & (1 << 24) != 0 { 0xe000 } else { 0 };
                // Les messages synthétiques de test peuvent ne pas contenir de scan code.
                if scan & 255 == 0 {
                    use windows::Win32::UI::Input::KeyboardAndMouse::{
                        MAPVK_VK_TO_VSC_EX, MapVirtualKeyW,
                    };
                    scan = unsafe { MapVirtualKeyW(msg.wParam.0 as u32, MAPVK_VK_TO_VSC_EX) }
                        | (scan & 0xe000);
                }
                if let Some(key) = physical(scan, msg.wParam.0 as u32) {
                    if input.1.len() < 160 {
                        input.1.push(Stroke {
                            key,
                            down,
                            time: msg.time,
                        });
                    }
                }
            });
        }
        unsafe { CallNextHookEx(None, code, w, l) }
    }
    /// Doit être créé et détruit sur le fil graphique. Aucune interception des autres processus.
    pub struct Capture {
        hook: HHOOK,
    }
    impl Capture {
        pub fn start(window: isize) -> Result<Self, String> {
            let hwnd = HWND(window as *mut _);
            if hwnd.0.is_null()
                || unsafe { GetWindowThreadProcessId(hwnd, None) }
                    != unsafe { GetCurrentThreadId() }
            {
                return Err("Active la fenêtre Rustudio pour enregistrer.".into());
            }
            INPUT.with(|i| *i.borrow_mut() = (hwnd, Vec::new()));
            let hook = unsafe {
                SetWindowsHookExW(WH_GETMESSAGE, Some(message), None, GetCurrentThreadId())
            }
            .map_err(|e| format!("Capture des touches indisponible : {e}"))?;
            Ok(Self { hook })
        }
        pub fn events(&self) -> Vec<Stroke> {
            INPUT.with(|i| std::mem::take(&mut i.borrow_mut().1))
        }
    }
    impl Drop for Capture {
        fn drop(&mut self) {
            let _ = unsafe { UnhookWindowsHookEx(self.hook) };
            INPUT.with(|i| *i.borrow_mut() = (HWND(std::ptr::null_mut()), Vec::new()));
        }
    }
}
#[cfg(windows)]
pub use capture::Capture;

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn physical_keys_preserve_keypad_and_modifier_sides() {
        assert_eq!(physical(0x1c, 0x0d), Some(40));
        assert_eq!(physical(0xe01c, 0x0d), Some(88));
        assert_eq!(physical(0x02, 0x31), Some(30));
        assert_eq!(physical(0x4f, 0x61), Some(89));
        assert_eq!(physical(0x1e, 0x51), Some(4)); // Q en AZERTY = usage physique A.
        assert_eq!(physical(0x1d, 0x11), Some(224));
        assert_eq!(physical(0xe01d, 0x11), Some(228));
        assert_eq!(physical(0xe038, 0x12), Some(230));
        assert_eq!(physical(0x45, 0x13), Some(72));
        assert_eq!(physical(0x45, 0x90), Some(83));
        assert_eq!(physical(0, 0x87), Some(115));
        assert_eq!(physical(0, 0), None);
    }
}
