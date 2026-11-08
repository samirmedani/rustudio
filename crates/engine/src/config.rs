//! Dossiers, port et mode (simule ou vrai pilote).
use std::path::{Path, PathBuf};

pub const APP_NAME: &str = "Rustudio";
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
pub const API_HEADER: &str = "X-Rustudio";
pub const SERVICE_NAME: &str = "Rustudio";
pub const DEFAULT_PORT: u16 = 14565;
pub const SIMULATION_PORT: u16 = 14566;
pub const EXIT_RESTART: i32 = 75;
pub const EXIT_PORT_BUSY: i32 = 76;

#[derive(Clone, Debug)]
pub struct Config {
    pub simulate: bool,
    pub port: u16,
    pub data_dir: PathBuf,
    /// dossier d'installation (celui du moteur)
    pub base_dir: PathBuf,
    pub supervised: bool,
    pub engine_restarts: u32,
}

pub fn exe_dir() -> PathBuf {
    std::env::current_exe().ok().and_then(|p| p.parent().map(Path::to_path_buf)).unwrap_or_else(|| PathBuf::from("."))
}

pub fn default_data_dir() -> PathBuf {
    if cfg!(windows) {
        let pd = std::env::var("ProgramData").unwrap_or_else(|_| r"C:\ProgramData".into());
        PathBuf::from(pd).join(APP_NAME)
    } else {
        let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
        PathBuf::from(home).join(".local/share/rustudio")
    }
}

pub fn simulation_data_dir() -> PathBuf {
    std::env::temp_dir().join("rustudio-simulation")
}

/// Dossier de donnees de GoXLR Mini (l'ancien logiciel) : ses reglages sont repris au premier lancement.
pub fn legacy_data_dir() -> Option<PathBuf> {
    if cfg!(windows) {
        let pd = std::env::var("ProgramData").unwrap_or_else(|_| r"C:\ProgramData".into());
        Some(PathBuf::from(pd).join("GoXLR Mini"))
    } else {
        None
    }
}

impl Config {
    pub fn from_env() -> Config {
        let env = |k: &str| std::env::var(k).ok().filter(|v| !v.is_empty());
        let simulate = env("RUSTUDIO_SIMULATE").as_deref() == Some("1");
        Config {
            simulate,
            port: env("RUSTUDIO_PORT").and_then(|p| p.parse().ok()).unwrap_or(if simulate { SIMULATION_PORT } else { DEFAULT_PORT }),
            data_dir: env("RUSTUDIO_DATA_DIR").map(PathBuf::from).unwrap_or_else(|| {
                if simulate { simulation_data_dir() } else { default_data_dir() }
            }),
            base_dir: exe_dir(),
            supervised: env("RUSTUDIO_SUPERVISED").as_deref() == Some("1"),
            engine_restarts: env("RUSTUDIO_RESTARTS").and_then(|p| p.parse().ok()).unwrap_or(0),
        }
    }
    pub fn validate_simulation(&self) -> Result<(), &'static str> {
        if !self.simulate {
            return Ok(());
        }
        if self.port == DEFAULT_PORT {
            return Err("Le simulateur doit utiliser un autre port que celui de l’installation (14565).");
        }
        let canonical = |p: PathBuf| std::fs::canonicalize(&p).unwrap_or(p);
        let demo = canonical(self.data_dir.clone());
        if [Some(default_data_dir()), legacy_data_dir()].into_iter().flatten()
            .map(canonical).any(|real| demo.starts_with(real))
        {
            return Err("Le simulateur doit utiliser un dossier distinct des réglages installés.");
        }
        Ok(())
    }

    pub fn state_path(&self) -> PathBuf {
        self.data_dir.join("state.json")
    }
    pub fn log_path(&self) -> PathBuf {
        self.data_dir.join("rustudio.log")
    }
    pub fn manifest_path(&self) -> PathBuf {
        self.base_dir.join("manifest.json")
    }
}

pub fn now_epoch() -> f64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs_f64()).unwrap_or(0.0)
}
