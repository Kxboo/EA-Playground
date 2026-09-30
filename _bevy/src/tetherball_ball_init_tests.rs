use super::*;
use crate::area_transform::Matrix;
use serde_json::{Value, json};

struct Host<'a> {
    input: &'a Value,
    events: Vec<Value>,
    allocation: usize,
    keys: Vec<String>,
}
fn f(v: &Value) -> f32 {
    f32::from_bits(v.as_u64().unwrap() as u32)
}
fn floats<const N: usize>(v: &Value) -> [f32; N] {
    std::array::from_fn(|i| f(&v[i]))
}
impl Host<'_> {
    fn asset(&mut self, name: &str, texture: bool) -> LoadedAsset {
        let slot = match name {
            "teatherball.gsh" => 7,
            "teatherball.o" => 5,
            "teatherball_shadow.o" => 6,
            "teatherball_rope.gsh" => 12,
            "teatherball_rope.o" => 10,
            "teatherball_rope_shadow.o" => 11,
            _ => panic!("unknown asset {name}"),
        };
        let id = self.input["asset_id_base"].as_u64().unwrap() as u32 + slot;
        let handle = 0x72000000 + slot * 0x100;
        self.events.push(if texture {
            json!(["texture", name, id, handle])
        } else {
            json!(["model", name, id, handle, 0])
        });
        LoadedAsset { handle, id }
    }
}
impl BallInitServices for Host<'_> {
    fn texture(&mut self, name: &str) -> LoadedAsset {
        self.asset(name, true)
    }
    fn model(&mut self, name: &str, flags: i32) -> LoadedAsset {
        assert_eq!(flags, 0);
        self.asset(name, false)
    }
    fn set_textures(&mut self, model: u32, texture: u32) {
        self.events.push(json!(["textures", model, texture]));
    }
    fn allocate(&mut self, bytes: u32, tag: &str) -> u32 {
        let null = self.input["null_shadows"]
            .as_array()
            .unwrap()
            .iter()
            .any(|v| v.as_u64().unwrap() as usize == self.allocation);
        let handle = if null {
            0
        } else {
            0x73000000 + self.allocation as u32 * 0x100
        };
        self.allocation += 1;
        self.events.push(json!([
            "allocate",
            bytes,
            self.input["pool"],
            0,
            tag,
            handle
        ]));
        handle
    }
    fn construct_cached_model(&mut self, handle: u32) {
        self.events.push(json!(["cached_ctor", handle]));
    }
    fn bind_cached_model(&mut self, cached: u32, model: u32) {
        self.events.push(json!(["scale", cached, model]));
    }
    fn construct_shadow(&mut self, handle: u32, model: u32) {
        self.events.push(json!(["shadow_ctor", handle, model]));
    }
    fn add_scene_entity(&mut self, layer: i32, entity: u32) {
        self.events.push(json!(["add_entity", layer, entity]));
    }
    fn database_key(&mut self, name: &str) -> u64 {
        self.events.push(json!(["key", name]));
        self.keys.push(name.into());
        (self.keys.len() - 1) as u64
    }
    fn collection(&mut self, class: u64, name: u64) -> u32 {
        self.events.push(json!([
            "collection",
            self.keys[class as usize],
            self.keys[name as usize]
        ]));
        0x71012000
    }
    fn float_from_array(&mut self, collection: u32, field: &str, index: u32) -> f32 {
        assert_eq!(collection, 0x71012000);
        self.events.push(json!(["float", field, index]));
        let slot = [
            "ball_basehitspeed",
            "ball_acceleratemodifier",
            "ball_powermodifier",
            "ball_megamodifier",
        ]
        .iter()
        .position(|x| *x == field)
        .unwrap();
        f(&self.input["tuning"][slot])
    }
    fn destroy_collection(&mut self, collection: u32) {
        assert_eq!(collection, 0x71012000);
        self.events.push(json!(["destroy_collection"]));
    }
}

#[test]
fn original_ball_initialization() {
    let fixture: Value = serde_json::from_str(include_str!(
        "../tests/data/tetherball_ball_init_golden.json"
    ))
    .unwrap();
    assert_eq!(fixture["elf_sha256"], crate::recovered::ELF_SHA256);
    for (i, row) in fixture["cases"].as_array().unwrap().iter().enumerate() {
        let c = &row["input"];
        let mut motion = crate::tetherball::tests::ball(&c["initial"]);
        let matrix: Matrix = std::array::from_fn(|j| 17. + j as f32);
        let mut scene = BallScene {
            anchor: [17., 18., 19.],
            position: [17., 18., 19.],
            ball_matrix: matrix,
            rope_matrix: matrix,
            trails: [100, 101, 102],
            null_trail: u32::MAX,
            ball_shadow: false,
            rope_shadow: false,
        };
        let mut host = Host {
            input: c,
            events: vec![],
            allocation: 0,
            keys: vec![],
        };
        let resources = initialize_ball(
            &mut motion,
            &mut scene,
            BallInitInput {
                difficulty: c["difficulty"].as_u64().unwrap() as u32,
                anchor: floats(&c["anchor"]),
                heading: f(&c["heading"]),
                pole_height: f(&c["pole_height"]),
            },
            &mut host,
        );
        crate::tetherball::tests::assert_bits(
            &motion,
            &crate::tetherball::tests::ball(&row["motion"]),
            i,
            0,
        );
        assert_eq!(json!(host.events), row["events"], "events {i}");
        assert_eq!(json!(scene.anchor.map(f32::to_bits)), row["anchor"]);
        assert_eq!(json!(scene.position.map(f32::to_bits)), row["position"]);
        assert_eq!(
            json!(scene.ball_matrix.map(f32::to_bits)),
            row["ball_matrix"]
        );
        assert_eq!(
            json!(scene.rope_matrix.map(f32::to_bits)),
            row["rope_matrix"]
        );
        assert_eq!(json!(scene.trails), row["trails"]);
        assert_eq!(scene.null_trail, u32::MAX);
        assert_eq!(scene.ball_shadow, resources.shadows[0] != 0);
        assert_eq!(scene.rope_shadow, resources.shadows[1] != 0);
        let [b, bs, t, r, rs, rt] = resources.asset_ids;
        assert_eq!(
            json!([
                resources.shadows[0],
                resources.shadows[1],
                resources.cached[0],
                resources.cached[1],
                b,
                bs,
                t,
                resources.cached[2],
                resources.cached[3],
                r,
                rs,
                rt
            ]),
            row["resources"]
        );
        assert_eq!(
            json!(resources.accelerate_modifier_158.to_bits()),
            row["accelerate_modifier"]
        );
        assert_eq!(json!(resources.flag_15c), row["flag_15c"]);
    }
}
