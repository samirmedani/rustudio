//! Vérification après un branchement explicite : ouvre la seule entrée choisie en mode partagé,
//! libère les paquets sans en lire le contenu et vérifie le compteur de la DLL dans audiodg.
use crate::mapping::Mapping;
use std::{
    ptr,
    sync::atomic::Ordering,
    time::{Duration, Instant},
};
use windows::{
    core::HSTRING,
    Win32::{Media::Audio::*, System::Com::*},
};

pub fn verify(guid: &str) -> Result<(u64, u32), String> {
    let (frames, calls) = measure(guid)?;
    if frames == 0 || calls < 3 {
        return Err(format!(
            "Windows a livré {frames} trames, avec {calls} appel(s) à l’effet Rustudio."
        ));
    }
    Ok((frames, calls))
}

fn stage<T>(name: &str, result: windows::core::Result<T>) -> Result<T, String> {
    result.map_err(|e| format!("Vérification du micro, {name} : {e}"))
}

/// Mesure également une entrée sans effet pour le banc de comparaison en machine virtuelle.
pub fn measure(guid: &str) -> Result<(u64, u32), String> {
    let map = Mapping::open(guid, false).map_err(|e| e.to_string())?;
    let before = map.shared().callbacks.load(Ordering::Acquire);
    unsafe {
        CoInitializeEx(None, COINIT_MULTITHREADED)
            .ok()
            .map_err(|e| e.to_string())?;
        let result = (|| -> Result<_, String> {
            let enumerator: IMMDeviceEnumerator = stage(
                "énumération",
                CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL),
            )?;
            let device = stage(
                "entrée sélectionnée",
                enumerator.GetDevice(&HSTRING::from(format!("{{0.0.1.00000000}}.{guid}"))),
            )?;
            let client: IAudioClient =
                stage("activation WASAPI", device.Activate(CLSCTX_ALL, None))?;
            let f = stage("format partagé", client.GetMixFormat())?;
            let init = client.Initialize(AUDCLNT_SHAREMODE_SHARED, 0, 1_000_000, 0, f, None);
            CoTaskMemFree(Some(f.cast()));
            stage("initialisation du flux", init)?;
            let capture: IAudioCaptureClient = stage("service de capture", client.GetService())?;
            stage("démarrage de la capture", client.Start())?;
            let end = Instant::now() + Duration::from_secs(3);
            let mut frames = 0u64;
            let mut callbacks = 0;
            while Instant::now() < end {
                std::thread::sleep(Duration::from_millis(25));
                while Instant::now() < end
                    && stage("paquet suivant", capture.GetNextPacketSize())? != 0
                {
                    let (mut p, mut n, mut flags) = (ptr::null_mut(), 0, 0);
                    stage(
                        "réception du paquet",
                        capture.GetBuffer(&mut p, &mut n, &mut flags, None, None),
                    )?;
                    stage("libération du paquet", capture.ReleaseBuffer(n))?;
                    frames += u64::from(n);
                }
                callbacks = map
                    .shared()
                    .callbacks
                    .load(Ordering::Acquire)
                    .wrapping_sub(before);
                if frames > 0 && callbacks >= 3 {
                    break;
                }
            }
            stage("arrêt de la capture", client.Stop())?;
            Ok((frames, callbacks))
        })();
        CoUninitialize();
        result
    }
}
