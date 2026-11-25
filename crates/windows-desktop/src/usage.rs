//! Consommation du processus courant, sans énumérer les autres applications.
use std::time::Instant;

#[derive(Clone, Copy, Debug, Default)]
pub struct Usage {
    /// Part de la capacité CPU disponible pour ce processus, de 0 à 100 %.
    pub cpu_percent: Option<f64>,
    /// Mémoire physique actuellement résidente (working set), en octets.
    pub resident_bytes: Option<u64>,
    /// Mémoire physique du PC, en octets.
    pub total_bytes: Option<u64>,
}

pub struct Sampler {
    previous: Option<(Instant, u64)>,
    processors: usize,
}

impl Default for Sampler {
    fn default() -> Self {
        Self {
            previous: None,
            processors: std::thread::available_parallelism()
                .map(usize::from)
                .unwrap_or(1),
        }
    }
}

impl Sampler {
    pub fn sample(&mut self) -> Usage {
        let (cpu_ticks, resident_bytes) = current_process();
        let now = Instant::now();
        let cpu_percent = self.cpu_at(now, cpu_ticks);
        Usage {
            cpu_percent,
            resident_bytes,
            total_bytes: total_memory(),
        }
    }

    // GetProcessTimes compte des unités de 100 ns, cumulées sur tous les fils.
    fn cpu_at(&mut self, now: Instant, ticks: Option<u64>) -> Option<f64> {
        let previous = self.previous.take();
        let ticks = ticks?;
        self.previous = Some((now, ticks));
        let (at, old) = previous?;
        let elapsed = now.checked_duration_since(at)?.as_secs_f64();
        if elapsed <= 0.0 {
            return None;
        }
        let used = ticks.checked_sub(old)? as f64 / 10_000_000.0;
        Some((used / elapsed / self.processors.max(1) as f64 * 100.0).clamp(0.0, 100.0))
    }
}

#[cfg(windows)]
fn current_process() -> (Option<u64>, Option<u64>) {
    use windows::Win32::{
        Foundation::FILETIME,
        System::{
            ProcessStatus::{GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS},
            Threading::{GetCurrentProcess, GetProcessTimes},
        },
    };
    // Pseudo-handle Windows : aucune ouverture, aucun droit administrateur ni fermeture.
    unsafe {
        let process = GetCurrentProcess();
        let mut created = FILETIME::default();
        let mut exited = FILETIME::default();
        let mut kernel = FILETIME::default();
        let mut user = FILETIME::default();
        let ticks = |t: FILETIME| ((t.dwHighDateTime as u64) << 32) | t.dwLowDateTime as u64;
        let cpu = GetProcessTimes(process, &mut created, &mut exited, &mut kernel, &mut user)
            .ok()
            .map(|_| ticks(kernel).saturating_add(ticks(user)));
        let size = std::mem::size_of::<PROCESS_MEMORY_COUNTERS>() as u32;
        let mut memory = PROCESS_MEMORY_COUNTERS {
            cb: size,
            ..Default::default()
        };
        let ram = GetProcessMemoryInfo(process, &mut memory, size)
            .ok()
            .map(|_| memory.WorkingSetSize as u64);
        (cpu, ram)
    }
}

#[cfg(windows)]
fn total_memory() -> Option<u64> {
    use windows::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};
    let mut status = MEMORYSTATUSEX { dwLength: std::mem::size_of::<MEMORYSTATUSEX>() as u32, ..Default::default() };
    unsafe { GlobalMemoryStatusEx(&mut status) }.ok().map(|_| status.ullTotalPhys)
}

#[cfg(not(windows))]
fn total_memory() -> Option<u64> {
    None
}

#[cfg(not(windows))]
fn current_process() -> (Option<u64>, Option<u64>) {
    (None, None)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn cpu_uses_elapsed_time_and_all_processors() {
        let mut sampler = Sampler {
            previous: None,
            processors: 4,
        };
        let now = Instant::now();
        assert_eq!(sampler.cpu_at(now, Some(10_000_000)), None);
        // Deux secondes CPU en deux secondes réelles sur quatre processeurs = 25 %.
        assert_eq!(
            sampler.cpu_at(now + Duration::from_secs(2), Some(30_000_000)),
            Some(25.0)
        );
        assert_eq!(
            sampler.cpu_at(now + Duration::from_secs(3), Some(30_000_000)),
            Some(0.0)
        );
    }

    #[test]
    fn failed_read_requires_a_fresh_baseline() {
        let mut sampler = Sampler::default();
        let now = Instant::now();
        sampler.cpu_at(now, Some(10));
        assert_eq!(sampler.cpu_at(now + Duration::from_secs(1), None), None);
        assert_eq!(
            sampler.cpu_at(now + Duration::from_secs(2), Some(100)),
            None
        );
        assert_eq!(sampler.cpu_at(now + Duration::from_secs(3), Some(90)), None);
    }

    #[cfg(windows)]
    #[test]
    fn reads_the_running_process_without_elevation() {
        let mut sampler = Sampler::default();
        let first = sampler.sample();
        assert!(first.resident_bytes.is_some_and(|bytes| bytes > 0));
        assert!(first.cpu_percent.is_none());
        std::thread::sleep(Duration::from_millis(30));
        let second = sampler.sample();
        assert!(
            second
                .cpu_percent
                .is_some_and(|cpu| (0.0..=100.0).contains(&cpu))
        );
        assert!(second.resident_bytes.is_some_and(|bytes| bytes > 0));
    }
}
