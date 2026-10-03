//! Rust ports of original functions, replacing the emulated PowerPC one function at a time.
//!
//! Each port is a `HookFn` bound to the function's mangled symbol: it reads the arguments from the guest registers, works
//! on the guest objects in emulated memory exactly like the original, calls other original functions through the VM where
//! the original does, and returns the same way.  `EAGL_PORTS` selects how ports are used:
//!   unset / `off`  ports are not installed (the original code runs)
//!   `on`           ports replace the original functions
//!   `verify`       the original and the port both run on the same state and are compared
pub mod attrib;
pub mod character;
pub mod freethrow;
pub mod microbug;
pub mod rmath;

use super::MgHost;
use crate::gekko::verify::Ret;
use crate::gekko::{HookFn, Vm};

pub type Port = (&'static str, HookFn<MgHost>);

/// Every port: (mangled symbol, Rust function).
pub fn all() -> Vec<Port> {
    let mut v = vec![];
    v.extend_from_slice(microbug::PORTS_BUG);
    v.extend_from_slice(microbug::PORTS_HUNT);
    v.extend_from_slice(rmath::PORTS);
    v.extend_from_slice(attrib::PORTS);
    v.extend_from_slice(character::PORTS);
    v.extend_from_slice(freethrow::PORTS_GAME);
    v.extend_from_slice(freethrow::PORTS_DRIBBLE);
    v
}

/// `EAGL_PORTS=on`: bind the ports as hooks (after the engine hooks, so a port wins over an engine hook).
/// `EAGL_PORTS=verify`: run each port side by side with its original and compare (`gekko::verify`).
pub fn install(vm: &mut Vm<MgHost>) {
    let mode = std::env::var("EAGL_PORTS").unwrap_or_default();
    // EAGL_PORTS_ONLY=sym,sym,..: only these ports (verifies ports that otherwise only run inside other ports)
    let only: Option<Vec<String>> = std::env::var("EAGL_PORTS_ONLY").ok().map(|v| v.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect());
    if mode == "verify" {
        vm.ver.params = Some(ghidra_params);
    }
    for (sym, f) in all() {
        if only.as_ref().is_some_and(|o| !o.iter().any(|x| x == sym)) {
            continue;
        }
        let ok = match mode.as_str() {
            "on" => vm.hook(sym, f),
            "verify" => {
                let ret = vm.img.addr(sym).map(return_kind).unwrap_or(Ret::Int);
                vm.verify_port(sym, f, ret)
            }
            _ => true,
        };
        if !ok {
            vm.log_missing_symbol(sym);
        }
    }
}

/// The Ghidra signature of the function at `addr` (from its first declaration line up to the closing parenthesis), from
/// the local code pack.
fn ghidra_signature(addr: u32) -> Option<String> {
    let path = crate::bridge::root().join("..").join("GameMap").join("site").join("code").join(format!("{:05x}.json", addr >> 14));
    let text = std::fs::read_to_string(path).ok()?;
    let k = text.find(&format!("\"{addr:08x}\""))?;
    let g = text[k..].find("\"ghidra\":\"")?;
    let body = text[k + g + 10..].replace("\\n", "\n");
    let mut sig = String::new();
    for l in body.lines().map(str::trim).skip_while(|l| l.is_empty() || l.starts_with("//") || l.starts_with("/*")) {
        sig.push_str(l);
        sig.push(' ');
        if l.contains(')') {
            break;
        }
    }
    sig.contains('(').then_some(sig)
}

/// Integer and float argument registers of the function at `addr` from its Ghidra parameter list.
pub fn ghidra_params(addr: u32) -> Option<(usize, usize)> {
    let sig = ghidra_signature(addr)?;
    let open = sig.find('(')?;
    let close = sig.rfind(')')?;
    let list = sig[open + 1..close].trim();
    if list.is_empty() || list == "void" {
        return Some((0, 0));
    }
    let (mut ints, mut floats) = (0, 0);
    for p in list.split(',') {
        let p = p.trim();
        let ty = p.split_whitespace().next().unwrap_or("");
        if p.contains('*') {
            ints += 1;
        } else if ty == "float" || ty == "double" {
            floats += 1;
        } else if ty == "longlong" || ty == "ulonglong" || ty == "undefined8" {
            ints += 2;
        } else {
            ints += 1;
        }
    }
    Some((ints.min(8), floats.min(4)))
}

/// The return type of the original function at `addr`, from the Ghidra signature in the local code pack
/// (`GameMap/site/code`, built by `GameMap/tools/build_code_pack.py`); integer when unknown.
pub fn return_kind(addr: u32) -> Ret {
    let Some(sig) = ghidra_signature(addr) else { return Ret::Int };
    let head = sig.split('(').next().unwrap_or("");
    let ty = head.split_whitespace().next().unwrap_or("");
    match ty {
        "void" if !head.contains('*') => Ret::Void,
        "float" | "double" | "longdouble" if !head.contains('*') => Ret::Float,
        "longlong" | "ulonglong" | "undefined8" | "int64_t" | "uint64_t" if !head.contains('*') => Ret::Int64,
        _ => Ret::Int,
    }
}

/// The verification results (`EAGL_PORTS=verify`), written to `EAGL_PORT_REPORT` (default `port_verification.tsv`).
pub fn write_report(vm: &Vm<MgHost>) {
    if std::env::var("EAGL_PORTS").as_deref() != Ok("verify") {
        return;
    }
    let path = std::env::var("EAGL_PORT_REPORT").unwrap_or_else(|_| "port_verification.tsv".into());
    let text = vm.verify_report();
    match std::fs::write(&path, &text) {
        Ok(()) => println!("port verification -> {path}\n{text}"),
        Err(e) => println!("port verification: cannot write {path}: {e}"),
    }
}
