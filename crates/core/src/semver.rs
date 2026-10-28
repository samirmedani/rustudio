//! Versions semver (« v1.2.3 », « 1.2.3-beta.1 »).

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Version {
    pub major: u64,
    pub minor: u64,
    pub patch: u64,
    pub pre: Vec<String>,
}

pub fn parse(text: &str) -> Option<Version> {
    let mut s = text.trim();
    if s.starts_with('v') || s.starts_with('V') {
        s = &s[1..];
    }
    let s = s.split('+').next().unwrap_or("");
    let (core, pre) = match s.split_once('-') {
        Some((c, p)) => (c, p),
        None => (s, ""),
    };
    let nums: Vec<&str> = core.split('.').collect();
    if nums.len() != 3 || !nums.iter().all(|n| !n.is_empty() && n.chars().all(|c| c.is_ascii_digit()) && (*n == "0" || !n.starts_with('0'))) {
        return None;
    }
    let parts: Vec<String> = if pre.is_empty() { vec![] } else { pre.split('.').map(String::from).collect() };
    if !pre.is_empty() && !parts.iter().all(|p| !p.is_empty() && p.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')) {
        return None;
    }
    Some(Version { major: nums[0].parse().ok()?, minor: nums[1].parse().ok()?, patch: nums[2].parse().ok()?, pre: parts })
}

/// a < b (une pre-version passe avant la version finale) ; faux si l'une est illisible.
pub fn lt(a: &str, b: &str) -> bool {
    let (Some(va), Some(vb)) = (parse(a), parse(b)) else { return false };
    let (ca, cb) = ((va.major, va.minor, va.patch), (vb.major, vb.minor, vb.patch));
    if ca != cb {
        return ca < cb;
    }
    let (pa, pb) = (&va.pre, &vb.pre);
    if pa.is_empty() || pb.is_empty() {
        return !pa.is_empty() && pb.is_empty();
    }
    for (x, y) in pa.iter().zip(pb.iter()) {
        if x == y {
            continue;
        }
        let (xd, yd) = (x.chars().all(|c| c.is_ascii_digit()), y.chars().all(|c| c.is_ascii_digit()));
        if xd && yd {
            return x.parse::<u64>().unwrap_or(0) < y.parse::<u64>().unwrap_or(0);
        }
        if xd != yd {
            return xd;
        }
        return x < y;
    }
    pa.len() < pb.len()
}
