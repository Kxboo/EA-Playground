//! Shared game-facing asset contract. No invented gameplay is labelled original.
use bevy::prelude::*;
use serde::{Deserialize,Serialize};

#[derive(Component,Debug,Clone,Serialize,Deserialize)]
pub struct OriginalAsset {
    pub source:String,
    pub decoder_version:String,
    pub evidence:EvidenceLevel,
}

#[derive(Debug,Clone,Serialize,Deserialize,PartialEq)]
pub enum EvidenceLevel { Unresolved, AssetDerived, ExecutableDerived, GameplayCompared }

