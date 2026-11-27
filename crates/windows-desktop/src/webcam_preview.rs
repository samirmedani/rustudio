//! Aperçu vidéo en temps réel de la Logitech C920 (strictement vidéo, sans jamais ouvrir le micro).
//! Sous Windows : Media Foundation (IMFSourceReader) avec conversion matérielle RGB32 (BGRA) directe.
//! L'ouverture et la fermeture du flux sont sérialisées par un verrou global pour ne jamais bloquer l'interface
//! ni entrer en conflit avec OBS ou Discord dès qu'on quitte la page webcam ou met l'aperçu en pause.
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Status {
    Idle,
    Starting,
    Live { width: u32, height: u32, fps: u32 },
    /// Le flux vidéo est déjà pris en exclusivité par OBS, Discord ou une autre appli.
    Busy(String),
    /// Aucune Logitech C920 vidéo n'a été trouvée par Media Foundation.
    Disconnected,
    Error(String),
}

#[derive(Clone)]
pub struct PreviewFrame {
    pub seq: u64,
    pub width: u32,
    pub height: u32,
    /// Pixels BGRA (B, G, R, A=255) prêts pour gpui::RenderImage.
    pub bgra: Arc<Vec<u8>>,
    pub fps: u32,
}

struct SharedState {
    status: Status,
    frame: Option<PreviewFrame>,
}

impl Default for SharedState {
    fn default() -> Self {
        Self { status: Status::Idle, frame: None }
    }
}

pub struct WebcamPreview {
    stop: Arc<AtomicBool>,
    state: Arc<Mutex<SharedState>>,
    active: bool,
}

impl Default for WebcamPreview {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for WebcamPreview {
    fn drop(&mut self) {
        self.stop();
    }
}

impl WebcamPreview {
    pub fn new() -> Self {
        Self {
            stop: Arc::new(AtomicBool::new(true)),
            state: Arc::new(Mutex::new(SharedState::default())),
            active: false,
        }
    }

    pub fn is_active(&self) -> bool {
        self.active
    }

    /// Démarre la capture vidéo en arrière-plan (sans effet si déjà active).
    pub fn start(&mut self) {
        if self.active {
            return;
        }
        self.active = true;
        let stop = Arc::new(AtomicBool::new(false));
        self.stop = stop.clone();
        let state = self.state.clone();
        if let Ok(mut s) = state.lock() {
            s.status = Status::Starting;
        }
        #[cfg(windows)]
        {
            let _ = std::thread::Builder::new()
                .name("c920-preview".into())
                .spawn(move || win::run(stop, state));
        }
        #[cfg(not(windows))]
        {
            let _ = stop;
            if let Ok(mut s) = state.lock() {
                s.status = Status::Idle;
            };
        }
    }

    /// Arrête immédiatement la capture et libère la caméra pour OBS / Discord (non bloquant pour l'UI).
    pub fn stop(&mut self) {
        if !self.active {
            return;
        }
        self.active = false;
        self.stop.store(true, Ordering::SeqCst);
        if let Ok(mut s) = self.state.lock() {
            s.status = Status::Idle;
            s.frame = None;
        }
    }

    /// Redémarre proprement la capture (ex. bouton « Réessayer » quand OBS vient de libérer la caméra).
    pub fn restart(&mut self) {
        self.stop();
        self.start();
    }

    /// Renvoie l'état courant et la dernière image capturée (clonage O(1) de l'Arc).
    pub fn latest(&self) -> (Status, Option<PreviewFrame>) {
        match self.state.lock() {
            Ok(s) => (s.status.clone(), s.frame.clone()),
            Err(_) => (Status::Idle, None),
        }
    }
}

/// Reconnaît les liens symboliques vidéo Windows d'une Logitech C920 (jamais l'interface audio).
pub fn is_c920_video_symlink(link: &str) -> bool {
    let l = link.to_ascii_lowercase();
    l.contains("vid_046d") && (l.contains("pid_0892") || l.contains("pid_082d") || l.contains("pid_08e5"))
}

/// Copie une image RGB32 (BGRA, stride signé : négatif = bas en haut) dans un tampon BGRA contigu (alpha = 255).
/// # Safety
/// `src_scanline0` doit pointer sur le début de la ligne 0 d'une image de `height` lignes, avec un écart de
/// `pitch` octets entre deux lignes consécutives (négatif si stockée de bas en haut) et au moins `width * 4`
/// octets lisibles par ligne.
pub unsafe fn copy_rgb32_to_bgra(
    src_scanline0: *const u8,
    pitch: isize,
    width: usize,
    height: usize,
    dst: &mut Vec<u8>,
) {
    let row_bytes = width.saturating_mul(4);
    let total = row_bytes.saturating_mul(height);
    dst.resize(total, 0);
    if row_bytes == 0 || height == 0 {
        return;
    }
    for y in 0..height {
        let src_row = unsafe { src_scanline0.offset(pitch * y as isize) };
        let dst_row = &mut dst[y * row_bytes..(y + 1) * row_bytes];
        unsafe {
            std::ptr::copy_nonoverlapping(src_row, dst_row.as_mut_ptr(), row_bytes);
        }
        for px in dst_row.chunks_exact_mut(4) {
            px[3] = 255;
        }
    }
}

/// Retourne horizontalement une image BGRA (effet miroir local, ne touche jamais le flux OBS).
pub fn flip_horizontal_bgra(width: usize, height: usize, buf: &[u8]) -> Vec<u8> {
    let row_bytes = width.saturating_mul(4);
    if buf.len() < row_bytes.saturating_mul(height) || width == 0 {
        return buf.to_vec();
    }
    let mut out = vec![0u8; row_bytes * height];
    for y in 0..height {
        let src = &buf[y * row_bytes..(y + 1) * row_bytes];
        let dst = &mut out[y * row_bytes..(y + 1) * row_bytes];
        for x in 0..width {
            let s = (width - 1 - x) * 4;
            let d = x * 4;
            dst[d..d + 4].copy_from_slice(&src[s..s + 4]);
        }
    }
    out
}

/// Choisit la meilleure résolution native pour l'aperçu (16:9 fluide entre 640 et 960 px de large, léger en CPU).
pub fn score_native_mode(width: u32, height: u32) -> i32 {
    if width < 320 || height < 180 {
        return -10_000;
    }
    let is_16_9 = (width * 9).abs_diff(height * 16) <= height;
    let mut score = if is_16_9 { 2_000 } else { 0 };
    // Préférence : 640x360, 800x448, 864x480 ou 960x540 (net dans la carte aperçu, faible charge USB et CPU)
    if (640..=960).contains(&width) {
        score += 1_500 - (width as i32 - 800).abs();
    } else if width > 960 {
        score += 400 - ((width as i32 - 960) / 4);
    } else {
        score += width as i32;
    }
    score
}

/// Sonde silencieuse d'une seule image vidéo (test CLI sur PC Windows, strictement vidéo, zéro son).
#[cfg(windows)]
pub fn probe_one_frame() -> Result<(u32, u32), String> {
    win::probe_one_frame()
}

#[cfg(not(windows))]
pub fn probe_one_frame() -> Result<(u32, u32), String> {
    Err("L'aperçu Media Foundation n'est disponible que sous Windows.".into())
}

#[cfg(windows)]
mod win {
    use super::{PreviewFrame, SharedState, Status, copy_rgb32_to_bgra, is_c920_video_symlink, score_native_mode};
    use std::sync::{
        Arc, Mutex, OnceLock,
        atomic::{AtomicBool, Ordering},
    };
    use std::time::{Duration, Instant};
    use windows::Win32::Media::MediaFoundation::{
        IMF2DBuffer, IMFActivate, IMFAttributes, IMFMediaSource, IMFMediaType, IMFSourceReader, MF_DEVSOURCE_ATTRIBUTE_SOURCE_TYPE,
        MF_DEVSOURCE_ATTRIBUTE_SOURCE_TYPE_VIDCAP_GUID, MF_DEVSOURCE_ATTRIBUTE_SOURCE_TYPE_VIDCAP_SYMBOLIC_LINK,
        MF_MT_DEFAULT_STRIDE, MF_MT_FRAME_SIZE, MF_MT_MAJOR_TYPE, MF_MT_SUBTYPE,
        MF_SOURCE_READER_ENABLE_VIDEO_PROCESSING, MF_SOURCE_READER_FIRST_VIDEO_STREAM,
        MF_SOURCE_READERF_CURRENTMEDIATYPECHANGED, MF_SOURCE_READERF_ENDOFSTREAM, MF_SOURCE_READERF_STREAMTICK,
        MF_VERSION, MFCreateAttributes, MFCreateMediaType, MFCreateSourceReaderFromMediaSource, MFEnumDeviceSources,
        MFMediaType_Video, MFSTARTUP_NOSOCKET, MFShutdown, MFStartup, MFVideoFormat_RGB32,
    };
    use windows::Win32::System::Com::{COINIT_MULTITHREADED, CoInitializeEx, CoTaskMemFree, CoUninitialize};
    use windows_core::{Interface, PWSTR};

    /// Garantit qu'une seule session Media Foundation ouvre la C920 à la fois.
    fn capture_slot() -> &'static Mutex<()> {
        static SLOT: OnceLock<Mutex<()>> = OnceLock::new();
        SLOT.get_or_init(|| Mutex::new(()))
    }

    struct ComMfGuard {
        com_ok: bool,
        mf_ok: bool,
    }
    impl ComMfGuard {
        fn init() -> Result<Self, String> {
            unsafe {
                let hr = CoInitializeEx(None, COINIT_MULTITHREADED);
                let com_ok = hr.is_ok();
                MFStartup(MF_VERSION, MFSTARTUP_NOSOCKET)
                    .map_err(|e| format!("Media Foundation indisponible : {e}"))?;
                Ok(Self { com_ok, mf_ok: true })
            }
        }
    }
    impl Drop for ComMfGuard {
        fn drop(&mut self) {
            unsafe {
                if self.mf_ok {
                    let _ = MFShutdown();
                }
                if self.com_ok {
                    CoUninitialize();
                }
            }
        }
    }

    struct OpenedCamera {
        source: IMFMediaSource,
        reader: IMFSourceReader,
        width: u32,
        height: u32,
        default_stride: i32,
    }

    impl Drop for OpenedCamera {
        fn drop(&mut self) {
            // Libère le lecteur avant de couper la source pour éteindre la LED et rendre la caméra immédiatement.
            unsafe {
                let _ = self.source.Shutdown();
            }
        }
    }

    fn read_dims(mt: &IMFMediaType) -> Option<(u32, u32)> {
        let packed = unsafe { mt.GetUINT64(&MF_MT_FRAME_SIZE).ok()? };
        let w = (packed >> 32) as u32;
        let h = (packed & 0xFFFF_FFFF) as u32;
        (w > 0 && h > 0).then_some((w, h))
    }

    fn read_stride(mt: &IMFMediaType, width: u32) -> i32 {
        unsafe { mt.GetUINT32(&MF_MT_DEFAULT_STRIDE).map(|v| v as i32).unwrap_or((width * 4) as i32) }
    }

    fn is_busy_hresult(code: u32) -> bool {
        matches!(
            code,
            0xC00D_3704 // MF_E_HW_MFT_FAILED_START_STREAMING
                | 0xC00D_ABE0 // MF_E_VIDEO_RECORDING_DEVICE_INVALIDATED / busy
                | 0xC00D_36B5 // MF_E_NOTACCEPTING
                | 0x8007_0005 // E_ACCESSDENIED
                | 0x8007_0020 // ERROR_SHARING_VIOLATION
                | 0x8007_00AA // ERROR_BUSY
                | 0x8007_0490 // ERROR_NOT_FOUND
        )
    }

    fn classify_err(err: &windows_core::Error, context: &str) -> Status {
        let code = err.code().0 as u32;
        if is_busy_hresult(code) {
            Status::Busy("Flux vidéo utilisé par OBS, Discord ou une autre appli : tes réglages UVC agissent quand même en direct.".into())
        } else {
            Status::Error(format!("{context} (0x{code:08X})"))
        }
    }

    fn find_c920_activate() -> Result<Option<IMFActivate>, String> {
        unsafe {
            let mut attrs: Option<IMFAttributes> = None;
            MFCreateAttributes(&mut attrs, 1).map_err(|e| format!("Attributs MF : {e}"))?;
            let attrs = attrs.ok_or("Attributs MF vides")?;
            // STRICTEMENT VIDCAP (vidéo uniquement, jamais l'interface audio de la webcam).
            attrs
                .SetGUID(&MF_DEVSOURCE_ATTRIBUTE_SOURCE_TYPE, &MF_DEVSOURCE_ATTRIBUTE_SOURCE_TYPE_VIDCAP_GUID)
                .map_err(|e| format!("Filtre vidéo MF : {e}"))?;
            let mut raw_ptr: *mut Option<IMFActivate> = std::ptr::null_mut();
            let mut count: u32 = 0;
            MFEnumDeviceSources(&attrs, &mut raw_ptr, &mut count)
                .map_err(|e| format!("Énumération vidéo MF : {e}"))?;
            if raw_ptr.is_null() || count == 0 {
                return Ok(None);
            }
            let slice = std::slice::from_raw_parts_mut(raw_ptr, count as usize);
            let mut found = None;
            for slot in slice.iter_mut() {
                if let Some(act) = slot.take() {
                    let mut pwstr = PWSTR::null();
                    let mut len: u32 = 0;
                    if act
                        .GetAllocatedString(&MF_DEVSOURCE_ATTRIBUTE_SOURCE_TYPE_VIDCAP_SYMBOLIC_LINK, &mut pwstr, &mut len)
                        .is_ok()
                        && !pwstr.is_null()
                    {
                        let link = pwstr.to_string().unwrap_or_default();
                        CoTaskMemFree(Some(pwstr.0 as *const _));
                        if found.is_none() && is_c920_video_symlink(&link) {
                            found = Some(act);
                        }
                    }
                }
            }
            CoTaskMemFree(Some(raw_ptr as *const _));
            Ok(found)
        }
    }

    fn open_camera() -> Result<OpenedCamera, Status> {
        let activate = match find_c920_activate() {
            Ok(Some(a)) => a,
            Ok(None) => return Err(Status::Disconnected),
            Err(e) => return Err(Status::Error(e)),
        };
        unsafe {
            let source: IMFMediaSource = activate
                .ActivateObject()
                .map_err(|e| classify_err(&e, "Impossible d'ouvrir la C920"))?;
            let mut reader_attrs: Option<IMFAttributes> = None;
            if MFCreateAttributes(&mut reader_attrs, 1).is_ok() {
                if let Some(ref a) = reader_attrs {
                    let _ = a.SetUINT32(&MF_SOURCE_READER_ENABLE_VIDEO_PROCESSING, 1);
                }
            }
            let reader = MFCreateSourceReaderFromMediaSource(&source, reader_attrs.as_ref())
                .map_err(|e| {
                    let _ = source.Shutdown();
                    classify_err(&e, "Lecteur vidéo C920")
                })?;

            let stream = MF_SOURCE_READER_FIRST_VIDEO_STREAM.0 as u32;
            // Choisit d'abord une résolution native légère (640x360 .. 960x540 en 16:9) si disponible.
            let mut best: Option<(i32, IMFMediaType)> = None;
            for idx in 0..64u32 {
                let Ok(mt) = reader.GetNativeMediaType(stream, idx) else {
                    break;
                };
                if let Some((w, h)) = read_dims(&mt) {
                    let sc = score_native_mode(w, h);
                    if best.as_ref().is_none_or(|(b, _)| sc > *b) {
                        best = Some((sc, mt));
                    }
                }
            }
            if let Some((_, native_mt)) = best {
                let _ = reader.SetCurrentMediaType(stream, None, &native_mt);
            }

            // Demande une sortie RGB32 (BGRA 32 bits) via la conversion matérielle de Media Foundation.
            let rgb_mt = MFCreateMediaType().map_err(|e| {
                let _ = source.Shutdown();
                classify_err(&e, "Type média RGB32")
            })?;
            let _ = rgb_mt.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video);
            let _ = rgb_mt.SetGUID(&MF_MT_SUBTYPE, &MFVideoFormat_RGB32);
            if let Err(e) = reader.SetCurrentMediaType(stream, None, &rgb_mt) {
                let _ = source.Shutdown();
                return Err(classify_err(&e, "Format RGB32 refusé par la C920"));
            }

            let cur = reader.GetCurrentMediaType(stream).map_err(|e| {
                let _ = source.Shutdown();
                classify_err(&e, "Lecture du format actif")
            })?;
            let (width, height) = read_dims(&cur).unwrap_or((640, 360));
            let default_stride = read_stride(&cur, width);
            Ok(OpenedCamera { source, reader, width, height, default_stride })
        }
    }

    fn read_one_sample(cam: &mut OpenedCamera, dst: &mut Vec<u8>) -> Result<bool, Status> {
        let stream = MF_SOURCE_READER_FIRST_VIDEO_STREAM.0 as u32;
        let mut flags: u32 = 0;
        let mut sample = None;
        unsafe {
            cam.reader
                .ReadSample(stream, 0, None, Some(&mut flags), None, Some(&mut sample))
                .map_err(|e| classify_err(&e, "Flux vidéo interrompu"))?;
            if (flags & MF_SOURCE_READERF_CURRENTMEDIATYPECHANGED.0 as u32) != 0 {
                if let Ok(cur) = cam.reader.GetCurrentMediaType(stream) {
                    if let Some((w, h)) = read_dims(&cur) {
                        cam.width = w;
                        cam.height = h;
                        cam.default_stride = read_stride(&cur, w);
                    }
                }
            }
            if (flags & MF_SOURCE_READERF_ENDOFSTREAM.0 as u32) != 0 {
                return Err(Status::Busy("Flux vidéo repris par une autre application.".into()));
            }
            if (flags & MF_SOURCE_READERF_STREAMTICK.0 as u32) != 0 {
                return Ok(false);
            }
            let Some(sample) = sample else {
                return Ok(false);
            };
            let buf = sample
                .ConvertToContiguousBuffer()
                .map_err(|e| classify_err(&e, "Tampon vidéo"))?;
            let (w, h) = (cam.width as usize, cam.height as usize);
            if let Ok(buf2d) = buf.cast::<IMF2DBuffer>() {
                let mut scan0: *mut u8 = std::ptr::null_mut();
                let mut pitch: i32 = 0;
                if buf2d.Lock2D(&mut scan0, &mut pitch).is_ok() && !scan0.is_null() {
                    copy_rgb32_to_bgra(scan0, pitch as isize, w, h, dst);
                    let _ = buf2d.Unlock2D();
                    return Ok(true);
                }
            }
            let mut ptr: *mut u8 = std::ptr::null_mut();
            let mut cur_len: u32 = 0;
            buf.Lock(&mut ptr, None, Some(&mut cur_len))
                .map_err(|e| classify_err(&e, "Verrou tampon vidéo"))?;
            if !ptr.is_null() && (cur_len as usize) >= w * h * 4 {
                let stride = cam.default_stride as isize;
                if stride < 0 {
                    let top_row = ptr.add((h - 1) * (-stride) as usize);
                    copy_rgb32_to_bgra(top_row, stride, w, h, dst);
                } else {
                    let pitch = if stride >= (w * 4) as isize { stride } else { (w * 4) as isize };
                    copy_rgb32_to_bgra(ptr, pitch, w, h, dst);
                }
            }
            let _ = buf.Unlock();
            Ok(true)
        }
    }

    pub fn probe_one_frame() -> Result<(u32, u32), String> {
        let _slot = capture_slot().lock().unwrap_or_else(|e| e.into_inner());
        let _guard = ComMfGuard::init()?;
        let mut cam = open_camera().map_err(|s| format!("{s:?}"))?;
        let mut buf = Vec::new();
        for _ in 0..15 {
            match read_one_sample(&mut cam, &mut buf) {
                Ok(true) if !buf.is_empty() => return Ok((cam.width, cam.height)),
                Ok(_) => std::thread::sleep(Duration::from_millis(20)),
                Err(s) => return Err(format!("{s:?}")),
            }
        }
        Err("Aucune image reçue dans le délai imparti.".into())
    }

    pub fn run(stop: Arc<AtomicBool>, state: Arc<Mutex<SharedState>>) {
        let _slot = capture_slot().lock().unwrap_or_else(|e| e.into_inner());
        if stop.load(Ordering::SeqCst) {
            return;
        }
        let _guard = match ComMfGuard::init() {
            Ok(g) => g,
            Err(e) => {
                if let Ok(mut s) = state.lock() {
                    s.status = Status::Error(e);
                }
                return;
            }
        };
        let mut cam = match open_camera() {
            Ok(c) => c,
            Err(st) => {
                if !stop.load(Ordering::SeqCst) {
                    if let Ok(mut s) = state.lock() {
                        s.status = st;
                        s.frame = None;
                    }
                }
                return;
            }
        };

        let mut seq: u64 = 0;
        let mut scratch = Vec::with_capacity((cam.width * cam.height * 4) as usize);
        let mut fps_window_start = Instant::now();
        let mut fps_frames: u32 = 0;
        let mut current_fps: u32 = 30;
        let mut empty_ticks: u32 = 0;

        while !stop.load(Ordering::SeqCst) {
            let t0 = Instant::now();
            match read_one_sample(&mut cam, &mut scratch) {
                Ok(true) if !scratch.is_empty() => {
                    empty_ticks = 0;
                    seq = seq.wrapping_add(1);
                    fps_frames += 1;
                    let elapsed = fps_window_start.elapsed();
                    if elapsed >= Duration::from_secs(1) {
                        current_fps = ((fps_frames as f64 / elapsed.as_secs_f64()).round() as u32).clamp(1, 60);
                        fps_frames = 0;
                        fps_window_start = Instant::now();
                    }
                    let bgra = Arc::new(scratch.clone());
                    let status = Status::Live { width: cam.width, height: cam.height, fps: current_fps };
                    if !stop.load(Ordering::SeqCst) {
                        if let Ok(mut s) = state.lock() {
                            s.status = status;
                            s.frame = Some(PreviewFrame { seq, width: cam.width, height: cam.height, bgra, fps: current_fps });
                        }
                    }
                    // Cadence douce (~30 ips max) pour laisser le CPU tranquille.
                    if let Some(rem) = Duration::from_millis(32).checked_sub(t0.elapsed()) {
                        std::thread::sleep(rem);
                    }
                }
                Ok(_) => {
                    empty_ticks += 1;
                    if empty_ticks > 120 {
                        if let Ok(mut s) = state.lock() {
                            s.status = Status::Busy("Flux vidéo occupé ou sans image : tes réglages UVC restent actifs.".into());
                            s.frame = None;
                        }
                        break;
                    }
                    std::thread::sleep(Duration::from_millis(15));
                }
                Err(st) => {
                    if !stop.load(Ordering::SeqCst) {
                        if let Ok(mut s) = state.lock() {
                            s.status = st;
                            s.frame = None;
                        }
                    }
                    break;
                }
            }
        }
        // `cam` et `_guard` se ferment ici : lecteur libéré, source arrêtée, LED bleue éteinte.
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reconnait_uniquement_les_c920_logitech() {
        assert!(is_c920_video_symlink(r"\\?\usb#vid_046d&pid_0892&mi_00#7&123#{e5323777}"));
        assert!(is_c920_video_symlink(r"\\?\USB#VID_046D&PID_082D&MI_00#6&abc"));
        assert!(is_c920_video_symlink(r"\\?\usb#vid_046d&pid_08e5&mi_00#7&xyz"));
        assert!(!is_c920_video_symlink(r"\\?\usb#vid_046d&pid_085e&mi_00#7&brio"));
        assert!(!is_c920_video_symlink(r"\\?\usb#vid_1220&pid_8fe4#goxlr"));
    }

    #[test]
    fn copie_rgb32_top_down_et_bottom_up_avec_alpha_opaque() {
        // 2x2 pixels avec stride = 12 octets (> 8) et alpha = 0 (comme RGB32 Windows)
        let mut src = vec![0u8; 24];
        // Ligne 0 : (10,20,30,0) (40,50,60,0)
        src[0..8].copy_from_slice(&[10, 20, 30, 0, 40, 50, 60, 0]);
        // Ligne 1 : (70,80,90,0) (100,110,120,0)
        src[12..20].copy_from_slice(&[70, 80, 90, 0, 100, 110, 120, 0]);

        let mut dst = Vec::new();
        unsafe { copy_rgb32_to_bgra(src.as_ptr(), 12, 2, 2, &mut dst) };
        assert_eq!(dst, vec![10, 20, 30, 255, 40, 50, 60, 255, 70, 80, 90, 255, 100, 110, 120, 255]);

        // Bottom-up (stride = -12, scanline0 pointe sur ligne 1)
        let mut flipped = Vec::new();
        unsafe { copy_rgb32_to_bgra(src.as_ptr().add(12), -12, 2, 2, &mut flipped) };
        assert_eq!(flipped, vec![70, 80, 90, 255, 100, 110, 120, 255, 10, 20, 30, 255, 40, 50, 60, 255]);

        // Miroir horizontal
        let mirrored = flip_horizontal_bgra(2, 2, &dst);
        assert_eq!(mirrored, vec![40, 50, 60, 255, 10, 20, 30, 255, 100, 110, 120, 255, 70, 80, 90, 255]);
    }

    #[test]
    fn score_resolution_prefere_16_9_fluide() {
        assert!(score_native_mode(800, 448) > score_native_mode(1920, 1080));
        assert!(score_native_mode(640, 360) > score_native_mode(640, 480));
        assert!(score_native_mode(864, 480) > score_native_mode(320, 240));
    }
}
