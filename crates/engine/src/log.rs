//! Journal avec anti-repetition : une erreur en boucle ne noie pas le fichier.
use std::collections::VecDeque;
use std::io::Write;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};

struct LogState {
    path: Option<PathBuf>,
    last: Option<String>,
    n: u64,
    t: f64,
    recent: VecDeque<String>,
    prefix: String,
    echo: bool,
}

fn st() -> &'static Mutex<LogState> {
    static S: OnceLock<Mutex<LogState>> = OnceLock::new();
    S.get_or_init(|| {
        Mutex::new(LogState { path: None, last: None, n: 0, t: 0.0, recent: VecDeque::new(), prefix: String::new(), echo: true })
    })
}

pub fn set_file(path: PathBuf) {
    st().lock().unwrap().path = Some(path);
}
pub fn set_prefix(p: &str) {
    st().lock().unwrap().prefix = p.into();
}
pub fn set_echo(on: bool) {
    st().lock().unwrap().echo = on;
}
pub fn recent(n: usize) -> Vec<String> {
    let s = st().lock().unwrap();
    s.recent.iter().skip(s.recent.len().saturating_sub(n)).cloned().collect()
}

fn write_line(s: &mut LogState, line: String) {
    if s.recent.len() >= 80 {
        s.recent.pop_front();
    }
    s.recent.push_back(line.clone());
    if s.echo {
        println!("{line}");
    }
    let Some(path) = s.path.clone() else { return };
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    if std::fs::metadata(&path).map(|m| m.len() > 2_000_000).unwrap_or(false) {
        for i in [2, 1] {
            let from = PathBuf::from(format!("{}.{i}", path.display()));
            if from.exists() {
                let _ = std::fs::rename(&from, format!("{}.{}", path.display(), i + 1));
            }
        }
        let _ = std::fs::rename(&path, format!("{}.1", path.display()));
    }
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&path) {
        let _ = writeln!(f, "{line}");
    }
}

pub fn log(msg: impl AsRef<str>) {
    let now = crate::config::now_epoch();
    let stamp = chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
    let mut s = st().lock().unwrap();
    let msg = format!("{}{}", s.prefix, msg.as_ref());
    if s.last.as_deref() == Some(msg.as_str()) {
        s.n += 1;
        if now - s.t >= 60.0 {
            let n = s.n;
            write_line(&mut s, format!("{stamp} {msg} (encore {n} fois depuis la dernière ligne)"));
            s.n = 0;
            s.t = now;
        }
        return;
    }
    if s.n > 0 {
        let n = s.n;
        write_line(&mut s, format!("{stamp} (ligne précédente répétée {n} fois)"));
    }
    s.last = Some(msg.clone());
    s.n = 0;
    s.t = now;
    write_line(&mut s, format!("{stamp} {msg}"));
}

#[macro_export]
macro_rules! logf {
    ($($a:tt)*) => { $crate::log::log(format!($($a)*)) };
}
