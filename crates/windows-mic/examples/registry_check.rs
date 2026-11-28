//! Banc Windows sous SYSTEM. Le registre HKLM est redirigé pour ce processus vers
//! une ruche de test dans HKCU. Aucun périphérique ni service audio n'est ouvert.
#[cfg(windows)]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    use rustudio_windows_mic::{control::CLASS, registry};
    use sha2::{Digest, Sha256};
    use std::{fs, io, path::PathBuf, ptr};
    use windows_sys::Win32::System::Registry::RegOverridePredefKey;
    use winreg::{enums::*, RegKey};

    const CAP: &str = r"SOFTWARE\Microsoft\Windows\CurrentVersion\MMDevices\Audio\Capture";
    const CHILD: &str = r"SOFTWARE\EqualizerAPO\Child APOs";
    const FX: &str = "{d04e05a6-594b-4fb6-a80d-01af5eed7d1d},";
    const DISABLED: &str = "{1da5d803-d492-4edd-8c23-e0c0ffee7f0e},5";
    const EQ: &str = "{EACD2258-FCAC-4FF4-B36D-419E924A6D79}";
    const OEM: &str = "{7949FEBE-450E-4711-9E6E-91D6E2E2B91F}";
    const G1: &str = "{8392EE06-6D66-4DFC-836E-1ABA0730EA12}";
    const G2: &str = "{EA500815-0A30-47F3-B8DA-1B8F1242747B}";

    struct Fixture {
        hive: String,
        dir: PathBuf,
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            unsafe {
                RegOverridePredefKey(HKEY_LOCAL_MACHINE, ptr::null_mut());
            }
            let _ = RegKey::predef(HKEY_CURRENT_USER).delete_subkey_all(&self.hive);
            let _ = fs::remove_dir_all(&self.dir);
        }
    }
    fn payload(dir: &std::path::Path, body: &[u8]) -> io::Result<()> {
        let name = "rustudio_micro_apo.dll";
        let hash = format!("{:x}", Sha256::digest(body));
        fs::write(dir.join(name), body)?;
        fs::write(
            dir.join("manifest.json"),
            serde_json::json!({"files":{(name):hash}}).to_string(),
        )
    }

    let id = format!("{:?}", windows::core::GUID::new()?);
    let fixture = Fixture {
        hive: format!(r"Software\RustudioMicroTest-{id}"),
        dir: std::env::temp_dir().join(format!("rustudio-micro-{id}")),
    };
    let hive = RegKey::predef(HKEY_CURRENT_USER)
        .create_subkey(&fixture.hive)?
        .0;
    hive.set_value("RustudioTest", &id)?;
    let code = unsafe { RegOverridePredefKey(HKEY_LOCAL_MACHINE, hive.raw_handle()) };
    if code != 0 {
        return Err(io::Error::from_raw_os_error(code as i32).into());
    }
    let root = RegKey::predef(HKEY_LOCAL_MACHINE);
    // Confirmer la redirection avant toute écriture via HKLM.
    assert_eq!(root.get_value::<String, _>("RustudioTest")?, id);
    let dir = fixture.dir.join("Rustudio");
    fs::create_dir_all(&dir)?;
    std::env::set_var("ProgramW6432", &fixture.dir);
    payload(&dir, b"test-v1")?;

    let ep1 = root.create_subkey(format!(r"{CAP}\{G1}"))?.0;
    let ep2 = root.create_subkey(format!(r"{CAP}\{G2}"))?.0;
    for e in [&ep1, &ep2] {
        e.set_value("DeviceState", &1u32)?;
    }
    let fx = ep1.create_subkey("FxProperties")?.0;
    let slot5 = format!("{FX}5");
    let slot7 = format!("{FX}7");
    fx.set_value(&slot5, &EQ)?;
    fx.set_value(DISABLED, &1u32)?;
    let backup = root.create_subkey(format!(r"{CHILD}\{G1}"))?.0;
    backup.set_value(&slot5, &OEM)?;
    let com = format!(r"SOFTWARE\Classes\CLSID\{CLASS}\InprocServer32");

    assert!(registry::attach(G1, false).is_err());
    assert!(registry::managed()?.is_empty());
    assert!(root.open_subkey(&com).is_err());
    assert_eq!(fx.get_value::<String, _>(&slot5)?, EQ);
    registry::attach(G1, true)?;
    registry::attach(G1, true)?;
    assert!(registry::attached(G1));
    assert_eq!(fx.get_value::<String, _>(&slot5)?, OEM);
    assert_eq!(fx.get_value::<String, _>(&slot7)?, CLASS);
    assert!(fx.get_raw_value(DISABLED).is_err());
    registry::attach(G2, false)?;
    let old: String = root.open_subkey(&com)?.get_value("")?;
    assert!(!registry::refresh_binary()?);
    fs::write(dir.join("rustudio_micro_apo.dll"), b"modified")?;
    assert!(registry::refresh_binary().is_err());
    assert_eq!(root.open_subkey(&com)?.get_value::<String, _>("")?, old);
    payload(&dir, b"test-v2")?;
    assert!(registry::refresh_binary()?);
    assert_ne!(root.open_subkey(&com)?.get_value::<String, _>("")?, old);
    let previous = std::path::Path::new(&old);
    let foreign = previous.parent().unwrap().join("fichier-tiers.txt");
    fs::write(&foreign, b"preserver")?;
    registry::cleanup_binary_cache()?;
    assert!(previous.is_file());
    fs::File::options()
        .write(true)
        .open(previous)?
        .set_modified(std::time::SystemTime::now() - std::time::Duration::from_secs(2 * 86400))?;
    registry::cleanup_binary_cache()?;
    assert!(!previous.exists());
    assert_eq!(fs::read(foreign)?, b"preserver");
    assert!(PathBuf::from(root.open_subkey(&com)?.get_value::<String, _>("")?).is_file());
    registry::detach(G1)?;
    assert!(registry::attached(G2));
    assert_eq!(fx.get_value::<String, _>(&slot5)?, EQ);
    assert_eq!(fx.get_value::<u32, _>(DISABLED)?, 1);
    assert!(fx.get_raw_value(&slot7).is_err());
    let fx2 = ep2.open_subkey_with_flags("FxProperties", KEY_ALL_ACCESS)?;
    fx2.set_value(&slot7, &OEM)?;
    assert!(!registry::attached(G2));
    assert!(registry::attach(G2, false).is_err());
    registry::detach_all()?;
    assert_eq!(fx2.get_value::<String, _>(&slot7)?, OEM);
    assert!(root.open_subkey(&com).is_err());
    assert!(registry::managed()?.is_empty());

    // Une migration peut modifier deux fois le même emplacement dans la transaction.
    fx.delete_value(&slot5)?;
    fx.set_value(&slot7, &EQ)?;
    backup.set_value(&slot7, &"!VALUE")?;
    registry::attach(G1, true)?;
    registry::detach(G1)?;
    assert_eq!(fx.get_value::<String, _>(&slot7)?, EQ);
    assert_eq!(fx.get_value::<u32, _>(DISABLED)?, 1);
    println!("Migration, coexistence, mise à jour vérifiée, modification du pilote et restauration validées dans la ruche isolée.");
    Ok(())
}
#[cfg(not(windows))]
fn main() {
    eprintln!("Banc de registre réservé à Windows.");
}
