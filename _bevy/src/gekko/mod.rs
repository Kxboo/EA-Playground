//! A PowerPC (Gekko) virtual machine that runs the original `playgroundz.elf` code.
//!
//! The minigames are not re-implemented by hand: their original methods execute here, on guest objects that live in
//! emulated memory.  Everything that touches hardware or engine services (rendering, Havok, audio, assets, the front end)
//! is a *hook*: a Rust function bound to a symbol name that reads its arguments from the guest registers and returns the
//! way the original would.  Functions that are neither hooked nor allowed to run natively stop the machine with their
//! name, so missing services are found by running rather than by guessing.
pub mod cpu;
pub mod elf;
pub mod mem;

use cpu::{Cpu, SENTINEL};
use elf::Image;
use mem::{Mem, MEM1_BASE, MEM2_BASE};
use std::collections::HashMap;

pub struct State {
    pub cpu: Cpu,
    pub mem: Mem,
}

pub type HookFn<H> = fn(&mut H, &mut Vm<H>) -> Result<(), String>;

struct HookEntry<H> {
    name: String,
    f: Option<HookFn<H>>,
    /// Observer hooks run `f` and then execute the original function instead of returning.
    observe: bool,
}

pub struct Vm<H> {
    pub st: State,
    pub img: Image,
    hooks: Vec<HookEntry<H>>,
    hook_at: HashMap<u32, usize>,
    hook_bits: Vec<u64>,
    heap: u32,
    heap_end: u32,
    /// Per-hook-name call counters (for diagnostics).
    pub trace: bool,
    pub depth: u32,
    /// Discovery mode: unhandled functions are recorded in `missing` and return 0 instead of stopping the machine.
    pub soft_traps: bool,
    pub missing: std::collections::BTreeMap<String, u32>,
    pub missing_symbols: Vec<String>,
    /// r4 at the end of the last `call` (second word of 64-bit results).
    pub ret_hi_lo: u32,
    /// Log entries into functions whose name contains this text (development aid, env `EAGL_PPC_CALLS`).
    pub call_filter: Option<String>,
}

pub const STACK_TOP: u32 = 0x817f_0000;

impl<H> Vm<H> {
    pub fn load() -> Result<Vm<H>, String> {
        let path = elf::find().ok_or("playgroundz.elf not found")?;
        let mut mem = Mem::default();
        let img = Image::load(&path, &mut mem)?;
        let words = ((mem::MEM1_SIZE) >> 2) / 64 + 1;
        let mut cpu = Cpu::default();
        cpu.r[1] = STACK_TOP;
        cpu.r[2] = img.addr("_SDA2_BASE_").ok_or("no _SDA2_BASE_")?;
        cpu.r[13] = img.addr("_SDA_BASE_").ok_or("no _SDA_BASE_")?;
        Ok(Vm {
            st: State { cpu, mem },
            img,
            hooks: vec![],
            hook_at: HashMap::new(),
            hook_bits: vec![0; words],
            heap: MEM2_BASE + 0x1000,
            heap_end: MEM2_BASE + mem::MEM2_SIZE as u32,
            trace: std::env::var("EAGL_PPC_TRACE").is_ok(),
            depth: 0,
            soft_traps: false,
            missing: Default::default(),
            missing_symbols: vec![],
            ret_hi_lo: 0,
            call_filter: std::env::var("EAGL_PPC_CALLS").ok(),
        })
    }

    fn set_bit(&mut self, addr: u32) {
        if addr >= MEM1_BASE && ((addr - MEM1_BASE) as usize) < mem::MEM1_SIZE {
            let i = ((addr - MEM1_BASE) >> 2) as usize;
            self.hook_bits[i >> 6] |= 1 << (i & 63);
        }
    }

    /// Bind `f` to the function symbol `name`.  Returns false when the symbol does not exist.
    pub fn hook(&mut self, name: &str, f: HookFn<H>) -> bool {
        let Some(addr) = self.img.addr(name) else { return false };
        self.hook_addr(addr, name, Some(f));
        true
    }
    /// Run `f` whenever the function `name` is entered, then continue with the original (native) code.
    pub fn observe(&mut self, name: &str, f: HookFn<H>) -> bool {
        let Some(addr) = self.img.addr(name) else { return false };
        self.hook_addr(addr, name, Some(f));
        let i = self.hook_at[&addr];
        self.hooks[i].observe = true;
        true
    }
    pub fn log_missing_symbol(&mut self, name: &str) {
        self.missing_symbols.push(name.to_string());
    }
    pub fn hook_addr(&mut self, addr: u32, name: &str, f: Option<HookFn<H>>) {
        if let Some(&i) = self.hook_at.get(&addr) {
            if f.is_some() || self.hooks[i].f.is_none() {
                self.hooks[i] = HookEntry { name: name.to_string(), f, observe: false };
            }
            return;
        }
        self.hook_at.insert(addr, self.hooks.len());
        self.hooks.push(HookEntry { name: name.to_string(), f, observe: false });
        self.set_bit(addr);
    }

    /// Every function for which `native(name)` is false and that has no hook stops the machine when entered.
    pub fn trap_unless(&mut self, native: impl Fn(&str) -> bool) {
        let traps: Vec<(u32, String)> = self.img.funcs.iter().map(|&i| &self.img.symbols[i]).filter(|s| !native(&s.name)).map(|s| (s.addr, s.name.clone())).collect();
        for (a, n) in traps {
            if !self.hook_at.contains_key(&a) {
                self.hook_addr(a, &n, None);
            }
        }
    }

    /// Integer argument `i` (r3 + i) of the call being hooked.
    pub fn a(&self, i: usize) -> u32 {
        self.st.cpu.r[3 + i]
    }
    /// Float argument `i` (f1 + i).
    pub fn fa(&self, i: usize) -> f32 {
        self.st.cpu.f[1 + i] as f32
    }
    pub fn ret(&mut self, v: u32) {
        self.st.cpu.r[3] = v;
    }
    pub fn fret(&mut self, v: f32) {
        self.st.cpu.f[1] = v as f64;
        self.st.cpu.ps1[1] = v as f64;
    }
    pub fn r32(&mut self, a: u32) -> u32 {
        self.st.mem.r32(a)
    }
    pub fn w32(&mut self, a: u32, v: u32) {
        self.st.mem.w32(a, v)
    }

    /// Bind `f` to every function symbol (without an explicit hook) for which `pred(name)` holds.
    pub fn hook_matching(&mut self, pred: impl Fn(&str) -> bool, f: HookFn<H>) {
        let list: Vec<(u32, String)> = self.img.funcs.iter().map(|&i| &self.img.symbols[i]).filter(|s| pred(&s.name)).map(|s| (s.addr, s.name.clone())).collect();
        for (a, n) in list {
            let explicit = self.hook_at.get(&a).map(|&i| self.hooks[i].f.is_some()).unwrap_or(false);
            if !explicit {
                self.hook_addr(a, &n, Some(f));
            }
        }
    }

    /// A NUL-terminated copy of `s` in guest memory.
    pub fn alloc_cstr(&mut self, s: &str) -> u32 {
        let a = self.alloc(s.len() as u32 + 1, 4);
        self.st.mem.write(a, s.as_bytes());
        self.st.mem.w8(a + s.len() as u32, 0);
        a
    }

    pub fn alloc(&mut self, size: u32, align: u32) -> u32 {
        let a = (self.heap + align.max(4) - 1) & !(align.max(4) - 1);
        let end = a + size.max(4);
        assert!(end <= self.heap_end, "guest heap exhausted");
        self.heap = end;
        a
    }

    /// Allocate zeroed guest memory.
    pub fn alloc_zeroed(&mut self, size: u32, align: u32) -> u32 {
        let a = self.alloc(size, align);
        self.st.mem.fill(a, size as usize, 0);
        a
    }

    pub fn name_of(&self, addr: u32) -> String {
        match self.img.func_at(addr) {
            Some(s) if s.addr == addr => s.name.clone(),
            Some(s) => format!("{}+{:#x}", s.name, addr - s.addr),
            None => format!("{addr:#010x}"),
        }
    }

    pub fn backtrace(&mut self) -> String {
        let mut out = vec![format!("pc {}", self.name_of(self.st.cpu.pc)), format!("lr {}", self.name_of(self.st.cpu.lr))];
        let mut sp = self.st.cpu.r[1];
        for _ in 0..10 {
            if sp < MEM1_BASE {
                break;
            }
            let prev = self.st.mem.r32(sp);
            if prev <= sp || prev == 0 {
                break;
            }
            let lr = self.st.mem.r32(prev + 4);
            out.push(self.name_of(lr));
            sp = prev;
        }
        out.join(" <- ")
    }

    /// Call the guest function at `addr` with integer arguments `args` (r3..) and float arguments `fargs` (f1..).
    /// Callee-saved state is preserved so hooks may call back into the guest.
    pub fn call(&mut self, host: &mut H, addr: u32, args: &[u32], fargs: &[f64]) -> Result<u32, String> {
        let saved = self.st.cpu.clone();
        let sp = (saved.r[1].wrapping_sub(0x200)) & !0xf;
        self.st.cpu.r[1] = sp;
        for (i, a) in args.iter().enumerate() {
            self.st.cpu.r[3 + i] = *a;
        }
        for (i, a) in fargs.iter().enumerate() {
            self.st.cpu.f[1 + i] = *a;
            self.st.cpu.ps1[1 + i] = *a;
        }
        self.st.cpu.lr = SENTINEL;
        self.st.cpu.pc = addr;
        self.depth += 1;
        let r = self.run(host);
        self.depth -= 1;
        let (ret, fret) = (self.st.cpu.r[3], self.st.cpu.f[1]);
        self.ret_hi_lo = self.st.cpu.r[4];
        let steps = self.st.cpu.steps;
        self.st.cpu = saved;
        self.st.cpu.steps = steps;
        // the return value is left in r3 / f1 for hooks that want it
        self.st.cpu.r[3] = ret;
        self.st.cpu.f[1] = fret;
        r.map(|_| ret)
    }

    pub fn call_by_name(&mut self, host: &mut H, name: &str, args: &[u32], fargs: &[f64]) -> Result<u32, String> {
        let a = self.img.addr(name).ok_or_else(|| format!("no symbol {name}"))?;
        self.call(host, a, args, fargs)
    }

    fn run(&mut self, host: &mut H) -> Result<(), String> {
        let mut budget: u64 = 400_000_000;
        loop {
            let pc = self.st.cpu.pc;
            if pc == SENTINEL {
                return Ok(());
            }
            if budget == 0 {
                return Err(format!("step budget exhausted at {}", self.backtrace()));
            }
            budget -= 1;
            if pc >= MEM1_BASE {
                let i = ((pc - MEM1_BASE) >> 2) as usize;
                if i >> 6 < self.hook_bits.len() && (self.hook_bits[i >> 6] >> (i & 63)) & 1 != 0 {
                    let idx = self.hook_at[&pc];
                    let f = self.hooks[idx].f;
                    if self.hooks[idx].observe {
                        if let Some(f) = f {
                            f(host, self).map_err(|e| format!("{}: {e}", self.hooks[idx].name))?;
                        }
                        // fall through: execute the first instruction of the original function
                        let w = self.st.mem.r32(pc);
                        if let Err(e) = self.st.cpu.step(&mut self.st.mem, w) {
                            return Err(format!("{e} ({})", self.backtrace()));
                        }
                        continue;
                    }
                    match f {
                        Some(f) => {
                            if self.trace {
                                eprintln!("[ppc] hook {}", self.hooks[idx].name);
                            }
                            f(host, self).map_err(|e| format!("{}: {e}", self.hooks[idx].name))?;
                            self.st.cpu.pc = self.st.cpu.lr;
                            if let Some(e) = self.st.mem.fault.take() {
                                return Err(format!("{e} in hook {}", self.hooks[idx].name));
                            }
                            continue;
                        }
                        None => {
                            let name = self.hooks[idx].name.clone();
                            if self.soft_traps {
                                *self.missing.entry(name).or_insert(0) += 1;
                                self.st.cpu.r[3] = 0;
                                self.st.cpu.f[1] = 0.;
                                self.st.cpu.pc = self.st.cpu.lr;
                                continue;
                            }
                            return Err(format!("unhandled engine function {name} ({})", self.backtrace()));
                        }
                    }
                }
            }
            if let Some(f) = &self.call_filter {
                if let Some(sym) = self.img.func_at(pc) {
                    if sym.addr == pc && sym.name.contains(f.as_str()) {
                        eprintln!("[call] {}", sym.name);
                    }
                }
            }
            let w = self.st.mem.r32(pc);
            if let Err(e) = self.st.cpu.step(&mut self.st.mem, w) {
                return Err(format!("{e} ({})", self.backtrace()));
            }
            if let Some(e) = self.st.mem.fault.take() {
                return Err(format!("{e} ({})", self.backtrace()));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct NoHost;

    fn vm() -> Option<Vm<NoHost>> {
        Vm::<NoHost>::load().ok()
    }

    #[test]
    fn pure_math_runs_natively() {
        let Some(mut vm) = vm() else { return };
        let mut h = NoHost;
        // rmDistanceSquaredXZ(const rmVector3&, const rmVector3&)
        let a = vm.alloc(16, 16);
        let b = vm.alloc(16, 16);
        for (i, v) in [1.0f32, 5.0, 2.0].iter().enumerate() {
            vm.st.mem.wf32(a + 4 * i as u32, *v);
        }
        for (i, v) in [4.0f32, 9.0, 6.0].iter().enumerate() {
            vm.st.mem.wf32(b + 4 * i as u32, *v);
        }
        vm.call_by_name(&mut h, "rmDistanceSquaredXZ__FRC9rmVector3RC9rmVector3", &[a, b], &[]).unwrap();
        assert_eq!(vm.st.cpu.f[1] as f32, 25.0);
    }

    #[test]
    fn unhandled_engine_functions_stop_with_their_name() {
        let Some(mut vm) = vm() else { return };
        let mut h = NoHost;
        vm.trap_unless(|n| n.starts_with("rm"));
        let e = vm.call_by_name(&mut h, "GetPlayerCharacter__8WorldManFi", &[0, 0], &[]).unwrap_err();
        assert!(e.contains("GetPlayerCharacter"), "{e}");
    }
}
