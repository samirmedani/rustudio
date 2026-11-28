//! Installation explicite de l'effet, limitée au magasin Capture. Les valeurs originales,
//! les valeurs posées et le branchement sont validés dans la même transaction de registre.
use crate::control::CLASS;
use sha2::{Digest, Sha256};
use std::{
    io,
    path::{Path, PathBuf},
    ptr,
};
use windows_sys::Win32::{Foundation::*, Security::*, System::Threading::*};
use winreg::{
    enums::*,
    transaction::Transaction,
    types::{FromRegValue, ToRegValue},
    RegKey, RegValue,
};

const CAPTURE: &str = r"SOFTWARE\Microsoft\Windows\CurrentVersion\MMDevices\Audio\Capture";
const JOURNAL: &str = r"SOFTWARE\Rustudio\Micro\Endpoints";
const FX: &str = "{d04e05a6-594b-4fb6-a80d-01af5eed7d1d},";
const MODES: &str = "{d3993a3f-99c2-4402-b5ec-a92a0367664b},";
const DISABLED: &str = "{1da5d803-d492-4edd-8c23-e0c0ffee7f0e},5";
const COM: &str = r"SOFTWARE\Classes\CLSID";
const AUDIO: &str = r"SOFTWARE\Classes\AudioEngine\AudioProcessingObjects";
const DLL: &str = "rustudio_micro_apo.dll";
const SLOT: [u32; 5] = [1, 2, 5, 6, 7];
const MODE_LIST: [&str; 3] = [
    "{C18E2F7E-933D-4965-B7D1-1EEF228D2AF3}",
    "{98951333-B9CD-48B1-A0A3-FF40682D73F7}",
    "{FC1CFC9B-B9D6-4CFA-B5E0-4BB2166878B2}",
];
const EAPO: [&str; 2] = [
    "{EACD2258-FCAC-4FF4-B36D-419E924A6D79}",
    "{EC1CC9CE-FAED-4822-828A-82A81A6F018F}",
];

fn key() -> RegKey {
    RegKey::predef(HKEY_LOCAL_MACHINE)
}
fn invalid(msg: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, msg)
}
fn check(guid: &str) -> io::Result<()> {
    if rustudio_core::micns::is_guid(guid) {
        Ok(())
    } else {
        Err(invalid("Identifiant de capture invalide."))
    }
}
fn optional<T>(result: io::Result<T>) -> io::Result<Option<T>> {
    match result {
        Ok(x) => Ok(Some(x)),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e),
    }
}

fn install_dir() -> io::Result<PathBuf> {
    let pf = std::env::var_os("ProgramW6432")
        .or_else(|| std::env::var_os("ProgramFiles"))
        .ok_or_else(|| invalid("Program Files introuvable."))?;
    Ok(Path::new(&pf).join("Rustudio"))
}

fn manifest_digest(dir: &Path) -> io::Result<String> {
    let manifest: serde_json::Value =
        serde_json::from_slice(&std::fs::read(dir.join("manifest.json"))?)
            .map_err(|_| invalid("Manifeste de Rustudio illisible."))?;
    manifest["files"][DLL]
        .as_str()
        .filter(|s| valid_digest(s))
        .map(str::to_owned)
        .ok_or_else(|| invalid("Empreinte de la DLL micro absente ou invalide."))
}

fn valid_digest(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

fn payload() -> io::Result<PathBuf> {
    let dir = install_dir()?;
    let bytes = std::fs::read(dir.join(DLL))?;
    let digest = format!("{:x}", Sha256::digest(&bytes));
    if manifest_digest(&dir)? != digest {
        return Err(invalid(
            "L’empreinte de la DLL micro ne correspond pas au manifeste.",
        ));
    }
    // Windows peut garder une image mappée pendant toute la vie d’audiodg. L’archive de mise
    // à jour reste remplaçable et le chemin COM désigne toujours une copie entièrement écrite.
    let folder = dir.join("Audio").join(&digest);
    std::fs::create_dir_all(&folder)?;
    let file = folder.join(DLL);
    if file.is_file() {
        if std::fs::read(&file)? != bytes {
            return Err(invalid("Copie de la DLL micro endommagée."));
        }
    } else {
        use std::io::Write;
        let tmp = folder.join(format!("{}.tmp", std::process::id()));
        let result = (|| -> io::Result<()> {
            let mut out = std::fs::File::create(&tmp)?;
            out.write_all(&bytes)?;
            out.sync_all()?;
            drop(out);
            std::fs::rename(&tmp, &file)
        })();
        if result.is_err() {
            let _ = std::fs::remove_file(tmp);
        }
        result?;
    }
    Ok(file)
}

/// Au démarrage, libère les anciennes copies de l'effet. Les fichiers du paquet
/// et du branchement courant restent disponibles, ainsi que les copies récentes
/// qu'un autre démarrage ou un retour de version peut encore utiliser.
pub fn cleanup_binary_cache() -> io::Result<()> {
    use std::os::windows::fs::MetadataExt;
    use std::time::Duration;
    let dir = install_dir()?;
    let audio = dir.join("Audio");
    let plain = |m: &std::fs::Metadata| m.file_attributes() & 0x400 == 0; // FILE_ATTRIBUTE_REPARSE_POINT
    let Some(meta) = optional(std::fs::symlink_metadata(&audio))? else {
        return Ok(());
    };
    if !meta.is_dir() || !plain(&meta) {
        return Ok(());
    }
    let current = manifest_digest(&dir)?;
    let com = optional(key().open_subkey(format!(r"{COM}\{CLASS}\InprocServer32")))?;
    let active = com
        .map(|k| k.get_value::<String, _>(""))
        .transpose()?
        .map(PathBuf::from);
    for entry in std::fs::read_dir(audio)? {
        let folder = entry?.path();
        let Some(digest) = folder.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        if !valid_digest(digest) || digest == current {
            continue;
        }
        let meta = std::fs::symlink_metadata(&folder)?;
        if !meta.is_dir() || !plain(&meta) {
            continue;
        }
        let file = folder.join(DLL);
        if active.as_ref() == Some(&file) {
            continue;
        }
        let Some(meta) = optional(std::fs::symlink_metadata(&file))? else {
            continue;
        };
        if !meta.is_file()
            || !plain(&meta)
            || meta.modified()?.elapsed().unwrap_or_default() < Duration::from_secs(86400)
        {
            continue;
        }
        let mut hash = Sha256::new();
        std::io::copy(&mut std::fs::File::open(&file)?, &mut hash)?;
        if format!("{:x}", hash.finalize()) == digest {
            match std::fs::remove_file(&file) {
                Ok(()) => {
                    if folder.read_dir()?.next().is_none() {
                        std::fs::remove_dir(&folder)?;
                    }
                }
                Err(e) if e.raw_os_error() == Some(32) => {} // audiodg possède encore cette image
                Err(e) => return Err(e),
            }
        }
    }
    Ok(())
}

fn register(t: &Transaction, path: &Path) -> io::Result<()> {
    let (com, _) = key().create_subkey_transacted(format!(r"{COM}\{CLASS}\InprocServer32"), t)?;
    com.set_value("", &path.as_os_str())?;
    com.set_value("ThreadingModel", &"Both")?;
    let (apo, _) = key().create_subkey_transacted(format!(r"{AUDIO}\{CLASS}"), t)?;
    apo.set_value("FriendlyName", &"Rustudio Micro")?;
    apo.set_value("Copyright", &"Rustudio (MIT)")?;
    for (name, n) in [
        ("MajorVersion", 1u32),
        ("MinorVersion", 0),
        ("Flags", 15),
        ("MinInputConnections", 1),
        ("MaxInputConnections", 1),
        ("MinOutputConnections", 1),
        ("MaxOutputConnections", 1),
        ("MaxInstances", u32::MAX),
        ("NumAPOInterfaces", 1),
    ] {
        apo.set_value(name, &n)?;
    }
    apo.set_value("APOInterface0", &"{FD7F2B29-24D0-4B5C-B177-592C39F9CA10}")
}

// Le service garde le propriétaire et la DACL du pilote. L'élévation de droits est limitée au
// fil de cette transaction et disparaît quand son jeton d’emprunt est refermé.
struct RestorePrivilege;
impl RestorePrivilege {
    fn acquire() -> io::Result<Self> {
        unsafe {
            if ImpersonateSelf(SecurityImpersonation) == 0 {
                return Err(io::Error::last_os_error());
            }
            let guard = Self;
            let mut token = ptr::null_mut();
            if OpenThreadToken(
                GetCurrentThread(),
                TOKEN_ADJUST_PRIVILEGES | TOKEN_QUERY,
                1,
                &mut token,
            ) == 0
            {
                return Err(io::Error::last_os_error());
            }
            let r = (|| {
                for name in [SE_BACKUP_NAME, SE_RESTORE_NAME] {
                    let mut tp: TOKEN_PRIVILEGES = std::mem::zeroed();
                    tp.PrivilegeCount = 1;
                    tp.Privileges[0].Attributes = SE_PRIVILEGE_ENABLED;
                    if LookupPrivilegeValueW(ptr::null(), name, &mut tp.Privileges[0].Luid) == 0 {
                        return Err(io::Error::last_os_error());
                    }
                    SetLastError(0);
                    if AdjustTokenPrivileges(token, 0, &tp, 0, ptr::null_mut(), ptr::null_mut())
                        == 0
                        || GetLastError() != 0
                    {
                        return Err(io::Error::last_os_error());
                    }
                }
                Ok(())
            })();
            CloseHandle(token);
            r.map(|_| guard)
        }
    }
}
impl Drop for RestorePrivilege {
    fn drop(&mut self) {
        unsafe {
            RevertToSelf();
        }
    }
}

fn write(k: &RegKey, name: &str, v: Option<&RegValue>) -> io::Result<()> {
    match v {
        Some(v) => k.set_raw_value(name, v),
        None => optional(k.delete_value(name)).map(|_| ()),
    }
}

struct Journal {
    before: RegKey,
    after: RegKey,
    entry: RegKey,
}
impl Journal {
    fn change(&self, fx: &RegKey, name: &str, new: Option<RegValue>) -> io::Result<()> {
        let old = optional(fx.get_raw_value(name))?;
        if old == new {
            return Ok(());
        }
        write(&self.before, name, old.as_ref())?;
        write(&self.after, name, new.as_ref())?;
        self.entry.set_value(name, &1u32)?;
        write(fx, name, new.as_ref())
    }
}

pub fn managed() -> io::Result<Vec<String>> {
    let Some(root) = optional(key().open_subkey(JOURNAL))? else {
        return Ok(Vec::new());
    };
    root.enum_keys()
        .filter(|g| {
            g.as_ref()
                .map_or(true, |s| rustudio_core::micns::is_guid(s))
        })
        .collect()
}

pub fn attached(guid: &str) -> bool {
    if check(guid).is_err() {
        return false;
    }
    let Ok(fx) = key().open_subkey(format!(r"{CAPTURE}\{guid}\FxProperties")) else {
        return false;
    };
    SLOT.iter().any(|s| {
        fx.get_value::<String, _>(format!("{FX}{s}"))
            .is_ok_and(|s| s.eq_ignore_ascii_case(CLASS))
    }) && !fx.get_value::<u32, _>(DISABLED).is_ok_and(|n| n != 0)
}

pub fn attach(guid: &str, migrate: bool) -> io::Result<()> {
    check(guid)?;
    let _privilege = RestorePrivilege::acquire()?;
    let tx = Transaction::new()?;
    let endpoint = key().open_subkey_transacted(format!(r"{CAPTURE}\{guid}"), &tx)?;
    if endpoint.get_value::<u32, _>("DeviceState")? != 1 {
        return Err(invalid("Le micro a été débranché."));
    }
    let (fx, created) = endpoint.create_subkey_transacted_with_options_flags(
        "FxProperties",
        &tx,
        REG_OPTION_BACKUP_RESTORE,
        KEY_ALL_ACCESS,
    )?;
    let (entry, status) = key().create_subkey_transacted(format!(r"{JOURNAL}\{guid}"), &tx)?;
    if status != REG_CREATED_NEW_KEY {
        return if attached(guid) {
            Ok(())
        } else {
            Err(invalid(
                "Windows a modifié ce micro. Retire le branchement précédent avant de le rétablir.",
            ))
        };
    }
    register(&tx, &payload()?)?;
    entry.set_value("CreatedFx", &u32::from(created == REG_CREATED_NEW_KEY))?;
    let j = Journal {
        before: entry.create_subkey_transacted("Before", &tx)?.0,
        after: entry.create_subkey_transacted("After", &tx)?.0,
        entry,
    };
    // Pour une migration demandée, remettre les effets constructeur derrière Equalizer APO
    // avant de choisir un emplacement libre. Le journal d'Equalizer APO reste intact.
    for slot in SLOT {
        let name = format!("{FX}{slot}");
        let is_eq = optional(fx.get_raw_value(&name))?
            .and_then(|v| String::from_reg_value(&v).ok())
            .is_some_and(|s| EAPO.iter().any(|id| s.eq_ignore_ascii_case(id)));
        if is_eq {
            if !migrate {
                return Err(invalid(
                    "Equalizer APO est déjà branché par un autre réglage sur ce micro.",
                ));
            }
            let child = key()
                .open_subkey_transacted(format!(r"SOFTWARE\EqualizerAPO\Child APOs\{guid}"), &tx)?;
            let old: String = child.get_value(&name)?;
            let value = if old == "!VALUE" {
                None
            } else if rustudio_core::micns::is_guid(&old) {
                Some(old.to_reg_value())
            } else {
                return Err(invalid("Sauvegarde Equalizer APO inconnue."));
            };
            j.change(&fx, &name, value)?;
        }
    }
    let legacy = [1, 2]
        .iter()
        .any(|s| fx.get_raw_value(format!("{FX}{s}")).is_ok());
    let slots: &[u32] = if legacy { &[1, 2] } else { &[7, 5, 6] };
    let mut chosen = None;
    for &s in slots {
        if optional(fx.get_raw_value(format!("{FX}{s}")))?.is_none()
            && (s < 5 || optional(fx.get_raw_value(format!("{FX}{}", s + 8)))?.is_none())
        {
            chosen = Some(s);
            break;
        }
    }
    let s = chosen
        .ok_or_else(|| invalid("Les emplacements d’effets de ce micro sont déjà utilisés."))?;
    let name = format!("{FX}{s}");
    // La migration peut toucher le même emplacement deux fois. Garder sa première valeur.
    if j.entry.get_raw_value(&name).is_ok() {
        j.after.set_value(&name, &CLASS)?;
        fx.set_value(&name, &CLASS)?;
    } else {
        j.change(&fx, &name, Some(CLASS.to_reg_value()))?;
    }
    if s >= 5 {
        j.change(
            &fx,
            &format!("{MODES}{s}"),
            Some(MODE_LIST.to_vec().to_reg_value()),
        )?;
    }
    j.change(&fx, DISABLED, None)?;
    tx.commit()
}

pub fn detach(guid: &str) -> io::Result<()> {
    check(guid)?;
    let _privilege = RestorePrivilege::acquire()?;
    let tx = Transaction::new()?;
    detach_in(guid, &tx)?;
    tx.commit()
}

fn detach_in(guid: &str, tx: &Transaction) -> io::Result<()> {
    let path = format!(r"{JOURNAL}\{guid}");
    let Some(entry) = optional(key().open_subkey_transacted_with_flags(&path, tx, KEY_ALL_ACCESS))?
    else {
        return Ok(());
    };
    let j = Journal {
        before: entry.open_subkey_transacted("Before", tx)?,
        after: entry.open_subkey_transacted("After", tx)?,
        entry,
    };
    if let Some(endpoint) =
        optional(key().open_subkey_transacted(format!(r"{CAPTURE}\{guid}"), tx))?
    {
        if optional(endpoint.open_subkey_transacted("FxProperties", tx))?.is_some() {
            let fx = endpoint
                .create_subkey_transacted_with_options_flags(
                    "FxProperties",
                    tx,
                    REG_OPTION_BACKUP_RESTORE,
                    KEY_ALL_ACCESS,
                )?
                .0;
            for value in j.entry.enum_values() {
                let (name, _) = value?;
                if name == "CreatedFx" {
                    continue;
                }
                if optional(fx.get_raw_value(&name))? == optional(j.after.get_raw_value(&name))? {
                    write(
                        &fx,
                        &name,
                        optional(j.before.get_raw_value(&name))?.as_ref(),
                    )?;
                }
            }
            if j.entry.get_value::<u32, _>("CreatedFx")? == 1
                && fx.query_info()?.values == 0
                && fx.query_info()?.sub_keys == 0
            {
                drop(fx);
                endpoint.delete_subkey_transacted("FxProperties", tx)?;
            }
        }
    }
    j.entry.delete_subkey_transacted("Before", tx)?;
    j.entry.delete_subkey_transacted("After", tx)?;
    key().delete_subkey_transacted(&path, tx)?;
    let root = key().open_subkey_transacted(JOURNAL, tx)?;
    if root.query_info()?.sub_keys == 0 {
        for p in [
            format!(r"{COM}\{CLASS}\InprocServer32"),
            format!(r"{COM}\{CLASS}"),
            format!(r"{AUDIO}\{CLASS}"),
        ] {
            optional(key().delete_subkey_transacted(p, tx))?;
        }
    }
    Ok(())
}

/// Restaure toutes les entrées en une seule transaction, y compris celles débranchées.
pub fn detach_all() -> io::Result<bool> {
    let targets = managed()?;
    if targets.is_empty() {
        return Ok(false);
    }
    let _privilege = RestorePrivilege::acquire()?;
    let tx = Transaction::new()?;
    for guid in &targets {
        detach_in(guid, &tx)?;
    }
    tx.commit()?;
    Ok(true)
}

/// Une mise à jour remplace uniquement le chemin COM des micros déjà gérés.
pub fn refresh_binary() -> io::Result<bool> {
    if managed()?.is_empty() {
        return Ok(false);
    }
    let file = payload()?;
    let tx = Transaction::new()?;
    let com = key().open_subkey_transacted_with_flags(
        format!(r"{COM}\{CLASS}\InprocServer32"),
        &tx,
        KEY_READ | KEY_SET_VALUE,
    )?;
    let previous: String = com.get_value("")?;
    if Path::new(&previous) == file {
        return Ok(false);
    }
    com.set_value("", &file.as_os_str())?;
    tx.commit()?;
    Ok(true)
}
