use rustudio_core::mixer_profile::MixerSettings;
use serde::{Deserialize, Serialize};
use std::{
    fs::{self, File},
    io::{Read, Write},
    path::{Path, PathBuf},
};

const MAX_PROFILES: usize = 64;
const MAX_BYTES: u64 = 16 * 1024;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Profile {
    pub format: u8,
    pub module: String,
    pub name: String,
    pub mix: MixerSettings,
}

impl Profile {
    pub fn new(name: &str, mix: MixerSettings) -> Result<Self, String> {
        let profile = Self {
            format: 1,
            module: "goxlr-mini".into(),
            name: name.trim().into(),
            mix,
        };
        profile.validate()?;
        Ok(profile)
    }

    fn validate(&self) -> Result<(), String> {
        if self.format != 1 || self.module != "goxlr-mini" {
            return Err("Ce fichier n’est pas un profil de mixeur Rustudio compatible.".into());
        }
        if self.name.is_empty()
            || self.name.trim() != self.name
            || self.name.chars().count() > 40
            || self.name.chars().any(char::is_control)
        {
            return Err("Donne au profil un nom de 1 à 40 caractères.".into());
        }
        self.mix.validate().map_err(str::to_owned)
    }

    pub fn read(path: &Path) -> Result<Self, String> {
        let file = File::open(path).map_err(|e| format!("Lecture du profil impossible : {e}"))?;
        let mut bytes = Vec::new();
        file.take(MAX_BYTES + 1)
            .read_to_end(&mut bytes)
            .map_err(|e| format!("Lecture du profil impossible : {e}"))?;
        if bytes.len() as u64 > MAX_BYTES {
            return Err("Fichier de profil trop volumineux.".into());
        }
        let profile: Self = serde_json::from_slice(&bytes)
            .map_err(|_| "Fichier de profil illisible ou incompatible.")?;
        profile.validate()?;
        Ok(profile)
    }

    pub fn export(&self, path: &Path) -> Result<(), String> {
        self.write(path)
    }

    fn write(&self, path: &Path) -> Result<(), String> {
        self.validate()?;
        let parent = path
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."));
        let write = || -> std::io::Result<()> {
            let mut file = tempfile::NamedTempFile::new_in(parent)?;
            serde_json::to_writer_pretty(&mut file, self)?;
            file.write_all(b"\n")?;
            file.as_file().sync_all()?;
            file.persist_noclobber(path)?;
            Ok(())
        };
        write().map_err(|e| match e.kind() {
            std::io::ErrorKind::AlreadyExists => {
                "Un fichier porte déjà ce nom. Choisis un autre nom.".into()
            }
            _ => format!("Écriture du profil impossible : {e}"),
        })
    }
}

#[derive(Clone)]
pub struct Store {
    dir: PathBuf,
}

impl Store {
    pub fn new(dir: PathBuf) -> Self {
        Self { dir }
    }
    #[cfg(test)]
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    fn slot(&self, index: usize) -> PathBuf {
        self.dir.join(format!("profil-{index:02}.json"))
    }

    pub fn list(&self) -> (Vec<(usize, Profile)>, usize) {
        let mut profiles = Vec::new();
        let mut ignored = 0;
        for i in 1..=MAX_PROFILES {
            let path = self.slot(i);
            if path.exists() {
                match Profile::read(&path) {
                    Ok(profile) => profiles.push((i, profile)),
                    Err(_) => ignored += 1,
                }
            }
        }
        (profiles, ignored)
    }

    pub fn save(&self, profile: &Profile) -> Result<(), String> {
        fs::create_dir_all(&self.dir)
            .map_err(|e| format!("Dossier de profils inaccessible : {e}"))?;
        if self
            .list()
            .0
            .iter()
            .any(|(_, p)| p.name.eq_ignore_ascii_case(&profile.name))
        {
            return Err("Un profil porte déjà ce nom.".into());
        }
        let index = (1..=MAX_PROFILES)
            .find(|&i| !self.slot(i).exists())
            .ok_or("La liste de profils est pleine.")?;
        profile.write(&self.slot(index))
    }

    pub fn remove(&self, index: usize) -> Result<(), String> {
        if !(1..=MAX_PROFILES).contains(&index) {
            return Err("Profil inconnu.".into());
        }
        fs::remove_file(self.slot(index))
            .map_err(|e| format!("Suppression du profil impossible : {e}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rustudio_core::consts::{CHANNELS, FADERS};

    pub fn fixture() -> MixerSettings {
        MixerSettings {
            volumes: CHANNELS.into_iter().map(|c| (c.into(), 70)).collect(),
            faders: FADERS
                .into_iter()
                .zip(CHANNELS)
                .map(|(f, c)| (f.into(), c.into()))
                .collect(),
            mute_modes: FADERS
                .into_iter()
                .map(|f| (f.into(), "all".into()))
                .collect(),
            cuts: CHANNELS.into_iter().map(|c| (c.into(), None)).collect(),
        }
    }

    #[test]
    fn profil_enregistre_recharge_exporte_et_reimporte_sans_ecraser() {
        let root = tempfile::tempdir().unwrap();
        let store = Store::new(root.path().join("premier"));
        let profile = Profile::new("Démo", fixture()).unwrap();
        store.save(&profile).unwrap();
        assert!(store.save(&profile).is_err());
        let (list, ignored) = Store::new(store.dir().into()).list();
        assert_eq!(ignored, 0);
        assert_eq!(list, [(1, profile.clone())]);
        let output = root.path().join("partage.json");
        profile.export(&output).unwrap();
        assert!(profile.export(&output).is_err());
        let second = Store::new(root.path().join("second"));
        second.save(&Profile::read(&output).unwrap()).unwrap();
        assert_eq!(second.list().0, list);
        second.remove(1).unwrap();
        assert!(second.list().0.is_empty());
        assert_eq!(Profile::read(&output).unwrap(), profile);
    }

    #[test]
    fn rejette_les_donnees_hors_mixeur_et_le_fichier_trop_grand() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("invalide.json");
        let mut value = serde_json::to_value(Profile::new("Démo", fixture()).unwrap()).unwrap();
        value["voice_print"] = serde_json::json!({});
        fs::write(&path, value.to_string()).unwrap();
        assert!(Profile::read(&path).is_err());
        fs::write(&path, vec![b' '; MAX_BYTES as usize + 1]).unwrap();
        assert!(Profile::read(&path).is_err());
        let mut mix = fixture();
        mix.faders.insert("D".into(), mix.faders["A"].clone());
        assert!(Profile::new("Démo", mix).is_err());
    }
}
