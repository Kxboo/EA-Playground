//! Port verification: run an original function and its Rust port on the same guest state and compare what they do.
//!
//! When a function with a registered port is entered (`Vm::verify_port`):
//! 1. the ORIGINAL runs in the interpreter with a write journal on; every engine (hook) call it makes is recorded
//!    (arguments, results, the memory it wrote);
//! 2. its writes are undone and the CPU state restored;
//! 3. the PORT runs on the same state; its engine calls are not executed again but answered from the record
//!    (same function, same arguments in the same order, or it is a mismatch), and the memory each recorded call wrote is
//!    written again;
//! 4. compared: return value (by the function's return type), every byte written outside the function's own stack frame,
//!    and the engine calls (all of them, in order);
//! 5. the original's outcome is kept (the port's writes are undone, the original's re-applied), so a long scenario keeps
//!    following the original game and every later call is compared on genuine state.
use super::cpu::{Cpu, SENTINEL};
use super::Vm;
use std::collections::HashMap;

/// What a function returns (decides which register is compared).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Ret {
    Void,
    Int,
    Int64,
    Float,
}

#[derive(Clone)]
struct HookRec {
    idx: usize,
    r: [u32; 8],
    f: [f64; 4],
    ret_r3: u32,
    ret_r4: u32,
    ret_f1: f64,
    j0: usize,
    j1: usize,
    /// index of the first record after this call (nested calls made by the hook are skipped when it is replayed)
    end: usize,
}

struct Replay {
    recs: Vec<HookRec>,
    writes: Vec<(u32, Vec<u8>, Vec<u8>)>,
    cursor: usize,
    error: Option<String>,
}

#[derive(Default)]
pub struct Stats {
    pub calls: u64,
    pub mismatches: u64,
    pub first: Option<String>,
}

#[derive(Default)]
pub struct State {
    /// > 0 while a verification runs (nested port entries then run as original code)
    pub depth: u32,
    recording: Option<Vec<HookRec>>,
    replay: Option<Replay>,
    /// results by function address
    pub stats: HashMap<u32, Stats>,
    /// Argument registers (int, float) of an engine function by address, when the host knows them better than the
    /// mangled name (static methods look like methods there).  Cached per hook.
    pub params: Option<fn(u32) -> Option<(usize, usize)>>,
    param_cache: HashMap<usize, (usize, usize)>,
}

impl State {
    pub fn replaying(&self) -> bool {
        self.replay.is_some()
    }
    /// Start recording an engine call (only while the original runs).  Returns the record index.
    pub fn record_start(&mut self, idx: usize, cpu: &Cpu, j: usize) -> Option<usize> {
        let recs = self.recording.as_mut()?;
        let mut r = [0u32; 8];
        r.copy_from_slice(&cpu.r[3..11]);
        let mut f = [0f64; 4];
        f.copy_from_slice(&cpu.f[1..5]);
        recs.push(HookRec { idx, r, f, ret_r3: 0, ret_r4: 0, ret_f1: 0., j0: j, j1: j, end: 0 });
        Some(recs.len() - 1)
    }
    pub fn record_end(&mut self, k: usize, cpu: &Cpu, j: usize) {
        if let Some(recs) = self.recording.as_mut() {
            let n = recs.len();
            let rec = &mut recs[k];
            rec.ret_r3 = cpu.r[3];
            rec.ret_r4 = cpu.r[4];
            rec.ret_f1 = cpu.f[1];
            rec.j1 = j;
            rec.end = n;
        }
    }
}

/// Integer and float argument registers used by a CodeWarrior-mangled function (`this` included for methods).
/// Best effort: unknown encodings count as one integer register.
pub fn param_regs(sym: &str) -> (usize, usize) {
    let Some(k) = sym.find("__") else { return (8, 4) };
    let rest = &sym[k + 2..];
    let b = rest.as_bytes();
    let mut i = 0;
    let mut ints = 0;
    // class qualifier: <len><name> or Q<n>...
    let qualified = |i: &mut usize| -> bool {
        if *i < b.len() && b[*i] == b'Q' && *i + 1 < b.len() && b[*i + 1].is_ascii_digit() {
            let n = (b[*i + 1] - b'0') as usize;
            *i += 2;
            for _ in 0..n {
                let s = *i;
                while *i < b.len() && b[*i].is_ascii_digit() {
                    *i += 1;
                }
                let len: usize = rest[s..*i].parse().unwrap_or(0);
                *i += len;
            }
            true
        } else if *i < b.len() && b[*i].is_ascii_digit() {
            let s = *i;
            while *i < b.len() && b[*i].is_ascii_digit() {
                *i += 1;
            }
            let len: usize = rest[s..*i].parse().unwrap_or(0);
            *i += len;
            true
        } else {
            false
        }
    };
    if qualified(&mut i) {
        ints += 1; // this
    }
    if i < b.len() && b[i] == b'C' {
        i += 1;
    }
    if i >= b.len() || b[i] != b'F' {
        return (8, 4);
    }
    i += 1;
    let mut floats = 0;
    while i < b.len() && b[i] != b'_' {
        let mut indirect = false;
        while i < b.len() && matches!(b[i], b'C' | b'V' | b'U' | b'S' | b'P' | b'R') {
            if matches!(b[i], b'P' | b'R') {
                indirect = true;
            }
            i += 1;
        }
        if i >= b.len() {
            break;
        }
        match b[i] {
            b'v' => {
                i += 1;
                if !indirect {
                    continue;
                }
                ints += 1;
            }
            b'f' | b'd' if !indirect => {
                floats += 1;
                i += 1;
            }
            b'x' if !indirect => {
                ints += 2;
                i += 1;
            }
            b'b' | b'c' | b's' | b'i' | b'l' | b'w' | b'f' | b'd' | b'x' => {
                ints += 1;
                i += 1;
            }
            b'e' => {
                i += 1;
            }
            b'Q' | b'0'..=b'9' => {
                qualified(&mut i);
                ints += 1; // structs are passed by reference
            }
            b'F' => {
                // function pointer: skip to the matching return type
                let mut depth = 1;
                i += 1;
                while i < b.len() && depth > 0 {
                    if b[i] == b'_' {
                        depth -= 1;
                    }
                    i += 1;
                }
                if i < b.len() {
                    i += 1;
                }
                ints += 1;
            }
            b'A' => {
                while i < b.len() && b[i] != b'_' {
                    i += 1;
                }
                i += 1;
                ints += 1;
            }
            _ => {
                ints += 1;
                i += 1;
            }
        }
    }
    (ints.min(8), floats.min(4))
}

impl<H> Vm<H> {
    /// Verify `port` against the original function `name` whenever it is entered.  Returns false for unknown symbols.
    pub fn verify_port(&mut self, name: &str, port: super::HookFn<H>, ret: Ret) -> bool {
        let Some(addr) = self.img.addr(name) else { return false };
        if !self.hook_at.contains_key(&addr) {
            self.hook_addr(addr, name, None);
        }
        let i = self.hook_at[&addr];
        self.hooks[i].port = Some((port, ret));
        true
    }

    /// Answer an engine call made during the port's run from the original's record.  Returns true when handled.
    pub(super) fn replay_hook(&mut self, idx: usize) -> Result<bool, String> {
        let observe = self.hooks[idx].observe;
        let name = self.hooks[idx].name.clone();
        let rp = self.ver.replay.as_mut().unwrap();
        let fail = |rp: &mut Replay, msg: String| -> Result<bool, String> {
            if rp.error.is_none() {
                rp.error = Some(msg);
            }
            Err("verify-abort".into())
        };
        let Some(rec) = rp.recs.get(rp.cursor).cloned() else {
            return fail(rp, format!("port calls {name}, the original made no more engine calls"));
        };
        let k = rp.cursor;
        if rec.idx != idx {
            let want = self.hooks[rec.idx].name.clone();
            let rp = self.ver.replay.as_mut().unwrap();
            return fail(rp, format!("engine call #{k}: original calls {want}, port calls {name}"));
        }
        let (ni, nf) = match self.ver.param_cache.get(&idx) {
            Some(p) => *p,
            None => {
                let addr = self.hook_at.iter().find(|e| *e.1 == idx).map(|e| *e.0).unwrap_or(0);
                let p = self.ver.params.and_then(|f| f(addr)).unwrap_or_else(|| param_regs(&name));
                self.ver.param_cache.insert(idx, p);
                p
            }
        };
        for a in 0..ni {
            if self.st.cpu.r[3 + a] != rec.r[a] {
                let got = self.st.cpu.r[3 + a];
                let rp = self.ver.replay.as_mut().unwrap();
                return fail(rp, format!("engine call #{k} {name}: int arg {a} original {:#x} port {got:#x}", rec.r[a]));
            }
        }
        for a in 0..nf {
            if self.st.cpu.f[1 + a].to_bits() != rec.f[a].to_bits() {
                let got = self.st.cpu.f[1 + a];
                let rp = self.ver.replay.as_mut().unwrap();
                return fail(rp, format!("engine call #{k} {name}: float arg {a} original {} port {got}", rec.f[a]));
            }
        }
        let rp = self.ver.replay.as_mut().unwrap();
        rp.cursor = rec.end;
        let writes: Vec<(u32, Vec<u8>)> = rp.writes[rec.j0..rec.j1].iter().map(|(a, _, n)| (*a, n.clone())).collect();
        for (a, d) in writes {
            self.st.mem.write(a, &d);
        }
        if observe {
            // the original body follows (the record covered the observer and the first instruction)
            let pc = self.st.cpu.pc;
            let w = self.st.mem.r32(pc);
            self.st.cpu.step(&mut self.st.mem, w).map_err(|e| format!("{e} ({})", self.backtrace()))?;
            return Ok(true);
        }
        self.st.cpu.r[3] = rec.ret_r3;
        self.st.cpu.r[4] = rec.ret_r4;
        self.st.cpu.f[1] = rec.ret_f1;
        self.st.cpu.pc = self.st.cpu.lr;
        Ok(true)
    }

    /// Run the original and the port of hook `idx` on the current state, compare them, keep the original's outcome.
    pub(super) fn verify_call(&mut self, host: &mut H, idx: usize, port: super::HookFn<H>, ret: Ret) -> Result<(), String> {
        let entry = self.st.cpu.pc;
        let saved = self.st.cpu.clone();
        let sp0 = saved.r[1];
        let outer_journal = self.st.mem.journal.take();

        // 1. the original
        self.st.mem.journal = Some(vec![]);
        self.ver.recording = Some(vec![]);
        self.ver.depth += 1;
        self.st.cpu.lr = SENTINEL;
        self.depth += 1;
        let r = self.run(host);
        self.depth -= 1;
        self.ver.depth -= 1;
        let orig_writes = self.st.mem.journal.take().unwrap_or_default();
        let recs = self.ver.recording.take().unwrap_or_default();
        if let Err(e) = r {
            self.st.mem.journal = outer_journal;
            return Err(e);
        }
        let orig_cpu = self.st.cpu.clone();
        for (a, old, _) in orig_writes.iter().rev() {
            self.st.mem.write(*a, old);
        }
        self.st.cpu = saved.clone();

        // 2. the port
        self.st.mem.journal = Some(vec![]);
        self.ver.replay = Some(Replay { recs, writes: orig_writes, cursor: 0, error: None });
        self.ver.depth += 1;
        let pr = port(host, self);
        self.ver.depth -= 1;
        let port_writes = self.st.mem.journal.take().unwrap_or_default();
        let rp = self.ver.replay.take().unwrap();
        let port_cpu = self.st.cpu.clone();
        let fault = self.st.mem.fault.take();

        // 3. compare
        let mut diff: Vec<String> = vec![];
        if let Some(e) = rp.error.clone() {
            diff.push(e);
        } else if let Err(e) = &pr {
            diff.push(format!("port failed: {e}"));
        } else if rp.cursor < rp.recs.len() {
            let want = self.hooks[rp.recs[rp.cursor].idx].name.clone();
            diff.push(format!("port made {} of {} engine calls; next expected {want}", rp.cursor, rp.recs.len()));
        }
        if let Some(f) = fault {
            diff.push(format!("port memory fault: {f}"));
        }
        match ret {
            Ret::Void => {}
            Ret::Int => {
                if orig_cpu.r[3] != port_cpu.r[3] {
                    diff.push(format!("return r3 original {:#x} port {:#x}", orig_cpu.r[3], port_cpu.r[3]));
                }
            }
            Ret::Int64 => {
                if (orig_cpu.r[3], orig_cpu.r[4]) != (port_cpu.r[3], port_cpu.r[4]) {
                    diff.push(format!("return r3:r4 original {:#x}:{:#x} port {:#x}:{:#x}", orig_cpu.r[3], orig_cpu.r[4], port_cpu.r[3], port_cpu.r[4]));
                }
            }
            Ret::Float => {
                if orig_cpu.f[1].to_bits() != port_cpu.f[1].to_bits() {
                    diff.push(format!("return f1 original {} port {}", orig_cpu.f[1], port_cpu.f[1]));
                }
            }
        }
        // bytes: the original's final value against the port's, outside the function's own stack frame
        let frame = |a: u32| a >= sp0.wrapping_sub(0x10000) && a < sp0.wrapping_add(8);
        let mut orig_final: HashMap<u32, u8> = HashMap::new();
        let mut pre: HashMap<u32, u8> = HashMap::new();
        for (a, old, new) in &rp.writes {
            for (i, (o, n)) in old.iter().zip(new).enumerate() {
                let b = a.wrapping_add(i as u32);
                pre.entry(b).or_insert(*o);
                orig_final.insert(b, *n);
            }
        }
        let mut port_bytes: Vec<u32> = vec![];
        for (a, old, _) in &port_writes {
            for (i, o) in old.iter().enumerate() {
                let b = a.wrapping_add(i as u32);
                pre.entry(b).or_insert(*o);
                port_bytes.push(b);
            }
        }
        let mut addrs: Vec<u32> = orig_final.keys().copied().chain(port_bytes.iter().copied()).filter(|a| !frame(*a)).collect();
        addrs.sort_unstable();
        addrs.dedup();
        let mut bad = vec![];
        for a in addrs {
            let want = orig_final.get(&a).copied().unwrap_or(pre[&a]);
            let got = self.st.mem.r8(a);
            if want != got {
                bad.push((a, want, got));
            }
        }
        if !bad.is_empty() {
            let shown: Vec<String> = bad.iter().take(6).map(|(a, w, g)| format!("{a:#010x}: original {w:02x} port {g:02x}")).collect();
            diff.push(format!("{} byte(s) differ: {}", bad.len(), shown.join(", ")));
        }

        // 4. keep the original's outcome
        for (a, old, _) in port_writes.iter().rev() {
            self.st.mem.write(*a, old);
        }
        for (a, _, new) in &rp.writes {
            self.st.mem.write(*a, new);
        }
        self.st.mem.fault = None;
        self.st.cpu = orig_cpu;
        self.st.cpu.lr = saved.lr;
        self.st.cpu.pc = saved.lr;
        // the outer verification (if any) must see this call's writes in its own journal
        if let Some(mut j) = outer_journal {
            for (a, old, new) in rp.writes {
                j.push((a, old, new));
            }
            self.st.mem.journal = Some(j);
        }

        let st = self.ver.stats.entry(entry).or_default();
        st.calls += 1;
        if !diff.is_empty() {
            st.mismatches += 1;
            if st.first.is_none() {
                st.first = Some(diff.join("; "));
            }
        }
        Ok(())
    }

    /// One line per verified function: address, symbol, calls compared, mismatches, first difference.
    pub fn verify_report(&self) -> String {
        let mut rows: Vec<(&u32, &Stats)> = self.ver.stats.iter().collect();
        rows.sort_by_key(|r| *r.0);
        let mut out = String::from("# addr\tsymbol\tcalls\tmismatches\tfirst_difference\n");
        for (a, s) in rows {
            out += &format!("{a:08x}\t{}\t{}\t{}\t{}\n", self.name_of(*a), s.calls, s.mismatches, s.first.clone().unwrap_or_default());
        }
        out
    }
}
