//! Fast-iteration harness for the minigame VM (`gekko` + `mgvm`) without the renderer:
//! `cargo run --profile lab --bin mglab -- probe [minigame type]`.
#![allow(dead_code)]
#[path = "../gekko/mod.rs"]
mod gekko;
#[path = "../mgvm/mod.rs"]
mod mgvm;
#[path = "../vlt.rs"]
mod vlt;
#[path = "../archive.rs"]
mod archive;
#[path = "../havok.rs"]
mod havok;
mod bridge {
    use std::path::{Path, PathBuf};
    pub fn root() -> PathBuf {
        if let Ok(p) = std::env::var("EAGL_WORKSPACE") {
            return PathBuf::from(p);
        }
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
    }
    pub fn data_root() -> PathBuf {
        if let Ok(p) = std::env::var("EAGL_DATA") {
            return PathBuf::from(p);
        }
        let rel = Path::new("eagl EA PLAYGROUND").join("extra").join("more").join("eaplayground files").join("DATA");
        for p in root().ancestors() {
            let c = p.join(&rel);
            if c.exists() {
                return c;
            }
        }
        PathBuf::from(r"D:\_eagl\eagl EA PLAYGROUND\extra\more\eaplayground files\DATA")
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    match args.get(1).map(String::as_str) {
        Some("classify") => {
            let out = args.get(2).cloned().unwrap_or_else(|| "classify.tsv".into());
            if let Err(e) = mgvm::classify(&out) {
                eprintln!("classify failed: {e}");
            }
        }
        Some("probe") => {
            let ty = args.get(2).and_then(|v| v.parse().ok()).unwrap_or(3);
            mgvm::probe(ty);
        }
        Some("hkb") => {
            let name = args.get(2).cloned().unwrap_or_default();
            let path = bridge::data_root().join("files/data/physics").join(&name);
            let bytes = std::fs::read(&path).expect("read hkx");
            let ct = havok::ClassTable::embedded();
            let pf = havok::Packfile::parse(&bytes, &ct).expect("parse");
            let mut counts = std::collections::BTreeMap::new();
            for (_, c) in &pf.virt {
                *counts.entry(c.clone()).or_insert(0) += 1;
            }
            println!("{counts:?}");
            for b in havok::rigid_bodies(&pf) {
                println!("{} massinv {} fric {} rest {} pos {:?} filter {:#x}", b.name, b.mass_inv, b.friction, b.restitution, b.translation, b.filter);
                for p in &b.prims {
                    match p {
                        havok::Prim::Hull(v) => {
                            let (mut lo, mut hi) = ([f32::MAX; 3], [f32::MIN; 3]);
                            for q in v {
                                for i in 0..3 {
                                    lo[i] = lo[i].min(q[i]);
                                    hi[i] = hi[i].max(q[i]);
                                }
                            }
                            println!("   hull {} pts {:?}..{:?}", v.len(), lo, hi);
                        }
                        havok::Prim::Mesh(t) => println!("   mesh {} tris", t.len()),
                        o => println!("   {o:?}"),
                    }
                }
            }
        }
        Some("hk") => {
            let name = args.get(2).cloned().unwrap_or_default();
            let path = bridge::data_root().join("files/data/physics").join(&name);
            let bytes = std::fs::read(&path).expect("read hkx");
            let ct = havok::ClassTable::embedded();
            let pf = havok::Packfile::parse(&bytes, &ct).expect("parse");
            let mut counts = std::collections::BTreeMap::new();
            for (_, c) in &pf.virt {
                *counts.entry(c.clone()).or_insert(0) += 1;
            }
            println!("{counts:?}");
            for (&(si, off), cn) in &pf.virt {
                if cn == "hkRigidBody" {
                    let o = pf.decode_object(si, off, cn, 0);
                    println!("{}", serde_json::to_string_pretty(&o).unwrap().chars().take(6000).collect::<String>());
                    let r = o["collidable"]["shape"]["$ref"].as_array().unwrap();
                    let key = (r[0].as_u64().unwrap() as usize, r[1].as_i64().unwrap() as i32);
                    let sc = pf.class_of(key).cloned().unwrap();
                    println!("SHAPE {sc}: {}", pf.decode_object(key.0, key.1, &sc, 0));
                    break;
                }
            }
        }
        _ => eprintln!("usage: mglab probe [type] | hk file.hkx"),
    }
}
