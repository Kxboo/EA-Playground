use super::*;
use serde_json::{Value, json};

const SHADOW_MANAGER: u32 = 0x7100_1000;

fn u32v(value: &Value) -> u32 {
    value.as_u64().unwrap() as u32
}
fn vec3(value: &Value) -> [f32; 3] {
    [
        f32::from_bits(u32v(&value[0])),
        f32::from_bits(u32v(&value[1])),
        f32::from_bits(u32v(&value[2])),
    ]
}

#[derive(Default)]
struct Recorder {
    events: Vec<Value>,
}

impl ShadowSetupServices for Recorder {
    fn set_shadow_viewport(&mut self, mode: i32, options: ShadowViewOptions) {
        self.events.push(json!([
            "set_viewport",
            SHADOW_MANAGER,
            mode,
            options.native_words(),
        ]));
    }
}

#[test]
fn matches_original_setup_shadow_options_cases() {
    let fixture: Value = serde_json::from_str(include_str!(
        "../tests/data/tetherball_shadow_setup_golden.json"
    ))
    .unwrap();

    for (index, case) in fixture["cases"].as_array().unwrap().iter().enumerate() {
        let mut reset = ResetState::blank();
        reset.world_position_110 = vec3(&case["position"]);
        let mut services = Recorder::default();
        setup_shadow_options(&reset, &mut services);
        assert_eq!(
            json!(services.events),
            case["events"],
            "viewport request case {index}"
        );
    }
}
