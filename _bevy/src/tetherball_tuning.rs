//! Native tetherball tuning selection and inherited corpus reads. See TETHERBALL_TUNING.md.
use crate::vlt::{Collection, Database};
use serde_json::Value;

pub const SINGLE_FIELDS: [&str; 6] = [
    "game_play_type",
    "single_player_num_rounds",
    "distance_from_pole",
    "handicap",
    "game_duration",
    "single_player_rotations_to_win",
];
pub const SPEED_FIELDS: [&str; 4] = [
    "ball_basehitspeed",
    "ball_powermodifier",
    "ball_megamodifier",
    "ball_acceleratemodifier",
];
pub const ANGLE_FIELDS: [&str; 4] = [
    "hit_returnanglepredelta",
    "hit_returnanglepostdelta",
    "hit_accelanglepredelta",
    "hit_accelanglepostdelta",
];
pub const AI_FIELDS: [&str; 7] = [
    "ai_toofastchance",
    "ai_tooslowchance",
    "ai_wrongheightchance",
    "ai_powerhitchance",
    "ai_megahitchance",
    "ai_powermessupfactor",
    "ai_megamessupfactor",
];

/// `GetTunablesCollectionName` at 0x8039ce38; session mode (+0x40) is compared as signed.
pub fn collection_name(session_mode: i32, dare: i32) -> Option<&'static str> {
    if session_mode > 1 || dare == -1 {
        return Some("tunables");
    }
    match dare {
        0..=2 => Some("dares_speed_rounds"),
        3..=5 => Some("dares_time"),
        6..=8 => Some("dares_endurance"),
        _ => None,
    }
}
/// Minigame's array-index conversion at 0x803abea8.
pub fn difficulty_index(difficulty: i32) -> usize {
    match difficulty {
        0..=3 => difficulty as usize,
        _ => 0,
    }
}

/// Resolve the first attribute present on the collection or its parent chain.
/// Array bounds are evaluated *after* inheritance; a short child array does not merge its parent.
pub fn inherited_attribute(
    db: &Database,
    collection: &Collection,
    name: &str,
) -> Result<Option<Value>, String> {
    let mut c = collection;
    let mut seen = Vec::new();
    loop {
        if seen.contains(&c.key) {
            return Err(format!("collection inheritance cycle at {:#x}", c.key));
        }
        seen.push(c.key);
        if let Some(a) = c
            .attributes
            .iter()
            .find(|a| a.name_key == crate::vlt::string_hash64(name))
        {
            return db.value(a).map(Some);
        }
        if c.parent_key == 0 {
            return Ok(None);
        }
        c = db
            .collections
            .iter()
            .find(|p| p.key == c.parent_key && p.class_key == c.class_key)
            .ok_or_else(|| format!("missing parent {:#x}", c.parent_key))?;
    }
}
fn element(db: &Database, c: &Collection, name: &str, index: usize) -> Result<Value, String> {
    let Some(v) = inherited_attribute(db, c, name)? else {
        return Ok(Value::Null);
    };
    Ok(match v {
        Value::Array(a) => a.get(index).cloned().unwrap_or(Value::Null),
        v if index == 0 => v,
        _ => Value::Null,
    })
}
fn int(db: &Database, c: &Collection, name: &str, index: usize) -> Result<u32, String> {
    let v = element(db, c, name, index)?;
    if v.is_null() {
        return Ok(0);
    }
    v.as_i64()
        .map(|x| x as u32)
        .or_else(|| v.as_u64().map(|x| x as u32))
        .ok_or_else(|| format!("{name} is not an integer"))
}
fn float(db: &Database, c: &Collection, name: &str, index: usize) -> Result<f32, String> {
    let v = element(db, c, name, index)?;
    if v.is_null() {
        return Ok(0.0);
    }
    v.as_f64()
        .map(|x| x as f32)
        .ok_or_else(|| format!("{name} is not a float"))
}

/// Native unsigned-byte array read, with the existing inheritance and absent
/// attribute/out-of-range semantics shared by all decoded tuning callers.
pub fn ai_byte_at(
    db: &Database,
    collection: &Collection,
    field: &str,
    index: u32,
) -> Result<u8, String> {
    Ok(int(db, collection, field, index as usize)? as u8)
}
/// AI Initialize uses the raw unsigned difficulty index, unlike the minigame
/// initializer's clamped difficulty selector.
pub fn ai_difficulty(
    db: &Database,
    session_mode: i32,
    dare: i32,
    index: u32,
) -> Result<[u8; 7], String> {
    let name = collection_name(session_mode, dare).ok_or("native null tuning collection")?;
    let collection = db
        .find_collection("mg_tetherball", name)
        .ok_or("missing AI tuning collection")?;
    let mut values = [0; 7];
    for (i, field) in AI_FIELDS.iter().enumerate() {
        values[i] = ai_byte_at(db, collection, field, index)?;
    }
    Ok(values)
}

/// HitCompulsion::Activate selects the dare collection for these arrays;
/// MGTetherball's separate initialization uses the regular collection instead.
pub fn ai_hit_angles(db: &Database, session_mode: i32, dare: i32) -> Result<[[f32; 3]; 4], String> {
    let name = collection_name(session_mode, dare).ok_or("native null tuning collection")?;
    let collection = db
        .find_collection("mg_tetherball", name)
        .ok_or("missing AI tuning collection")?;
    let mut values = [[0.; 3]; 4];
    for (i, field) in ANGLE_FIELDS.iter().enumerate() {
        for index in 0..3 {
            values[i][index] =
                (int(db, collection, field, index)? as i16 as f32) * f32::from_bits(0x3c8efa35);
        }
    }
    Ok(values)
}

#[derive(Debug, Clone, PartialEq)]
pub struct TetherballTuning {
    pub collection: &'static str,
    /// MGTetherball offsets +16c,+170,+174,+178,+17c,+180.
    pub single_player: [u32; 6],
    /// Always regular `tunables`: base, power, mega, acceleration; shared with Tetherball Initialize.
    pub speeds: [f32; 4],
    /// Regular collection, each indexed by hit zone 0..2, converted from signed Int16 degrees.
    pub angles: [[f32; 3]; 4],
    /// Selected collection: fast, slow, wrong-height, power-hit, mega-hit, power-error, mega-error.
    pub ai: [u8; 7],
}
impl TetherballTuning {
    /// Validated single-player tuning reads; multiplayer lifecycle overwrites its game fields separately.
    pub fn load(
        db: &Database,
        session_mode: i32,
        dare: i32,
        difficulty: i32,
    ) -> Result<Self, String> {
        let name = collection_name(session_mode, dare)
            .ok_or_else(|| format!("native selector returns null for dare {dare}"))?;
        let selected = db
            .find_collection("mg_tetherball", name)
            .ok_or_else(|| format!("missing mg_tetherball/{name}"))?;
        let regular = db
            .find_collection("mg_tetherball", "tunables")
            .ok_or("missing mg_tetherball/tunables")?;
        let index = difficulty_index(difficulty);
        let mut result = Self {
            collection: name,
            single_player: [0; 6],
            speeds: [0.0; 4],
            angles: [[0.0; 3]; 4],
            ai: [0; 7],
        };
        for (i, n) in SINGLE_FIELDS.iter().enumerate() {
            result.single_player[i] = int(db, selected, n, if i == 0 { 0 } else { index })?
        }
        for (i, n) in SPEED_FIELDS.iter().enumerate() {
            result.speeds[i] = float(db, regular, n, index)?
        }
        for (i, n) in ANGLE_FIELDS.iter().enumerate() {
            for zone in 0..3 {
                result.angles[i][zone] =
                    (int(db, regular, n, zone)? as i16 as f32) * f32::from_bits(0x3c8efa35)
            }
        }
        for (i, n) in AI_FIELDS.iter().enumerate() {
            result.ai[i] = int(db, selected, n, index)? as u8
        }
        Ok(result)
    }
}
/// InitTunablesForMultiPlayer copies parameter offsets 0,4,8,0xc,0x10 (0xc is unused).
/// Returns object +3c,+44,+16c,+170,+174,+178,+17c,+180.
pub fn multiplayer_fields(parameters: [u32; 5]) -> [u32; 8] {
    [
        parameters[0],
        parameters[1],
        0,
        parameters[4],
        0,
        0,
        0,
        parameters[2],
    ]
}
/// Ball Initialize starts in zone 0; these are the executable's three zone height offsets.
pub fn ball_height(pole_height: f32, zone: usize) -> Option<f32> {
    [0.5, f32::from_bits(0x3f666666), 0.0]
        .get(zone)
        .map(|x| pole_height + *x)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn original_tuning_fixture() {
        let f: Value =
            serde_json::from_str(include_str!("../tests/data/tetherball_tuning_golden.json"))
                .unwrap();
        assert_eq!(
            f["elf_sha256"],
            "5cef3efc7005fb71fed0a75e60ee240ee6ac4243b00dd3296e6e53ca269a3e2c"
        );
        for c in f["selectors"].as_array().unwrap() {
            assert_eq!(
                collection_name(
                    c["session_mode"].as_i64().unwrap() as i32,
                    c["dare"].as_i64().unwrap() as i32
                ),
                c["collection"].as_str()
            )
        }
        for c in f["indices"].as_array().unwrap() {
            assert_eq!(
                difficulty_index(c["difficulty"].as_i64().unwrap() as i32),
                c["index"].as_u64().unwrap() as usize
            )
        }
        for c in f["multiplayer"].as_array().unwrap() {
            let p = std::array::from_fn(|i| c["parameters"][i].as_u64().unwrap() as u32);
            assert_eq!(serde_json::json!(multiplayer_fields(p)), c["fields"])
        }
        for c in f["heights"].as_array().unwrap() {
            assert_eq!(
                ball_height(
                    f32::from_bits(c["pole_height_bits"].as_u64().unwrap() as u32),
                    c["zone"].as_u64().unwrap() as usize
                )
                .unwrap()
                .to_bits(),
                c["height_bits"].as_u64().unwrap() as u32
            )
        }
        let dir = crate::bridge::data_root().join("files/data/db");
        let v = std::fs::read(dir.join("db.vlt")).expect("original tuning corpus required");
        let b = std::fs::read(dir.join("db.bin")).unwrap();
        let db = Database::load(&v, &b, crate::vlt::known_names()).unwrap();
        assert!(TetherballTuning::load(&db, 1, 9, 0).is_err());
        for c in f["records"].as_array().unwrap() {
            let t = TetherballTuning::load(
                &db,
                1,
                c["dare"].as_i64().unwrap() as i32,
                c["difficulty"].as_i64().unwrap() as i32,
            )
            .unwrap();
            assert_eq!(serde_json::json!(t.single_player), c["single"]);
            assert_eq!(
                serde_json::json!(t.speeds.map(f32::to_bits)),
                c["speed_bits"]
            );
            assert_eq!(
                serde_json::json!(t.speeds.map(f32::to_bits)),
                c["ball_speed_bits"]
            );
            assert_eq!(
                serde_json::json!(t.angles.map(|a| a.map(f32::to_bits))),
                c["angle_bits"]
            );
            assert_eq!(serde_json::json!(t.ai), c["ai"]);
        }
    }
}
