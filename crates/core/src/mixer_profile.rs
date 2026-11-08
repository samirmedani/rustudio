//! Réglages portables du mixeur uniquement. Aucun identifiant, routage ou traitement vocal.
use crate::consts::{channel_input, mute_target, CHANNELS, FADERS};
use crate::State;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MixerSettings {
    pub volumes: BTreeMap<String, u8>,
    pub faders: BTreeMap<String, String>,
    pub mute_modes: BTreeMap<String, String>,
    pub cuts: BTreeMap<String, Option<String>>,
}

impl MixerSettings {
    pub fn validate(&self) -> Result<(), &'static str> {
        let valid_channels: BTreeSet<_> = CHANNELS.into_iter().collect();
        if self.volumes.keys().map(String::as_str).collect::<BTreeSet<_>>() != valid_channels
            || self.cuts.keys().map(String::as_str).collect::<BTreeSet<_>>() != valid_channels
        {
            return Err("Le profil doit contenir les onze voies du mixeur.");
        }
        if self.faders.len() != FADERS.len() || self.mute_modes.len() != FADERS.len()
            || FADERS.iter().any(|&id| !self.faders.contains_key(id)
                || !self.mute_modes.get(id).is_some_and(|mode| mute_target(mode).is_some()))
            || self.faders.values().any(|id| !CHANNELS.contains(&id.as_str()))
            || self.faders.values().collect::<BTreeSet<_>>().len() != FADERS.len()
        {
            return Err("Affectations ou cibles des quatre faders invalides.");
        }
        if self.cuts.iter().any(|(id, cut)| cut.as_ref().is_some_and(|target|
            mute_target(target).is_none() || (target != "all" && channel_input(id).is_none())))
        {
            return Err("Coupure invalide dans le profil.");
        }
        Ok(())
    }

    pub fn apply_to(&self, state: &mut State) -> Result<(), &'static str> {
        self.validate()?;
        for &id in &CHANNELS {
            state.volumes.insert(id.into(), self.volumes[id].into());
            let cut = self.cuts[id].as_deref();
            state.mutes.insert(id.into(), cut == Some("all"));
            state.partial.insert(id.into(), cut.filter(|&t| t != "all").map(str::to_owned));
        }
        for &id in &FADERS {
            state.faders.insert(id.into(), self.faders[id].clone());
            state.mute_modes.insert(id.into(), self.mute_modes[id].clone());
        }
        state.scene = None;
        Ok(())
    }
}
