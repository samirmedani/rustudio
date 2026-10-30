//! Visibilite WebView2 quand les ecrans s'eteignent. Aucune action audio ou d'alimentation.
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{sync_channel, Receiver, SyncSender};

static DISPLAY_ON: AtomicBool = AtomicBool::new(true);

pub fn is_on() -> bool {
    DISPLAY_ON.load(Ordering::Acquire)
}

#[cfg(any(windows, test))]
fn decode_state(data: &[u8]) -> Option<bool> {
    let [a, b, c, d] = data else { return None };
    match u32::from_le_bytes([*a, *b, *c, *d]) {
        0 => Some(false),
        1 | 2 => Some(true), // allume ou simplement attenue
        _ => None,
    }
}

pub struct Watcher {
    // Adresse stable, conservee jusqu'au retrait de la notification native.
    _sender: Box<SyncSender<()>>,
    #[cfg(windows)]
    registration: usize,
    #[cfg(windows)]
    recipient: Box<windows::Win32::System::Power::DEVICE_NOTIFY_SUBSCRIBE_PARAMETERS>,
}

// SAFETY : les deux allocations sont stables et immuables apres l'enregistrement.
// Le callback n'utilise que l'emetteur SyncSender (Send+Sync) et l'atomique ci-dessus.
// Le retrait de l'enregistrement peut se faire depuis n'importe quel fil, hors du callback.
#[cfg(windows)]
unsafe impl Send for Watcher {}
#[cfg(windows)]
unsafe impl Sync for Watcher {}

fn channel() -> (Watcher, Receiver<()>) {
    let (sender, receiver) = sync_channel(1);
    let watcher = Watcher {
        _sender: Box::new(sender),
        #[cfg(windows)]
        registration: 0,
        #[cfg(windows)]
        recipient: Box::default(),
    };
    (watcher, receiver)
}

pub fn subscribe() -> (Watcher, Receiver<()>) {
    #[allow(unused_mut)]
    let (mut watcher, receiver) = channel();
    #[cfg(windows)]
    {
        use windows::Win32::Foundation::HANDLE;
        use windows::Win32::System::Power::PowerSettingRegisterNotification;
        use windows::Win32::UI::WindowsAndMessaging::DEVICE_NOTIFY_CALLBACK;
        watcher.recipient.Callback = Some(on_power);
        watcher.recipient.Context = (&*watcher._sender as *const SyncSender<()>).cast_mut().cast();
        let recipient = HANDLE((&mut *watcher.recipient as *mut _ as *mut core::ffi::c_void).cast());
        let mut handle = std::ptr::null_mut();
        // SAFETY : signature Windows, allocations vivantes et pointeurs stables pendant toute la souscription.
        let status = unsafe { PowerSettingRegisterNotification(&SESSION_DISPLAY_STATUS, DEVICE_NOTIFY_CALLBACK, recipient, &mut handle) };
        if status.0 == 0 {
            watcher.registration = handle as usize;
        } else {
            DISPLAY_ON.store(true, Ordering::Release); // notification indisponible : conserver le rendu normal
        }
    }
    (watcher, receiver)
}

#[cfg(windows)]
const SESSION_DISPLAY_STATUS: windows::core::GUID = windows::core::GUID::from_u128(0x2b84c20e_ad23_4ddf_93db_05ffbd7efca5);

#[cfg(windows)]
unsafe extern "system" fn on_power(context: *const core::ffi::c_void, kind: u32, setting: *const core::ffi::c_void) -> u32 {
    use windows::Win32::System::Power::POWERBROADCAST_SETTING;
    use windows::Win32::UI::WindowsAndMessaging::PBT_POWERSETTINGCHANGE;
    // Aucun acces WebView2/COM depuis le callback Windows. Le fil de surveillance est reveille sans attente.
    let _ = std::panic::catch_unwind(|| {
        if kind != PBT_POWERSETTINGCHANGE || setting.is_null() || context.is_null() {
            return;
        }
        // SAFETY : Windows fournit POWERBROADCAST_SETTING pour ce type de notification.
        let setting = setting.cast::<POWERBROADCAST_SETTING>();
        let guid = unsafe { std::ptr::addr_of!((*setting).PowerSetting).read_unaligned() };
        let length = unsafe { std::ptr::addr_of!((*setting).DataLength).read_unaligned() };
        if guid != SESSION_DISPLAY_STATUS || length != 4 {
            return;
        }
        // SAFETY : les quatre octets annonces par Windows suivent l'en-tete valide.
        let data = unsafe { std::slice::from_raw_parts(std::ptr::addr_of!((*setting).Data).cast::<u8>(), 4) };
        if let Some(on) = decode_state(data) {
            if DISPLAY_ON.swap(on, Ordering::AcqRel) != on {
                // SAFETY : pointeur sur l'emetteur conserve par Watcher jusqu'au retrait du callback.
                let sender = unsafe { &*context.cast::<SyncSender<()>>() };
                let _ = sender.try_send(());
            }
        }
    });
    0
}

#[cfg(windows)]
impl Drop for Watcher {
    fn drop(&mut self) {
        if self.registration != 0 {
            use windows::Win32::System::Power::{HPOWERNOTIFY, PowerSettingUnregisterNotification};
            // SAFETY : un seul proprietaire ; retirer le callback avant de liberer ses allocations.
            unsafe { let _ = PowerSettingUnregisterNotification(HPOWERNOTIFY(self.registration as isize)); }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extinction_reveil_et_attenuation() {
        assert_eq!(decode_state(&0u32.to_le_bytes()), Some(false));
        assert_eq!(decode_state(&1u32.to_le_bytes()), Some(true));
        assert_eq!(decode_state(&2u32.to_le_bytes()), Some(true));
    }

    #[test]
    fn notification_tronquee_ou_inconnue_ignoree() {
        for data in [&[][..], &[0][..], &[0, 0, 0][..], &[0, 0, 0, 0, 0][..], &[3, 0, 0, 0][..]] {
            assert_eq!(decode_state(data), None);
        }
    }

    #[test]
    fn surveillance_sans_evenement_ne_boucle_pas_a_vide() {
        let (_watcher, events) = channel(); // meme repli si Windows refuse la souscription
        assert!(matches!(events.recv_timeout(std::time::Duration::from_millis(20)), Err(std::sync::mpsc::RecvTimeoutError::Timeout)));
    }
}
