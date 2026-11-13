//! Notes de version (CHANGELOG et releases GitHub), lues comme dans Wispad : une phrase d'accroche, puis les
//! rubriques Nouveau, Amélioré et Corrigé, et leurs points « **Titre** : détail ».
//!
//! Format d'une version dans CHANGELOG.md :
//!
//! ```text
//! ## 6.2.2 — 13 novembre 2026
//!
//! Rustudio 6.2.2 te montre toute la mise à jour, des nouveautés jusqu'au redémarrage.
//!
//! **Nouveau**
//! - **Titre court** : ce qui change pour toi, en une ou deux phrases.
//!
//! **Amélioré**
//! - …
//!
//! **Corrigé**
//! - …
//! ```
//!
//! Les anciennes notes, sans rubrique ni accroche, restent lisibles : leurs points forment une seule liste.

/// Rubrique d'une version.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    New,
    Improved,
    Fixed,
    /// intertitre libre, ou points avant toute rubrique (anciennes notes)
    Other,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Item {
    /// partie en gras au début du point, sans sa ponctuation finale
    pub title: Option<String>,
    /// suite du point (peut contenir du **gras**)
    pub detail: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Section {
    pub kind: Kind,
    /// nom affiché (« Nouveau »…, ou l'intertitre libre ; vide pour des points sans rubrique)
    pub label: String,
    pub items: Vec<Item>,
}

#[derive(Clone, Debug, PartialEq, Default)]
pub struct Notes {
    /// accroche : le texte avant la première rubrique ou le premier point
    pub lead: Option<String>,
    pub sections: Vec<Section>,
}

/// Une version du CHANGELOG.
#[derive(Clone, Debug, PartialEq)]
pub struct Entry {
    pub version: String,
    /// date écrite après le numéro (« 13 novembre 2026 »), si elle y est
    pub date: Option<String>,
    pub notes: String,
}

fn kind_of(label: &str) -> Option<Kind> {
    let l = label.trim().trim_end_matches([':', '.']).trim().to_lowercase();
    match l.as_str() {
        "nouveau" | "nouveautés" | "nouveautes" => Some(Kind::New),
        "amélioré" | "améliorations" | "ameliore" => Some(Kind::Improved),
        "corrigé" | "corrections" | "corrige" => Some(Kind::Fixed),
        _ => None,
    }
}

/// Texte entièrement en gras sur sa ligne (« **Nouveau** »), sans les étoiles.
fn whole_bold(line: &str) -> Option<&str> {
    let inner = line.strip_prefix("**")?.strip_suffix("**")?;
    (!inner.contains("**")).then_some(inner)
}

fn item(text: &str) -> Item {
    if let Some(rest) = text.strip_prefix("**") {
        if let Some(end) = rest.find("**") {
            let title = rest[..end].trim().trim_end_matches(['.', ':']).trim().to_string();
            let detail = rest[end + 2..].trim().trim_start_matches([':', '.', '—', '-']).trim().to_string();
            if detail.is_empty() {
                return Item { title: None, detail: title };
            }
            // affiché sous son titre, le détail commence une phrase
            let mut chars = detail.chars();
            let detail = chars.next().map(|c| c.to_uppercase().chain(chars).collect()).unwrap_or_default();
            return Item { title: Some(title), detail };
        }
    }
    Item { title: None, detail: text.trim().to_string() }
}

pub fn parse(md: &str) -> Notes {
    let mut notes = Notes::default();
    let mut lead: Vec<String> = vec![];
    for raw in md.lines() {
        let line = raw.trim();
        // les citations (conseils pour une ancienne version) et l'empreinte ne servent que sur GitHub
        if line.is_empty() || line.starts_with('>') || line.contains("SHA-256") || line.starts_with("---") {
            continue;
        }
        let heading = line.strip_prefix("#### ").or_else(|| line.strip_prefix("### "));
        let label = heading.or_else(|| whole_bold(line));
        if let Some(label) = label {
            match kind_of(label) {
                Some(kind) => {
                    notes.sections.push(Section { kind, label: label.trim().trim_end_matches(':').trim().to_string(), items: vec![] });
                    continue;
                }
                None if heading.is_some() => {
                    notes.sections.push(Section { kind: Kind::Other, label: label.trim().to_string(), items: vec![] });
                    continue;
                }
                None => {}
            }
        }
        if let Some(text) = line.strip_prefix("- ").or_else(|| line.strip_prefix("* ")) {
            if notes.sections.is_empty() {
                notes.sections.push(Section { kind: Kind::Other, label: String::new(), items: vec![] });
            }
            notes.sections.last_mut().unwrap().items.push(item(text));
        } else if notes.sections.is_empty() {
            lead.push(line.to_string());
        } else {
            notes.sections.last_mut().unwrap().items.push(Item { title: None, detail: line.to_string() });
        }
    }
    notes.lead = (!lead.is_empty()).then(|| lead.join(" "));
    notes.sections.retain(|s| !s.items.is_empty());
    notes
}

/// Notes composées par le moteur (« #### Aussi dans la x.y.z ») : (version, notes), la première sans version.
pub fn split_composed(md: &str) -> Vec<(Option<String>, String)> {
    let mut out: Vec<(Option<String>, String)> = vec![(None, String::new())];
    for line in md.lines() {
        if let Some(v) = line.trim().strip_prefix("#### Aussi dans la ") {
            out.push((Some(v.trim().to_string()), String::new()));
            continue;
        }
        let cur = &mut out.last_mut().unwrap().1;
        cur.push_str(line);
        cur.push('\n');
    }
    out.into_iter().map(|(v, n)| (v, n.trim().to_string())).filter(|(_, n)| !n.is_empty()).collect()
}

/// Sections « ## x.y.z » du CHANGELOG, dans l'ordre du fichier. « ## À venir » est ignoré.
pub fn changelog(text: &str) -> Vec<Entry> {
    let mut out: Vec<Entry> = vec![];
    let mut cur: Option<Entry> = None;
    for line in text.lines() {
        if let Some(h) = line.strip_prefix("## ") {
            out.extend(cur.take());
            let h = h.trim().trim_start_matches('[');
            let v = h.split([']', ' ']).next().unwrap_or("").to_string();
            let date = h.split_once(['—', '–']).map(|(_, d)| d.trim().to_string()).filter(|d| !d.is_empty());
            cur = crate::semver::parse(&v).is_some().then(|| Entry { version: v, date, notes: String::new() });
            continue;
        }
        if let Some(e) = cur.as_mut() {
            e.notes.push_str(line);
            e.notes.push('\n');
        }
    }
    out.extend(cur);
    out.into_iter().map(|e| Entry { notes: e.notes.trim().to_string(), ..e }).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rubriques_accroche_et_titres() {
        let n = parse("Rustudio 6.2.2 te montre toute la mise à jour.\n\n**Nouveau**\n- **Fenêtre de mise à jour** : les notes avant d'installer.\n- Un point sans titre.\n\n**Corrigé**\n- **Plus de saut.** La page ne bouge plus.\n\n> Conseil pour la 5.0.\n");
        assert_eq!(n.lead.as_deref(), Some("Rustudio 6.2.2 te montre toute la mise à jour."));
        assert_eq!(n.sections.len(), 2);
        assert_eq!((n.sections[0].kind, n.sections[0].label.as_str()), (Kind::New, "Nouveau"));
        assert_eq!(n.sections[0].items[0], Item { title: Some("Fenêtre de mise à jour".into()), detail: "Les notes avant d'installer.".into() });
        assert_eq!(n.sections[0].items[1].title, None);
        assert_eq!(n.sections[1].kind, Kind::Fixed);
        assert_eq!(n.sections[1].items[0], Item { title: Some("Plus de saut".into()), detail: "La page ne bouge plus.".into() });
    }

    #[test]
    fn anciennes_notes_sans_rubrique() {
        let n = parse("- **Latence en direct.** Page Diagnostic.\n- « Radio » s’appelle maintenant « Broadcast ».\n#### Aussi dans la 6.1.0\n- **Tout gras**\n");
        assert_eq!(n.lead, None);
        assert_eq!((n.sections[0].kind, n.sections[0].label.as_str(), n.sections[0].items.len()), (Kind::Other, "", 2));
        assert_eq!(n.sections[1].label, "Aussi dans la 6.1.0");
        assert_eq!(n.sections[1].items[0], Item { title: None, detail: "Tout gras".into() });
    }

    #[test]
    fn notes_composees_par_le_moteur() {
        let parts = split_composed("**Nouveau**\n- b\n\n#### Aussi dans la 6.2.1\n\n- a\n\n#### Aussi dans la 6.2.0\n\n- z");
        assert_eq!(parts, vec![(None, "**Nouveau**\n- b".into()), (Some("6.2.1".into()), "- a".into()), (Some("6.2.0".into()), "- z".into())]);
    }

    #[test]
    fn changelog_date_et_a_venir_ignore() {
        let e = changelog("# Nouveautés\n\n## À venir\n- x\n\n## 6.2.2 — 13 novembre 2026\n\nAccroche.\n\n## 6.2.1\n- y\n");
        assert_eq!(e.len(), 2);
        assert_eq!((e[0].version.as_str(), e[0].date.as_deref(), e[0].notes.as_str()), ("6.2.2", Some("13 novembre 2026"), "Accroche."));
        assert_eq!((e[1].version.as_str(), e[1].date.as_deref()), ("6.2.1", None));
    }
}
