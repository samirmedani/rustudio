//! Lecture tolerante de JSON, avec les memes regles que l'ancien moteur Python.
use serde_json::Value;

/// Nombre depuis un JSON (nombre ou texte), comme float() en Python.
pub fn num(v: &Value) -> Option<f64> {
    match v {
        Value::Number(n) => n.as_f64(),
        Value::String(s) => s.trim().parse::<f64>().ok(),
        Value::Bool(b) => Some(if *b { 1.0 } else { 0.0 }),
        _ => None,
    }
    .filter(|f| f.is_finite())
}

/// int(round(float(v))) borne entre lo et hi (arrondi au pair, comme Python).
pub fn clamp_f(f: f64, lo: i32, hi: i32) -> i32 {
    let r = f.round_ties_even();
    let r = if r < i32::MIN as f64 { i32::MIN } else if r > i32::MAX as f64 { i32::MAX } else { r as i32 };
    r.clamp(lo, hi)
}

pub fn clamp(v: &Value, lo: i32, hi: i32) -> Option<i32> {
    num(v).map(|f| clamp_f(f, lo, hi))
}

/// bool(x) en Python.
pub fn truthy(v: &Value) -> bool {
    match v {
        Value::Null => false,
        Value::Bool(b) => *b,
        Value::Number(n) => n.as_f64().map(|f| f != 0.0).unwrap_or(false),
        Value::String(s) => !s.is_empty(),
        Value::Array(a) => !a.is_empty(),
        Value::Object(o) => !o.is_empty(),
    }
}

/// parse_bool de l'API : « on », « true », « oui »... pour un texte, sinon bool(x).
pub fn parse_bool(v: &Value) -> bool {
    match v {
        Value::String(s) => matches!(s.to_lowercase().as_str(), "1" | "on" | "true" | "oui" | "yes"),
        other => truthy(other),
    }
}

/// 0-255, ou « NN% ».
pub fn parse_volume(v: &Value) -> Result<i32, String> {
    let s = match v {
        Value::String(s) => s.trim().to_string(),
        other => other.to_string(),
    };
    if let Some(p) = s.strip_suffix('%') {
        let f: f64 = p.trim().parse().map_err(|_| format!("volume invalide: {s}"))?;
        return Ok(clamp_f(f * 255.0 / 100.0, 0, 255));
    }
    let f: f64 = s.parse().map_err(|_| format!("volume invalide: {s}"))?;
    Ok((f.trunc() as i64).clamp(0, 255) as i32)
}

pub fn str_of(v: &Value) -> Option<&str> {
    v.as_str()
}

/// str(x) en Python pour un JSON simple.
pub fn py_str(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Null => "None".into(),
        Value::Bool(true) => "True".into(),
        Value::Bool(false) => "False".into(),
        other => other.to_string(),
    }
}

pub fn get<'a>(v: &'a Value, key: &str) -> &'a Value {
    static NULL: Value = Value::Null;
    v.get(key).unwrap_or(&NULL)
}

pub fn has(v: &Value, key: &str) -> bool {
    v.as_object().map(|o| o.contains_key(key)).unwrap_or(false)
}

pub fn first_chars(s: &str, n: usize) -> String {
    s.chars().take(n).collect()
}
