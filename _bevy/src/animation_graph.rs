//! CSV AnimationStateGraph initialization (0x803c92d8), with decoded bank names.
//! This is asset mapping; native AnimationState playback remains a separate port.
use crate::{anim, archive, skeleton::Skeleton};
use std::{collections::BTreeMap, path::Path, sync::LazyLock};

pub const UNKNOWN_STATE: usize = 247;
static NAMES: LazyLock<Vec<String>> = LazyLock::new(|| {
    serde_json::from_str(include_str!("animation_state_names.json")).expect("ELF state table")
});
pub fn state_id(name: &str) -> usize {
    NAMES
        .iter()
        .position(|n| n == name)
        .unwrap_or(UNKNOWN_STATE)
}
#[derive(Clone, Debug)]
pub struct Event {
    pub name: String,
    pub time: f32,
}
#[derive(Clone, Debug)]
pub struct StateInfo {
    pub asset: String,
    pub clips: Vec<usize>,
    pub looping: bool,
    pub reverse: bool,
    pub random: bool,
    pub frame_time: bool,
    pub blend: f32,
    pub start: f32,
    pub trim: f32,
    pub next: usize,
    pub events: [Option<Event>; 2],
    pub props: [String; 3],
}
#[derive(Clone, Debug)]
pub struct Graph {
    pub states: Vec<Option<StateInfo>>,
}
impl Graph {
    pub fn new() -> Self {
        Self {
            states: vec![None; 246],
        }
    }
    /// Rows are decoded CSV service results. Unknown states are skipped. Clip
    /// count is cleared before lookup; missing clips preserve existing metadata.
    /// At most two alternatives are attempted, including failed lookups.
    pub fn apply_rows(
        &mut self,
        rows: &[BTreeMap<String, String>],
        bank_names: &[String],
    ) -> Result<(), String> {
        for row in rows {
            let get = |key: &str| row.get(key).map(String::as_str).unwrap_or("");
            let id = state_id(get("ANIM_STATE"));
            if id == UNKNOWN_STATE {
                continue;
            }
            if let Some(old) = self.states[id].as_mut() {
                old.clips.clear();
            }
            let asset = get("ANIM_ASSET");
            let clips: Vec<usize> = asset
                .split('|')
                .take(2)
                .filter_map(|name| find_clip(bank_names, &format!("S_{name}")))
                .collect();
            if clips.is_empty() {
                continue;
            }
            let number = |key: &str| {
                get(key)
                    .parse::<f32>()
                    .map_err(|_| format!("{}: invalid {key}", get("ANIM_STATE")))
            };
            let flag = |key: &str| {
                get(key)
                    .parse::<i32>()
                    .map(|n| n != 0)
                    .map_err(|_| format!("{}: invalid {key}", get("ANIM_STATE")))
            };
            let mut events = [None, None];
            for (i, event) in events.iter_mut().enumerate() {
                let name = get(&format!("EVENT_NAME{}", i + 1));
                if !name.is_empty() {
                    *event = Some(Event {
                        name: name.into(),
                        time: number(&format!("EVENT_TIME{}", i + 1))?,
                    });
                }
            }
            self.states[id] = Some(StateInfo {
                asset: asset.into(),
                clips,
                looping: flag("ANIM_LOOPING")?,
                reverse: flag("ANIM_REVERSE")?,
                random: flag("RANDOM_START_TIME")?,
                frame_time: flag("USE_FRAMETIME")?,
                blend: number("ANIM_BLEND_TIME")?,
                start: number("START_TIME")?,
                trim: number("TRUNCATE_TIME")?,
                next: state_id(get("ANIM_NEXT_STATE")),
                events,
                props: ["CS_L_PROP_BONE", "CS_R_PROP_BONE", "CS_BODY_PROP_BONE"]
                    .map(|key| get(key).into()),
            });
        }
        Ok(())
    }
}
/// GetAnimIndex 0x803fca94: case-sensitive binary search of the sorted name table.
fn find_clip(names: &[String], name: &str) -> Option<usize> {
    let (mut low, mut high) = (0i32, names.len() as i32 - 1);
    while low <= high {
        let mid = ((low + high) >> 1) as usize;
        match name.cmp(&names[mid]) {
            std::cmp::Ordering::Less => high = mid as i32 - 1,
            std::cmp::Ordering::Greater => low = mid as i32 + 1,
            std::cmp::Ordering::Equal => return Some(mid),
        }
    }
    None
}
/// Same unquoted cCSVParser grammar already verified by control_bindings.
/// Missing headers/unsafe buffers/malformed numbers become explicit failures.
pub fn parse_csv(data: &[u8]) -> Result<Vec<BTreeMap<String, String>>, String> {
    if data.contains(&0) {
        return Err("animation CSV contains NUL".into());
    }
    let mut records = Vec::new();
    for raw in data.split(|&b| b == b'\n') {
        if raw.len() >= 1024 {
            return Err("animation CSV line exceeds native buffer".into());
        }
        let right = raw
            .iter()
            .rposition(|b| (0x21..=0x7e).contains(b))
            .map_or(0, |i| i + 1);
        let raw = &raw[..right];
        if raw.is_empty() || (raw.len() > 2 && raw.starts_with(b"//")) {
            continue;
        }
        let fields = raw
            .split(|&b| b == b',')
            .map(|s| {
                if s.len() >= 128 {
                    return Err("animation CSV field exceeds native buffer".into());
                }
                let left = s
                    .iter()
                    .position(|b| (0x21..=0x7e).contains(b))
                    .unwrap_or(s.len());
                String::from_utf8(s[left..].to_vec())
                    .map_err(|_| "animation CSV is not UTF-8".to_string())
            })
            .collect::<Result<Vec<_>, String>>()?;
        if fields.len() > 64 {
            return Err("animation CSV exceeds native column limit".into());
        }
        records.push(fields);
    }
    let header = records.first().ok_or("empty animation CSV")?;
    for name in [
        "ANIM_STATE",
        "ANIM_ASSET",
        "ANIM_LOOPING",
        "RANDOM_START_TIME",
        "ANIM_BLEND_TIME",
        "ANIM_NEXT_STATE",
        "ANIM_REVERSE",
        "USE_FRAMETIME",
        "START_TIME",
        "EVENT_NAME1",
        "EVENT_TIME1",
        "EVENT_NAME2",
        "EVENT_TIME2",
        "CS_L_PROP_BONE",
        "CS_R_PROP_BONE",
        "CS_BODY_PROP_BONE",
        "TRUNCATE_TIME",
    ] {
        if !header.iter().any(|h| h == name) {
            return Err(format!("missing animation CSV column {name}"));
        }
    }
    Ok(records
        .iter()
        .skip(1)
        .map(|row| {
            header
                .iter()
                .enumerate()
                .map(|(i, key)| (key.clone(), row.get(i).cloned().unwrap_or_default()))
                .collect()
        })
        .collect())
}

/// Original player graph, with female CSV applied on a copy of the base graph.
/// The caller supplies the selected gender; no character identity is inferred.
pub struct PlayerGraph {
    pub graph: Graph,
    pub bank: anim::Bank,
    pub skeleton: Skeleton,
}
impl PlayerGraph {
    pub fn load(data_root: &Path, female: bool) -> Result<Self, String> {
        let source = data_root.join("files/data/characters/player_anims.viv");
        let member = |name: &str| format!("{}::{name}", source.display());
        let bank = anim::Bank::parse(archive::read_virtual(&member("player_anims.anm"))?.0)?;
        let skeleton = Skeleton::parse(&archive::read_virtual(&member("player_skel.ske"))?.0)?;
        let mut graph = Graph::new();
        graph.apply_rows(
            &parse_csv(&archive::read_virtual(&member("player.csv"))?.0)?,
            &bank.names,
        )?;
        if female {
            graph.apply_rows(
                &parse_csv(&archive::read_virtual(&member("player_female.csv"))?.0)?,
                &bank.names,
            )?;
        }
        Ok(Self {
            graph,
            bank,
            skeleton,
        })
    }
    /// Decode every clip reached by tetherball's graph states (56..96). Shared
    /// clips decode once. Missing/unresolved states return errors, not idle poses.
    pub fn tetherball_clips(&self) -> Result<BTreeMap<usize, anim::Clip>, String> {
        let mut clips = BTreeMap::new();
        for id in 56..=96 {
            let state = self.graph.states[id]
                .as_ref()
                .filter(|s| !s.clips.is_empty())
                .ok_or_else(|| format!("unresolved tetherball animation state {id}"))?;
            for &index in &state.clips {
                if !clips.contains_key(&index) {
                    clips.insert(index, self.bank.decode(index, &self.skeleton)?);
                }
            }
        }
        Ok(clips)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Value, json};
    fn snapshot(graph: &Graph) -> Value {
        json!(graph.states.iter().map(|state|state.as_ref().map(|s|json!({
            "asset":s.asset,"clips":s.clips,"looping":s.looping,"reverse":s.reverse,"random":s.random,"frame_time":s.frame_time,
            "blend_bits":s.blend.to_bits(),"start_bits":s.start.to_bits(),"trim_bits":s.trim.to_bits(),"next":s.next,
            "events":s.events.iter().map(|e|e.as_ref().map(|e|json!({"name":e.name,"time_bits":e.time.to_bits()}))).collect::<Vec<_>>(),"props":s.props,
        }))).collect::<Vec<_>>())
    }
    #[test]
    fn native_graph_and_original_tetherball_clips() {
        let fixture: Value =
            serde_json::from_str(include_str!("../tests/data/animation_graph_golden.json"))
                .unwrap();
        assert_eq!(fixture["elf_sha256"], crate::recovered::ELF_SHA256);
        assert_eq!(json!(*NAMES), fixture["state_names"]);
        let root = crate::bridge::data_root();
        let mut graph = Graph::new();
        let bank = PlayerGraph::load(&root, false).unwrap();
        assert_eq!(json!(bank.bank.names), fixture["bank_names"]);
        for case in fixture["cases"].as_array().unwrap() {
            let rows = if let Some(file) = case["file"].as_str() {
                let source = format!(
                    "{}::{file}",
                    root.join("files/data/characters/player_anims.viv")
                        .display()
                );
                let raw = archive::read_virtual(&source).unwrap().0;
                assert_eq!(crate::sha256::hex(&raw), case["sha256"].as_str().unwrap());
                parse_csv(&raw).unwrap()
            } else {
                serde_json::from_value(case["rows"].clone()).unwrap()
            };
            graph.apply_rows(&rows, &bank.bank.names).unwrap();
            assert_eq!(snapshot(&graph), case["states"]);
        }
        for female in [false, true] {
            let player = PlayerGraph::load(&root, female).unwrap();
            let clips = player.tetherball_clips().unwrap();
            eprintln!("female={female}: {} tetherball clips", clips.len());
            for clip in clips.values() {
                crate::character::animation_clip(clip).unwrap();
            }
        }
    }
}
