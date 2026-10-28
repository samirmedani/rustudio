//! Lecture d'une release GitHub (GET /repos/…/releases/latest).
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Asset {
    pub url: String,
    pub size: Option<u64>,
    pub sha256: Option<String>,
    pub sha_url: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Release {
    pub version: String,
    pub tag: String,
    pub notes: String,
    pub url: String,
    pub published: Option<String>,
    pub asset: Option<Asset>,
}

pub struct Names<'a> {
    pub repo: &'a str,
    pub asset: &'a str,
    pub hash_asset: &'a str,
    pub download_prefix: &'a str,
}

/// None : release inutilisable (brouillon, pre-version, tag illisible).
pub fn parse_release(data: &Value, n: &Names) -> Option<Release> {
    let o = data.as_object()?;
    let flag = |k: &str| o.get(k).and_then(|v| v.as_bool()).unwrap_or(false);
    if flag("draft") || flag("prerelease") {
        return None;
    }
    let tag = o.get("tag_name").and_then(|v| v.as_str()).unwrap_or("");
    if !tag.starts_with('v') || crate::semver::parse(tag).is_none() {
        return None;
    }
    let notes: String = o.get("body").and_then(|v| v.as_str()).unwrap_or("").chars().take(20000).collect();
    let url = o
        .get("html_url")
        .and_then(|v| v.as_str())
        .map(String::from)
        .unwrap_or_else(|| format!("https://github.com/{}/releases/tag/{}", n.repo, tag));
    let published = o.get("published_at").and_then(|v| v.as_str()).map(String::from);
    let assets: Vec<&Value> = o.get("assets").and_then(|v| v.as_array()).map(|a| a.iter().collect()).unwrap_or_default();
    let find = |name: &str| assets.iter().copied().find(|a| a.get("name").and_then(|x| x.as_str()) == Some(name));
    let dl = |a: &Value| a.get("browser_download_url").and_then(|x| x.as_str()).unwrap_or("").to_string();
    let mut asset = None;
    if let Some(a) = find(n.asset) {
        let u = dl(a);
        if u.starts_with(n.download_prefix) {
            let digest = a.get("digest").and_then(|x| x.as_str()).unwrap_or("");
            let sha = (digest.starts_with("sha256:") && digest.len() == 71).then(|| digest[7..].to_lowercase());
            let sha_url = find(n.hash_asset).map(dl).filter(|u| u.starts_with(n.download_prefix));
            let size = a.get("size").and_then(|x| x.as_u64()).filter(|s| *s > 0);
            asset = Some(Asset { url: u, size, sha256: sha, sha_url });
        }
    }
    Some(Release { version: tag[1..].to_string(), tag: tag.into(), notes, url, published, asset })
}
