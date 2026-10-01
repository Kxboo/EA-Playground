//! Minigame / HUD sound effects from the AEMS module banks (`audio/aems/*.abk`).
//!
//! Recovered from the executable: `AuAEMSManager::PlaySFX(AUDIOAEMSBESFX, ..)` (0x802dcb48, jump table at 0x804cbc10) and
//! `PlaySFX(AUDIOAEMSFEHUDSFX, ..)` (0x802dc20c) instantiate a Csis class per id and pass the variant as the class input;
//! the emulated switch bodies give the (class, variant) pairs in [`backend`] / [`hud`].  The tetherball and common banks'
//! classes are plain sample tables (`{count, {volume 16.16, 1-based sound, pad}*}` reached through the bank's pointer
//! fixups), so a class input is an index into its table.  Panning by azimuth and the class volume curves are not modelled.
use crate::{aems, audio, bridge};
use std::{
    collections::HashMap,
    sync::{Arc, Mutex, OnceLock},
};

pub struct SfxBank {
    /// Embedded `BNKb` sample bank.
    samples: Vec<u8>,
    /// Class name -> table of (volume, 1-based sound number).
    classes: HashMap<String, Vec<(f32, u32)>>,
}

fn be32(d: &[u8], o: usize) -> Option<u32> {
    d.get(o..o + 4).map(|b| u32::from_be_bytes(b.try_into().unwrap()))
}

impl SfxBank {
    pub fn parse(d: &[u8], csi_names: &std::collections::BTreeMap<u16, String>) -> Result<SfxBank, String> {
        let bank = aems::decode(d, csi_names)?;
        let samples = audio::abk_bank(d)?.ok_or("no sample bank")?.to_vec();
        let class_names: Vec<String> = bank.json["csis_bindings"].as_array().ok_or("no bindings")?.iter().filter(|b| b["kind"] == "class").map(|b| b["name"].as_str().unwrap_or("?").to_string()).collect();
        let classes_json = bank.json["classes"].as_array().ok_or("no classes")?;
        let data: Vec<usize> = classes_json.iter().map(|c| c["data"].as_u64().unwrap_or(0) as usize).collect();
        let fixups: Vec<usize> = bank.json["pointer_fixups"].as_array().ok_or("no fixups")?.iter().map(|f| f["offset"].as_u64().unwrap_or(0) as usize).collect();
        let mut classes = HashMap::new();
        for (i, name) in class_names.iter().enumerate() {
            let start = data.get(i).copied().unwrap_or(usize::MAX);
            let end = data.get(i + 1).copied().unwrap_or(d.len());
            let mut table = vec![];
            for &f in fixups.iter().filter(|&&f| f >= start && f < end) {
                let Some(t) = be32(d, f).map(|t| t as usize) else { continue };
                let Some(count) = be32(d, t).map(|c| c as usize).filter(|&c| (1..=64).contains(&c)) else { continue };
                let entries: Vec<(f32, u32)> = (0..count)
                    .filter_map(|k| Some((be32(d, t + 4 + 12 * k)? as f32 / 65536., be32(d, t + 8 + 12 * k)?)))
                    .filter(|(v, s)| *v > 0. && *v <= 200. && (1..=4096).contains(s))
                    .collect();
                if entries.len() == count {
                    table = entries;
                    break;
                }
            }
            if !table.is_empty() {
                classes.insert(name.clone(), table);
            }
        }
        Ok(SfxBank { samples, classes })
    }

    pub fn class(&self, name: &str) -> Option<&[(f32, u32)]> {
        self.classes.get(name).map(Vec::as_slice)
    }
}

static BANKS: OnceLock<Mutex<HashMap<String, Option<Arc<SfxBank>>>>> = OnceLock::new();
static PCM: OnceLock<Mutex<HashMap<(String, u32), Option<audio::Pcm>>>> = OnceLock::new();

pub fn bank(file: &str) -> Option<Arc<SfxBank>> {
    let m = BANKS.get_or_init(|| Mutex::new(HashMap::new()));
    if let Some(b) = m.lock().ok()?.get(file) {
        return b.clone();
    }
    let dir = bridge::data_root().join("files").join("data").join("audio").join("aems");
    let loaded = (|| {
        let d = std::fs::read(dir.join(file)).ok()?;
        let csi = std::fs::read(dir.join("playground_aems.csi")).ok()?;
        let names: std::collections::BTreeMap<u16, String> = crate::formats2::csi(&csi)
            .ok()
            .map(|v| {
                ["table1", "table2", "table3"]
                    .iter()
                    .flat_map(|t| v[*t].as_array().cloned().unwrap_or_default())
                    .filter_map(|e| Some((u16::from_str_radix(e["id"].as_str()?, 16).ok()?, e["name"].as_str()?.to_string())))
                    .collect()
            })
            .unwrap_or_default();
        SfxBank::parse(&d, &names).ok().map(Arc::new)
    })();
    m.lock().ok()?.insert(file.to_string(), loaded.clone());
    loaded
}

/// `AUDIOAEMSBESFX` ids used by tetherball -> (bank, class, input).  `None` input = the caller already drew the random
/// variant (the original draws it inside `PlaySFX`; see [`random_variants`]).
pub fn backend(id: i32) -> Option<(&'static str, &'static str, Option<usize>)> {
    Some(match id {
        7 => ("common sfx.abk", "MGSFX_Grunts_Male", None),
        8 => ("common sfx.abk", "MGSFX_Grunts_Female", None),
        9 => ("mg_tetherball.abk", "MGSFX_MEGAGrunts_Male", None),
        10 => ("mg_tetherball.abk", "MGSFX_MEGAGrunts_Female", None),
        17..=24 => ("mg_tetherball.abk", "MGSFX_Tetherball_Hits", Some((id - 17) as usize)),
        _ => return None,
    })
}

/// Variants drawn with `rmRandRange(0, n)` inside `PlaySFX` for the ids above.
pub fn random_variants(id: i32) -> usize {
    match id {
        7 | 8 => 7,
        9 | 10 => 4,
        _ => 0,
    }
}

/// `AUDIOAEMSFEHUDSFX` ids >= 0x1a: `MGSFX_CommonHUD(id - 0x1a)`, 0x25 / 0x26 `MGSFX_HUD_TB(0 / 1)`.  Ids below 22 are the
/// `UI_*` sounds (`fe_sfx`).
pub fn hud(id: i32) -> Option<(&'static str, &'static str, usize)> {
    match id {
        0x1a..=0x24 => Some(("common sfx.abk", "MGSFX_CommonHUD", (id - 0x1a) as usize)),
        0x25 | 0x26 => Some(("mg_tetherball.abk", "MGSFX_HUD_TB", (id - 0x25) as usize)),
        _ => None,
    }
}

fn pcm(file: &str, sound: u32) -> Option<audio::Pcm> {
    let cache = PCM.get_or_init(|| Mutex::new(HashMap::new()));
    let key = (file.to_string(), sound);
    if let Some(p) = cache.lock().ok()?.get(&key) {
        return p.clone();
    }
    let b = bank(file)?;
    let p = audio::decode_bank_sound(&b.samples, sound as usize - 1).ok();
    cache.lock().ok()?.insert(key, p.clone());
    p
}

/// Play table entry `index` of `class` from `file` at `volume` (1.0 = the table's own level).
pub fn play(file: &'static str, class: &'static str, index: usize, volume: f32) {
    if std::env::args().any(|a| a == "--mute") {
        return;
    }
    std::thread::spawn(move || {
        let Some(b) = bank(file) else { return };
        let Some(table) = b.class(class) else { return };
        let Some(&(vol, sound)) = table.get(index.min(table.len().saturating_sub(1))) else { return };
        if let Some(p) = pcm(file, sound) {
            crate::playback::play_once(&p, (vol / 100.) * volume);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn tetherball_and_common_tables_follow_the_banks() {
        let Some(tb) = bank("mg_tetherball.abk") else { return };
        let hits = tb.class("MGSFX_Tetherball_Hits").unwrap();
        assert_eq!(hits.iter().map(|e| e.1).collect::<Vec<_>>(), (1..=8).collect::<Vec<_>>());
        assert_eq!(tb.class("MGSFX_HUD_TB").unwrap().iter().map(|e| e.1).collect::<Vec<_>>(), vec![11, 12]);
        assert_eq!(tb.class("MGSFX_MEGAGrunts_Male").unwrap().len(), 5);
        let common = bank("common sfx.abk").unwrap();
        assert_eq!(common.class("MGSFX_CommonHUD").unwrap().iter().map(|e| e.1).collect::<Vec<_>>(), (38..=48).collect::<Vec<_>>());
        assert_eq!(common.class("MGSFX_Grunts_Male").unwrap().len(), 8);
        // every referenced sound decodes
        for (file, class) in [("mg_tetherball.abk", "MGSFX_Tetherball_Hits"), ("mg_tetherball.abk", "MGSFX_HUD_TB"), ("common sfx.abk", "MGSFX_CommonHUD")] {
            for (_, s) in bank(file).unwrap().class(class).unwrap() {
                assert!(pcm(file, *s).is_some(), "{file} {class} sound {s}");
            }
        }
    }
}
