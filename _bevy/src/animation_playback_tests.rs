use super::*;
use serde_json::{Value, json};
fn f(v: &Value) -> f32 {
    f32::from_bits(v.as_u64().unwrap() as u32)
}
fn seed(v: &Value) -> Playback {
    Playback {
        function: v["function"].as_u64().unwrap() as u32,
        pose_valid: v["pose_valid"].as_bool().unwrap(),
        time: f(&v["time"]),
        duration: f(&v["duration"]),
        speed: f(&v["speed"]),
        current: v["current"].as_u64().unwrap() as usize,
        start: f(&v["start"]),
        trim: f(&v["trim"]),
        blend_remaining: f(&v["blend_remaining"]),
        blend_total: f(&v["blend_total"]),
        blending: v["blending"].as_bool().unwrap(),
        skip_advance: v["skip_advance"].as_bool().unwrap(),
        auto_after: f(&v["auto_after"]),
        pose_words: v["pose_words"].as_u64().unwrap() as u32,
        marker_count: v["marker_count"].as_i64().unwrap() as i32,
        handler_count: v["handler_count"].as_u64().unwrap() as u32 as i32,
    }
}
fn snapshot(p: &Playback) -> Value {
    json!({"function":p.function,"pose_valid":p.pose_valid,"time":p.time.to_bits(),"duration":p.duration.to_bits(),
        "speed":p.speed.to_bits(),"current":p.current,"start":p.start.to_bits(),"trim":p.trim.to_bits(),
        "blend_remaining":p.blend_remaining.to_bits(),"blend_total":p.blend_total.to_bits(),"blending":p.blending,
        "skip_advance":p.skip_advance,"auto_after":p.auto_after.to_bits(),"pose_words":p.pose_words,
        "marker_count":p.marker_count as u32,"handler_count":p.handler_count as u32})
}
fn graph(v: &Value) -> Graph {
    let mut g = Graph::new();
    for (id, s) in v.as_object().unwrap() {
        if s.is_null() {
            continue;
        }
        g.states[id.parse::<usize>().unwrap()] = Some(StateInfo {
            asset: s["asset"].as_str().unwrap().into(),
            clips: serde_json::from_value(s["clips"].clone()).unwrap(),
            looping: s["looping"].as_bool().unwrap(),
            reverse: s["reverse"].as_bool().unwrap(),
            random: s["random"].as_bool().unwrap(),
            frame_time: s["frame_time"].as_bool().unwrap(),
            blend: f(&s["blend_bits"]),
            start: f(&s["start_bits"]),
            trim: f(&s["trim_bits"]),
            next: s["next"].as_u64().unwrap() as usize,
            props: serde_json::from_value(s["props"].clone()).unwrap(),
            events: std::array::from_fn(|i| {
                if s["events"][i].is_null() {
                    None
                } else {
                    Some(Event {
                        name: s["events"][i]["name"].as_str().unwrap().into(),
                        time: f(&s["events"][i]["time_bits"]),
                    })
                }
            }),
        });
    }
    g
}
struct Host<'a> {
    input: &'a Value,
    effects: Vec<Value>,
    mode: bool,
}
impl Services for Host<'_> {
    fn random_index(&mut self, high: usize) -> usize {
        self.effects.push(json!(["random_index", high]));
        self.input["random_index"].as_u64().unwrap() as usize
    }
    fn random_time(&mut self, high: f32) -> f32 {
        self.effects.push(json!(["random_time", high.to_bits()]));
        f(&self.input["random_time_bits"])
    }
    fn clip(&mut self, index: usize) -> u32 {
        self.effects.push(json!(["clip", index]));
        0x74600000 + index as u32 * 4
    }
    fn allocate(&mut self, clip: u32) -> u32 {
        self.effects.push(json!(["allocate", clip]));
        0x74200000
    }
    fn release(&mut self, function: u32) {
        self.effects.push(json!(["release", function]));
    }
    fn use_fps(&mut self, function: u32, enabled: bool) {
        self.mode = enabled;
        self.effects.push(json!(["use_fps", function, enabled]));
    }
    fn function_length(&mut self, function: u32) -> f32 {
        self.effects
            .push(json!(["function_length", function, self.mode]));
        self.input[if self.mode { "seconds" } else { "samples" }]
            .as_f64()
            .unwrap() as f32
    }
    fn pose(&mut self, _: &Playback, effect: PoseEffect) {
        self.effects.push(match effect {
            PoseEffect::Copy { bytes } => json!(["copy", bytes]),
            PoseEffect::Still { buffer, masked } => json!(["still", buffer, masked]),
            PoseEffect::Evaluate {
                function,
                time,
                buffer,
                masked,
            } => json!(["evaluate", function, time.to_bits(), buffer, masked]),
            PoseEffect::Blend { weight, masked } => json!(["blend", weight.to_bits(), masked]),
            PoseEffect::Procedural => json!(["procedural"]),
            PoseEffect::Skin { masked } => json!(["skin", masked]),
            PoseEffect::Marker(index) => json!(["marker", index]),
        });
    }
    fn has_procedural(&mut self) -> bool {
        self.input["procedural"].as_bool().unwrap()
    }
    fn event(&mut self, state: &mut Playback, handler: i32, event: &Event) {
        self.effects
            .push(json!(["event", handler, event.name, event.time.to_bits()]));
        if self.input["shrink_handlers"] == true {
            state.handler_count = 1;
        }
    }
}
#[test]
fn original_animation_state_calls() {
    let fixture: Value =
        serde_json::from_str(include_str!("../tests/data/animation_playback_golden.json")).unwrap();
    assert_eq!(fixture["elf_sha256"], crate::recovered::ELF_SHA256);
    for (i, c) in fixture["cases"].as_array().unwrap().iter().enumerate() {
        let mut p = seed(&c["initial"]);
        let g = graph(&c["graph"]);
        let a = &c["input"];
        let mut host = Host {
            input: a,
            effects: vec![],
            mode: false,
        };
        match c["operation"].as_str().unwrap() {
            "select" => p
                .select(&g, a["target"].as_u64().unwrap() as usize, &mut host)
                .unwrap(),
            "set_next" => assert_eq!(
                p.set_next(
                    &g,
                    a["target"].as_u64().unwrap() as usize,
                    f(&a["speed_bits"]),
                    a["force"].as_bool().unwrap(),
                    a["after_ms"].as_i64().unwrap() as i32,
                    &mut host
                )
                .unwrap(),
                c["result"] == 1,
                "case {i} result"
            ),
            "update" => p
                .update(
                    &g,
                    f(&a["delta_bits"]),
                    a["request_pose"].as_bool().unwrap(),
                    a["use_mask"].as_bool().unwrap(),
                    &mut host,
                )
                .unwrap(),
            "events" => p.events(&g, f(&a["delta_bits"]), &mut host).unwrap(),
            "time" => {
                p.set_state_time(&g, f(&a["value_bits"])).unwrap();
                assert_eq!(
                    p.state_time(&g).unwrap().to_bits(),
                    c["result_bits"].as_u64().unwrap() as u32,
                    "case {i} time"
                );
            }
            _ => panic!("unknown fixture operation"),
        }
        assert_eq!(
            snapshot(&p),
            c["expected"],
            "case {i} {} state",
            c["operation"]
        );
        assert_eq!(
            json!(host.effects),
            c["effects"],
            "case {i} {} effects",
            c["operation"]
        );
    }
}
