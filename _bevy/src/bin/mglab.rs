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
        Some("probe") => {
            let ty = args.get(2).and_then(|v| v.parse().ok()).unwrap_or(3);
            mgvm::probe(ty);
        }
        _ => eprintln!("usage: mglab probe [type]"),
    }
}
