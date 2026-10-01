//! Frontend sound effects (`PlayAEMSsfx?nFEsfxID=UI_*`).
//!
//! Evidence: the executable's name table (`UI_RecessBell` ... `UI_FETally_Stop`, 22 entries before the HUD names) is in the
//! order of `AUDIOAEMSFEHUDSFX`; `AuAEMSManager::PlaySFX` (0x802dc20c) clamps the value to 0..21 and instantiates the
//! `Csis::UI_Sfx` module with it.  That module's data in `fe_ui.abk` (switch table at 0x5f8 = identity, sound-player node at
//! 0x840 with 22 entries) lists the 1-based bank sound for each value; `SAMPLE_IDS` is that list.
use crate::{audio,bridge};
use std::{collections::HashMap,sync::{Mutex,OnceLock}};

pub const NAMES:[&str;22]=["UI_RecessBell","UI_Confirm","UI_BigConfirm","UI_Erase","UI_Select","UI_Invalid","UI_StickerPlace","UI_StickerPick","UI_AreaTitle",
    "UI_Sticker","UI_PageTurn","UI_WiimoteSynch","UI_Highlight","UI_Purchase","UI_Pickup","UI_FinalSticker","UI_TourneyWin","UI_Scroll",
    "UI_MarblesTally_Start","UI_MarblesTally_Stop","UI_FETally_Start","UI_FETally_Stop"];
/// 1-based sound numbers of `fe_ui.abk`, indexed by the `UI_Sfx` parameter.
pub const SAMPLE_IDS:[usize;22]=[1,2,3,4,6,7,8,9,10,11,12,13,14,15,16,17,18,19,20,21,22,5];

static BANK:OnceLock<Option<Vec<u8>>>=OnceLock::new();
static CACHE:OnceLock<Mutex<HashMap<usize,Option<audio::Pcm>>>>=OnceLock::new();

fn bank()->Option<&'static [u8]>{
    BANK.get_or_init(||{
        let p=bridge::data_root().join("files").join("data").join("audio").join("aems").join("fe_ui.abk");
        let d=std::fs::read(p).ok()?;
        audio::abk_bank(&d).ok().flatten().map(|b|b.to_vec())
    }).as_deref()
}

/// Decoded sound for a table index (cached).
pub fn pcm(index:usize)->Option<audio::Pcm>{
    let cache=CACHE.get_or_init(||Mutex::new(HashMap::new()));
    if let Some(p)=cache.lock().ok()?.get(&index){return p.clone()}
    let p=bank().and_then(|b|audio::decode_bank_sound(b,SAMPLE_IDS.get(index)?-1).ok());
    cache.lock().ok()?.insert(index,p.clone());p
}

/// Play the effect for an `nFEsfxID` name; unknown names (the movies reference a few the game has no entry for) are ignored.
pub fn play(name:&str){
    if std::env::args().any(|a|a=="--mute"){return}
    let Some(i)=NAMES.iter().position(|n|*n==name) else{return};
    std::thread::spawn(move||{if let Some(p)=pcm(i){crate::playback::play_once(&p,0.7);}});
}

#[cfg(test)]
mod tests{
    use super::*;
    #[test] fn every_ui_sound_decodes(){
        if bank().is_none(){eprintln!("DATA absent; skipped");return}
        for (i,n) in NAMES.iter().enumerate(){
            let p=pcm(i).unwrap_or_else(||panic!("{n} did not decode"));
            eprintln!("{n}: {} samples {} ch {} Hz",p.samples.len(),p.channels,p.sample_rate);
            assert!(!p.samples.is_empty());
            // 28-sample EA-XA block seams: an interleaved second channel would show large jumps there.
            let (mut sb,mut nb,mut so,mut no)=(0f64,0u32,0f64,0u32);
            for k in 1..p.samples.len(){let d=(p.samples[k] as f64-p.samples[k-1] as f64).abs();if k%28==0{sb+=d;nb+=1}else{so+=d;no+=1}}
            if nb>0&&no>0{eprintln!("  seam ratio {:.2}",(sb/nb as f64)/(so/no as f64).max(1e-9));}
        }
    }
}
