//! Un canal de commande par micro. Le simulateur possède la même structure atomique en mémoire.
use rustudio_windows_mic::{
    control::{Controls, Shared},
    Voice,
};
use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    sync::{atomic::Ordering, Mutex},
    time::{Duration, Instant},
};

enum Memory {
    Sim(Box<Shared>),
    #[cfg(windows)]
    System(rustudio_windows_mic::mapping::Mapping),
}
impl Memory {
    fn shared(&self) -> &Shared {
        match self {
            Self::Sim(s) => s,
            #[cfg(windows)]
            Self::System(m) => m.shared(),
        }
    }
}
struct Link {
    memory: Memory,
    sent: Option<Controls>,
    samples: u32,
    last: Option<Instant>,
}

pub(crate) struct Native {
    simulated: bool,
    links: Mutex<BTreeMap<String, Link>>,
    last_check: Mutex<Option<Instant>>,
}

impl Native {
    pub fn new(simulated: bool) -> Self {
        Self {
            simulated,
            links: Mutex::new(BTreeMap::new()),
            last_check: Mutex::new(None),
        }
    }
    fn links(&self) -> std::sync::MutexGuard<'_, BTreeMap<String, Link>> {
        self.links.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn managed(&self, g: &str) -> bool {
        if self.simulated {
            return self.links().contains_key(g);
        }
        #[cfg(windows)]
        {
            rustudio_windows_mic::registry::managed()
                .is_ok_and(|v| v.iter().any(|k| k.eq_ignore_ascii_case(g)))
        }
        #[cfg(not(windows))]
        {
            false
        }
    }
    pub fn attached(&self, g: &str) -> bool {
        if self.simulated {
            return self.links().contains_key(g);
        }
        #[cfg(windows)]
        {
            rustudio_windows_mic::registry::attached(g)
        }
        #[cfg(not(windows))]
        {
            false
        }
    }

    pub fn sync(&self, g: &str, v: &Voice) -> Result<(), String> {
        self.publish(g, v, false)
    }

    fn publish(&self, g: &str, v: &Voice, explicit: bool) -> Result<(), String> {
        let mut links = self.links();
        let link = match links.entry(g.to_owned()) {
            std::collections::btree_map::Entry::Occupied(e) => e.into_mut(),
            std::collections::btree_map::Entry::Vacant(e) => {
                if !explicit && (self.simulated || !self.managed(g)) {
                    return Ok(());
                }
                let memory = if self.simulated {
                    Memory::Sim(Box::default())
                } else {
                    #[cfg(windows)]
                    {
                        Memory::System(
                            rustudio_windows_mic::mapping::Mapping::open(g, true)
                                .map_err(|e| format!("Mémoire partagée du micro : {e}"))?,
                        )
                    }
                    #[cfg(not(windows))]
                    {
                        return Err("Windows requis.".into());
                    }
                };
                e.insert(Link {
                    memory,
                    sent: None,
                    samples: 0,
                    last: None,
                })
            }
        };
        let controls = Controls::new(v, v.noise, v.strength, v.grace_ms);
        if link.sent != Some(controls) {
            link.memory.shared().publish(controls);
            link.sent = Some(controls);
        }
        Ok(())
    }

    pub fn attach(&self, g: &str, voice: &Voice, migrate: bool) -> Result<(), String> {
        self.publish(g, voice, true)?;
        if !self.simulated {
            #[cfg(windows)]
            if let Err(e) = rustudio_windows_mic::registry::attach(g, migrate) {
                self.links().remove(g);
                return Err(format!("Branchement du micro : {e}"));
            }
            #[cfg(not(windows))]
            {
                let _ = migrate;
                return Err("Windows requis.".into());
            }
        }
        Ok(())
    }

    pub fn remove(&self, g: &str) -> Result<(), String> {
        #[cfg(windows)]
        if !self.simulated {
            rustudio_windows_mic::registry::detach(g)
                .map_err(|e| format!("Restauration du micro : {e}"))?;
        }
        if let Some(link) = self.links().remove(g) {
            link.memory.shared().magic.store(0, Ordering::Release);
        }
        Ok(())
    }

    pub fn faults(&self) -> Vec<String> {
        let mut last = self.last_check.lock().unwrap_or_else(|e| e.into_inner());
        if last.is_some_and(|t| t.elapsed() < Duration::from_secs(5)) {
            return Vec::new();
        }
        let links = self.links();
        if links.is_empty() {
            return Vec::new();
        }
        *last = Some(Instant::now());
        #[cfg(windows)]
        let crashes = !self.simulated && repeated_audio_crash();
        #[cfg(not(windows))]
        let crashes = false;
        links
            .iter()
            .filter(|(_, l)| crashes || l.memory.shared().faults.load(Ordering::Acquire) > 0)
            .map(|(g, _)| g.clone())
            .collect()
    }

    pub fn status(&self, g: &str, v: &Voice) -> Value {
        let attached = self.attached(g);
        let levels = self.levels(g);
        json!({"attached":attached,"requested":v.native_requested,"safe_mode":v.safe_mode,"lost":v.native_requested && !attached,"levels":levels})
    }

    pub(crate) fn levels(&self, g: &str) -> Option<[f64; 5]> {
        let mut links = self.links();
        links.get_mut(g).and_then(|link| {
            let shared = link.memory.shared();
            let n = shared.callbacks.load(Ordering::Acquire);
            if n != link.samples {
                link.samples = n;
                link.last = Some(Instant::now());
            }
            link.last
                .filter(|t| t.elapsed() < Duration::from_secs(2))
                .map(|_| {
                    shared
                        .levels
                        .each_ref()
                        .map(|s| f64::from(s.load(Ordering::Relaxed)) / 256.)
                })
        })
    }
}

pub(crate) fn restart_audio(simulated: bool) -> Result<(), String> {
    if simulated {
        return Ok(());
    }
    #[cfg(windows)]
    {
        restart_windows_audio()
    }
    #[cfg(not(windows))]
    {
        Err("Le service audio requiert Windows.".into())
    }
}

#[cfg(windows)]
fn restart_windows_audio() -> Result<(), String> {
    use windows_service::{
        service::{ServiceAccess, ServiceState},
        service_manager::{ServiceManager, ServiceManagerAccess},
    };
    let error = |e: windows_service::Error| format!("Service audio de Windows : {e}");
    let manager = ServiceManager::local_computer(None::<&str>, ServiceManagerAccess::CONNECT)
        .map_err(error)?;
    let service = manager
        .open_service(
            "Audiosrv",
            ServiceAccess::QUERY_STATUS | ServiceAccess::START | ServiceAccess::STOP,
        )
        .map_err(error)?;
    let state = || {
        service
            .query_status()
            .map(|s| s.current_state)
            .map_err(error)
    };
    let wait = |wanted| -> Result<(), String> {
        let until = Instant::now() + Duration::from_secs(20);
        while state()? != wanted {
            if Instant::now() >= until {
                return Err(format!(
                    "Le service audio de Windows n’a pas atteint l’état {wanted:?}."
                ));
            }
            std::thread::sleep(Duration::from_millis(80));
        }
        Ok(())
    };
    let code = |e: &windows_service::Error| {
        if let windows_service::Error::Winapi(e) = e {
            e.raw_os_error()
        } else {
            None
        }
    };
    match state()? {
        ServiceState::Stopped | ServiceState::StopPending => {}
        current => {
            if current == ServiceState::StartPending {
                wait(ServiceState::Running)?;
            }
            if let Err(e) = service.stop() {
                match code(&e) {
                    Some(1051) => return restart_with_dependents(),
                    Some(1062) => {}
                    _ => return Err(error(e)),
                }
            }
        }
    }
    wait(ServiceState::Stopped)?;
    if let Err(e) = service.start::<&str>(&[]) {
        if code(&e) != Some(1056) {
            return Err(error(e));
        }
    }
    wait(ServiceState::Running)
}

// Certains pilotes ajoutent des services dépendants. Restaurer ceux qui tournaient
// avant le redémarrage, même si Windows refuse l'une des opérations.
#[cfg(windows)]
fn restart_with_dependents() -> Result<(), String> {
    let script = r#"$ErrorActionPreference='Stop'
function Active($s) { foreach ($d in $s.DependentServices) { if ($d.Status -eq 'Running') { $d; Active $d } } }
$s = Get-Service Audiosrv
$active = @(Active $s)
try { Restart-Service -InputObject $s -Force }
finally { foreach ($d in $active) { $d.Refresh(); if ($d.Status -eq 'Stopped') { Start-Service -InputObject $d } } }
$s.WaitForStatus('Running',[TimeSpan]::FromSeconds(20))"#;
    let result = crate::win::run_hidden(
        "powershell.exe",
        &["-NoProfile", "-NonInteractive", "-Command", script],
    )
    .ok_or("Impossible de redémarrer les services audio dépendants.")?;
    if result.status.success() {
        Ok(())
    } else {
        Err(format!(
            "Redémarrage des services audio dépendants refusé ({})",
            result.status
        ))
    }
}

#[cfg(windows)]
fn repeated_audio_crash() -> bool {
    use windows_sys::Win32::System::EventLog::*;
    let wide = |s: &str| s.encode_utf16().chain(Some(0)).collect::<Vec<_>>();
    let xpath="*[System[EventID=1000 and TimeCreated[timediff(@SystemTime) <= 120000]] and EventData[Data[@Name='ModuleName']='rustudio_micro_apo.dll' and Data[@Name='AppName']='audiodg.exe']]";
    unsafe {
        let q = EvtQuery(
            0,
            wide("Application").as_ptr(),
            wide(xpath).as_ptr(),
            EvtQueryChannelPath | EvtQueryReverseDirection,
        );
        if q == 0 {
            return false;
        }
        let mut found = 0;
        while found < 2 {
            let (mut event, mut n) = (0, 0);
            if EvtNext(q, 1, &mut event, 0, 0, &mut n) == 0 || n != 1 {
                break;
            }
            EvtClose(event);
            found += 1;
        }
        EvtClose(q);
        found >= 2
    }
}

#[cfg(windows)]
pub(crate) fn uninstall_effects() -> Result<(), String> {
    if rustudio_windows_mic::registry::detach_all().map_err(|e| e.to_string())? {
        restart_audio(false)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn un_defaut_audio_isole_le_micro_concerne() {
        let n = Native::new(true);
        for g in ["a", "b"] {
            n.attach(g, &Voice::default(), false).unwrap();
        }
        {
            let links = n.links();
            let shared = links["a"].memory.shared();
            shared.levels[2].store(768, Ordering::Release);
            shared.callbacks.store(1, Ordering::Release);
            shared.faults.store(1, Ordering::Release);
        }
        assert_eq!(n.levels("a").unwrap()[2], 3.);
        assert_eq!(n.faults(), ["a"]);
        n.remove("a").unwrap();
        n.sync("a", &Voice::default()).unwrap();
        assert!(!n.managed("a"));
        assert!(n.attached("b"));
    }
}
