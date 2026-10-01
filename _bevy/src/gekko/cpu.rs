//! PowerPC 750CL (Gekko / Broadway) interpreter: integer, floating point, load/store and the paired-single subset the
//! compiler emits (register spills, `ps_merge`, `ps_sel`, quantized loads / stores through the GQRs).
use super::mem::Mem;

pub const SENTINEL: u32 = 0xFFFF_FFF0;

#[derive(Clone)]
pub struct Cpu {
    pub r: [u32; 32],
    pub f: [f64; 32],
    /// Second slot of the paired-single registers.
    pub ps1: [f64; 32],
    pub cr: [u8; 8],
    pub lr: u32,
    pub ctr: u32,
    pub ca: bool,
    pub so: bool,
    pub pc: u32,
    pub gqr: [u32; 8],
    pub fpscr: u32,
    pub steps: u64,
}

impl Default for Cpu {
    fn default() -> Self {
        Cpu { r: [0; 32], f: [0.; 32], ps1: [0.; 32], cr: [0; 8], lr: 0, ctr: 0, ca: false, so: false, pc: 0, gqr: [0; 8], fpscr: 0, steps: 0 }
    }
}

fn mask(mb: u32, me: u32) -> u32 {
    let bits = |lo: u32, hi: u32| -> u32 {
        let n = hi - lo + 1;
        let ones = if n == 32 { u32::MAX } else { (1u32 << n) - 1 };
        ones << (31 - hi)
    };
    if mb <= me {
        bits(mb, me)
    } else {
        bits(mb, 31) | bits(0, me)
    }
}

fn sx16(w: u32) -> i32 {
    (w & 0xffff) as u16 as i16 as i32
}

fn round_single(x: f64) -> f64 {
    x as f32 as f64
}

impl Cpu {
    pub fn set_cr0(&mut self, v: u32) {
        let s = v as i32;
        self.cr[0] = (if s < 0 { 8 } else { 0 }) | (if s > 0 { 4 } else { 0 }) | (if s == 0 { 2 } else { 0 }) | self.so as u8;
    }
    fn cr_bit(&self, bi: u32) -> bool {
        (self.cr[(bi >> 2) as usize] >> (3 - (bi & 3))) & 1 != 0
    }
    fn set_cr_bit(&mut self, bi: u32, v: bool) {
        let f = (bi >> 2) as usize;
        let m = 1u8 << (3 - (bi & 3));
        if v {
            self.cr[f] |= m;
        } else {
            self.cr[f] &= !m;
        }
    }
    fn branch_ok(&mut self, bo: u32, bi: u32) -> bool {
        let ctr_ok = if bo & 4 == 0 {
            self.ctr = self.ctr.wrapping_sub(1);
            (self.ctr != 0) ^ (bo & 2 != 0)
        } else {
            true
        };
        let cond_ok = bo & 16 != 0 || self.cr_bit(bi) == ((bo >> 3) & 1 != 0);
        ctr_ok && cond_ok
    }
    fn cmp_set(&mut self, field: usize, lt: bool, gt: bool, eq: bool) {
        self.cr[field] = (if lt { 8 } else { 0 }) | (if gt { 4 } else { 0 }) | (if eq { 2 } else { 0 }) | self.so as u8;
    }
    fn fcmp(&mut self, field: usize, a: f64, b: f64) {
        self.cr[field] = if a.is_nan() || b.is_nan() {
            1
        } else if a < b {
            8
        } else if a > b {
            4
        } else {
            2
        };
    }

    fn quant_load(&self, mem: &mut Mem, addr: u32, ty: u32, scale: i32, two: bool) -> (f64, f64, u32) {
        let (v0, v1, size) = match ty {
            4 => (mem.r8(addr) as f64, if two { mem.r8(addr + 1) as f64 } else { 1. }, 1),
            5 => (mem.r16(addr) as f64, if two { mem.r16(addr + 2) as f64 } else { 1. }, 2),
            6 => (mem.r8(addr) as i8 as f64, if two { mem.r8(addr + 1) as i8 as f64 } else { 1. }, 1),
            7 => (mem.r16(addr) as i16 as f64, if two { mem.r16(addr + 2) as i16 as f64 } else { 1. }, 2),
            _ => (mem.rf32(addr) as f64, if two { mem.rf32(addr + 4) as f64 } else { 1. }, 4),
        };
        if ty == 0 || ty > 7 || ty == 1 || ty == 2 || ty == 3 {
            return (v0, v1, size * if two { 2 } else { 1 });
        }
        let k = 2f64.powi(-scale);
        (v0 * k, if two { v1 * k } else { v1 }, size * if two { 2 } else { 1 })
    }
    fn quant_store(&self, mem: &mut Mem, addr: u32, ty: u32, scale: i32, two: bool, a: f64, b: f64) -> u32 {
        let k = 2f64.powi(scale);
        let clampi = |v: f64, lo: f64, hi: f64| v.max(lo).min(hi) as i64;
        match ty {
            4 => {
                mem.w8(addr, clampi(a * k, 0., 255.) as u8);
                if two {
                    mem.w8(addr + 1, clampi(b * k, 0., 255.) as u8);
                }
                if two { 2 } else { 1 }
            }
            5 => {
                mem.w16(addr, clampi(a * k, 0., 65535.) as u16);
                if two {
                    mem.w16(addr + 2, clampi(b * k, 0., 65535.) as u16);
                }
                if two { 4 } else { 2 }
            }
            6 => {
                mem.w8(addr, clampi(a * k, -128., 127.) as i8 as u8);
                if two {
                    mem.w8(addr + 1, clampi(b * k, -128., 127.) as i8 as u8);
                }
                if two { 2 } else { 1 }
            }
            7 => {
                mem.w16(addr, clampi(a * k, -32768., 32767.) as i16 as u16);
                if two {
                    mem.w16(addr + 2, clampi(b * k, -32768., 32767.) as i16 as u16);
                }
                if two { 4 } else { 2 }
            }
            _ => {
                mem.wf32(addr, a as f32);
                if two {
                    mem.wf32(addr + 4, b as f32);
                }
                if two { 8 } else { 4 }
            }
        }
    }

    /// Execute one instruction at `self.pc`.  Branch targets and `bl` link handling happen here; hooks are the caller's job.
    pub fn step(&mut self, mem: &mut Mem, w: u32) -> Result<(), String> {
        let pc = self.pc;
        let mut next = pc.wrapping_add(4);
        let op = w >> 26;
        let rd = ((w >> 21) & 31) as usize;
        let ra = ((w >> 16) & 31) as usize;
        let rb = ((w >> 11) & 31) as usize;
        let imm = sx16(w);
        let uimm = w & 0xffff;
        let rc = w & 1 != 0;
        let base = |c: &Cpu| if ra == 0 { 0 } else { c.r[ra] };
        self.steps += 1;
        match op {
            14 => self.r[rd] = base(self).wrapping_add(imm as u32),
            15 => self.r[rd] = base(self).wrapping_add((imm as u32) << 16),
            12 | 13 => {
                // addic / addic.
                let (a, b) = (self.r[ra], imm as u32);
                let (s, c) = a.overflowing_add(b);
                self.ca = c;
                self.r[rd] = s;
                if op == 13 {
                    self.set_cr0(s);
                }
            }
            8 => {
                // subfic
                let (a, b) = (!self.r[ra], imm as u32);
                let t = a as u64 + b as u64 + 1;
                self.ca = t > u32::MAX as u64;
                self.r[rd] = t as u32;
            }
            7 => self.r[rd] = (self.r[ra] as i32).wrapping_mul(imm) as u32,
            24 => self.r[ra] = self.r[rd] | uimm,
            25 => self.r[ra] = self.r[rd] | (uimm << 16),
            26 => self.r[ra] = self.r[rd] ^ uimm,
            27 => self.r[ra] = self.r[rd] ^ (uimm << 16),
            28 => {
                self.r[ra] = self.r[rd] & uimm;
                self.set_cr0(self.r[ra]);
            }
            29 => {
                self.r[ra] = self.r[rd] & (uimm << 16);
                self.set_cr0(self.r[ra]);
            }
            21 => {
                let sh = (w >> 11) & 31;
                let (mb, me) = ((w >> 6) & 31, (w >> 1) & 31);
                self.r[ra] = self.r[rd].rotate_left(sh) & mask(mb, me);
                if rc {
                    self.set_cr0(self.r[ra]);
                }
            }
            20 => {
                let sh = (w >> 11) & 31;
                let (mb, me) = ((w >> 6) & 31, (w >> 1) & 31);
                let m = mask(mb, me);
                self.r[ra] = (self.r[rd].rotate_left(sh) & m) | (self.r[ra] & !m);
                if rc {
                    self.set_cr0(self.r[ra]);
                }
            }
            23 => {
                let sh = self.r[rb] & 31;
                let (mb, me) = ((w >> 6) & 31, (w >> 1) & 31);
                self.r[ra] = self.r[rd].rotate_left(sh) & mask(mb, me);
                if rc {
                    self.set_cr0(self.r[ra]);
                }
            }
            11 => {
                let a = self.r[ra] as i32;
                self.cmp_set(((w >> 23) & 7) as usize, a < imm, a > imm, a == imm);
            }
            10 => {
                let a = self.r[ra];
                self.cmp_set(((w >> 23) & 7) as usize, a < uimm, a > uimm, a == uimm);
            }
            32 => {
                let a = base(self).wrapping_add(imm as u32);
                self.r[rd] = mem.r32(a);
            }
            33 => {
                let a = self.r[ra].wrapping_add(imm as u32);
                self.r[rd] = mem.r32(a);
                self.r[ra] = a;
            }
            34 => {
                let a = base(self).wrapping_add(imm as u32);
                self.r[rd] = mem.r8(a) as u32;
            }
            35 => {
                let a = self.r[ra].wrapping_add(imm as u32);
                self.r[rd] = mem.r8(a) as u32;
                self.r[ra] = a;
            }
            40 => {
                let a = base(self).wrapping_add(imm as u32);
                self.r[rd] = mem.r16(a) as u32;
            }
            41 => {
                let a = self.r[ra].wrapping_add(imm as u32);
                self.r[rd] = mem.r16(a) as u32;
                self.r[ra] = a;
            }
            42 => {
                let a = base(self).wrapping_add(imm as u32);
                self.r[rd] = mem.r16(a) as i16 as i32 as u32;
            }
            43 => {
                let a = self.r[ra].wrapping_add(imm as u32);
                self.r[rd] = mem.r16(a) as i16 as i32 as u32;
                self.r[ra] = a;
            }
            36 => {
                let a = base(self).wrapping_add(imm as u32);
                mem.w32(a, self.r[rd]);
            }
            37 => {
                let a = self.r[ra].wrapping_add(imm as u32);
                mem.w32(a, self.r[rd]);
                self.r[ra] = a;
            }
            38 => {
                let a = base(self).wrapping_add(imm as u32);
                mem.w8(a, self.r[rd] as u8);
            }
            39 => {
                let a = self.r[ra].wrapping_add(imm as u32);
                mem.w8(a, self.r[rd] as u8);
                self.r[ra] = a;
            }
            44 => {
                let a = base(self).wrapping_add(imm as u32);
                mem.w16(a, self.r[rd] as u16);
            }
            45 => {
                let a = self.r[ra].wrapping_add(imm as u32);
                mem.w16(a, self.r[rd] as u16);
                self.r[ra] = a;
            }
            46 => {
                let mut a = base(self).wrapping_add(imm as u32);
                for i in rd..32 {
                    self.r[i] = mem.r32(a);
                    a = a.wrapping_add(4);
                }
            }
            47 => {
                let mut a = base(self).wrapping_add(imm as u32);
                for i in rd..32 {
                    mem.w32(a, self.r[i]);
                    a = a.wrapping_add(4);
                }
            }
            48 => {
                let a = base(self).wrapping_add(imm as u32);
                let v = mem.rf32(a) as f64;
                self.f[rd] = v;
                self.ps1[rd] = v;
            }
            49 => {
                let a = self.r[ra].wrapping_add(imm as u32);
                let v = mem.rf32(a) as f64;
                self.f[rd] = v;
                self.ps1[rd] = v;
                self.r[ra] = a;
            }
            50 => {
                let a = base(self).wrapping_add(imm as u32);
                self.f[rd] = f64::from_bits(mem.r64(a));
            }
            51 => {
                let a = self.r[ra].wrapping_add(imm as u32);
                self.f[rd] = f64::from_bits(mem.r64(a));
                self.r[ra] = a;
            }
            52 => {
                let a = base(self).wrapping_add(imm as u32);
                mem.wf32(a, self.f[rd] as f32);
            }
            53 => {
                let a = self.r[ra].wrapping_add(imm as u32);
                mem.wf32(a, self.f[rd] as f32);
                self.r[ra] = a;
            }
            54 => {
                let a = base(self).wrapping_add(imm as u32);
                mem.w64(a, self.f[rd].to_bits());
            }
            55 => {
                let a = self.r[ra].wrapping_add(imm as u32);
                mem.w64(a, self.f[rd].to_bits());
                self.r[ra] = a;
            }
            // psq_l / psq_lu / psq_st / psq_stu
            56 | 57 | 60 | 61 => {
                let i = ((w >> 12) & 7) as usize;
                let two = (w >> 15) & 1 == 0;
                let d = (((w & 0xfff) << 20) as i32 >> 20) as u32;
                let a = base(self).wrapping_add(d);
                let g = self.gqr[i];
                if op == 56 || op == 57 {
                    let (ty, sc) = ((g >> 16) & 7, ((((g >> 24) & 63) << 26) as i32 >> 26));
                    let (v0, v1, _) = self.quant_load(mem, a, ty, sc, two);
                    self.f[rd] = v0;
                    self.ps1[rd] = v1;
                } else {
                    let (ty, sc) = (g & 7, ((((g >> 8) & 63) << 26) as i32 >> 26));
                    let (x, y) = (self.f[rd], self.ps1[rd]);
                    self.quant_store(mem, a, ty, sc, two, x, y);
                }
                if op == 57 || op == 61 {
                    self.r[ra] = a;
                }
            }
            18 => {
                let li = ((w & 0x03ff_fffc) as i32) << 6 >> 6;
                let tgt = if w & 2 != 0 { li as u32 } else { pc.wrapping_add(li as u32) };
                if w & 1 != 0 {
                    self.lr = next;
                }
                next = tgt;
            }
            16 => {
                let (bo, bi) = ((w >> 21) & 31, (w >> 16) & 31);
                let bd = ((w & 0xfffc) as u16 as i16 as i32) as u32;
                if w & 1 != 0 {
                    // link is set whether or not the branch is taken
                }
                let lk = if w & 1 != 0 { Some(next) } else { None };
                let ok = self.branch_ok(bo, bi);
                if let Some(l) = lk {
                    self.lr = l;
                }
                if ok {
                    next = if w & 2 != 0 { bd } else { pc.wrapping_add(bd) };
                }
            }
            19 => {
                let xo = (w >> 1) & 0x3ff;
                let (bo, bi) = ((w >> 21) & 31, (w >> 16) & 31);
                match xo {
                    16 | 528 => {
                        let t = if xo == 16 { self.lr } else { self.ctr };
                        let lk = if w & 1 != 0 { Some(next) } else { None };
                        let ok = self.branch_ok(bo, bi);
                        if let Some(l) = lk {
                            self.lr = l;
                        }
                        if ok {
                            next = t & !3;
                        }
                    }
                    0 => {
                        let (bf, bfa) = (rd >> 2, ra >> 2);
                        self.cr[bf] = self.cr[bfa];
                    }
                    150 => {}
                    33 | 129 | 193 | 225 | 257 | 289 | 417 | 449 => {
                        let (a, b) = (self.cr_bit(ra as u32), self.cr_bit(rb as u32));
                        let v = match xo {
                            33 => !(a | b),
                            129 => a & !b,
                            193 => a ^ b,
                            225 => !(a & b),
                            257 => a & b,
                            289 => a == b,
                            417 => a | !b,
                            _ => a | b,
                        };
                        self.set_cr_bit(rd as u32, v);
                    }
                    _ => return Err(format!("op19 xo {xo} at {pc:#x}")),
                }
            }
            31 => self.op31(mem, w)?,
            59 | 63 => self.fp_ops(mem, w)?,
            4 => self.paired(mem, w)?,
            17 => return Err(format!("sc at {pc:#x}")),
            _ => return Err(format!("opcode {op} (word {w:08x}) at {pc:#x}")),
        }
        self.pc = next;
        Ok(())
    }

    fn op31(&mut self, mem: &mut Mem, w: u32) -> Result<(), String> {
        let pc = self.pc;
        let xo = (w >> 1) & 0x3ff;
        let rd = ((w >> 21) & 31) as usize;
        let ra = ((w >> 16) & 31) as usize;
        let rb = ((w >> 11) & 31) as usize;
        let rc = w & 1 != 0;
        let oe = xo & 0x200 != 0;
        let _ = oe;
        let ea = |c: &Cpu| (if ra == 0 { 0 } else { c.r[ra] }).wrapping_add(c.r[rb]);
        // arithmetic forms ignore the OE bit (bit 9 of xo); the overflow flags are not modelled
        let axo = xo & 0x1ff;
        let mut result_reg = None;
        match axo {
            266 => {
                self.r[rd] = self.r[ra].wrapping_add(self.r[rb]);
                result_reg = Some(rd);
            }
            40 => {
                self.r[rd] = self.r[rb].wrapping_sub(self.r[ra]);
                result_reg = Some(rd);
            }
            10 => {
                let (s, c) = self.r[ra].overflowing_add(self.r[rb]);
                self.ca = c;
                self.r[rd] = s;
                result_reg = Some(rd);
            }
            8 => {
                let t = (!self.r[ra]) as u64 + self.r[rb] as u64 + 1;
                self.ca = t > u32::MAX as u64;
                self.r[rd] = t as u32;
                result_reg = Some(rd);
            }
            138 => {
                let t = self.r[ra] as u64 + self.r[rb] as u64 + self.ca as u64;
                self.ca = t > u32::MAX as u64;
                self.r[rd] = t as u32;
                result_reg = Some(rd);
            }
            136 => {
                let t = (!self.r[ra]) as u64 + self.r[rb] as u64 + self.ca as u64;
                self.ca = t > u32::MAX as u64;
                self.r[rd] = t as u32;
                result_reg = Some(rd);
            }
            202 => {
                let t = self.r[ra] as u64 + self.ca as u64;
                self.ca = t > u32::MAX as u64;
                self.r[rd] = t as u32;
                result_reg = Some(rd);
            }
            200 => {
                let t = (!self.r[ra]) as u64 + self.ca as u64;
                self.ca = t > u32::MAX as u64;
                self.r[rd] = t as u32;
                result_reg = Some(rd);
            }
            234 => {
                let t = self.r[ra] as u64 + 0xffff_ffff + self.ca as u64;
                self.ca = t > u32::MAX as u64;
                self.r[rd] = t as u32;
                result_reg = Some(rd);
            }
            232 => {
                let t = (!self.r[ra]) as u64 + 0xffff_ffff + self.ca as u64;
                self.ca = t > u32::MAX as u64;
                self.r[rd] = t as u32;
                result_reg = Some(rd);
            }
            104 => {
                self.r[rd] = self.r[ra].wrapping_neg();
                result_reg = Some(rd);
            }
            235 => {
                self.r[rd] = (self.r[ra] as i32).wrapping_mul(self.r[rb] as i32) as u32;
                result_reg = Some(rd);
            }
            75 => {
                self.r[rd] = (((self.r[ra] as i32 as i64) * (self.r[rb] as i32 as i64)) >> 32) as u32;
                result_reg = Some(rd);
            }
            11 => {
                self.r[rd] = (((self.r[ra] as u64) * (self.r[rb] as u64)) >> 32) as u32;
                result_reg = Some(rd);
            }
            491 => {
                let (a, b) = (self.r[ra] as i32, self.r[rb] as i32);
                self.r[rd] = if b == 0 || (a == i32::MIN && b == -1) { 0 } else { (a / b) as u32 };
                result_reg = Some(rd);
            }
            459 => {
                let (a, b) = (self.r[ra], self.r[rb]);
                self.r[rd] = if b == 0 { 0 } else { a / b };
                result_reg = Some(rd);
            }
            _ => {}
        }
        if let Some(r) = result_reg {
            if rc {
                self.set_cr0(self.r[r]);
            }
            return Ok(());
        }
        let mut logical = None;
        match xo {
            28 => logical = Some(self.r[rd] & self.r[rb]),
            60 => logical = Some(self.r[rd] & !self.r[rb]),
            444 => logical = Some(self.r[rd] | self.r[rb]),
            412 => logical = Some(self.r[rd] | !self.r[rb]),
            316 => logical = Some(self.r[rd] ^ self.r[rb]),
            476 => logical = Some(!(self.r[rd] & self.r[rb])),
            124 => logical = Some(!(self.r[rd] | self.r[rb])),
            284 => logical = Some(!(self.r[rd] ^ self.r[rb])),
            24 => {
                let s = self.r[rb] & 63;
                logical = Some(if s < 32 { self.r[rd] << s } else { 0 });
            }
            536 => {
                let s = self.r[rb] & 63;
                logical = Some(if s < 32 { self.r[rd] >> s } else { 0 });
            }
            792 => {
                let s = self.r[rb] & 63;
                let v = self.r[rd] as i32;
                let (res, ca) = if s < 32 { (v >> s, v < 0 && s > 0 && (self.r[rd] & ((1u32 << s) - 1)) != 0) } else { (v >> 31, v < 0) };
                self.ca = ca;
                logical = Some(res as u32);
            }
            824 => {
                let s = rb as u32;
                let v = self.r[rd] as i32;
                self.ca = v < 0 && s > 0 && (self.r[rd] & ((1u32 << s) - 1)) != 0;
                logical = Some((v >> s) as u32);
            }
            922 => logical = Some(self.r[rd] as i16 as i32 as u32),
            954 => logical = Some(self.r[rd] as i8 as i32 as u32),
            26 => logical = Some(self.r[rd].leading_zeros()),
            _ => {}
        }
        if let Some(v) = logical {
            self.r[ra] = v;
            if rc {
                self.set_cr0(v);
            }
            return Ok(());
        }
        match xo {
            0 => {
                let (a, b) = (self.r[ra] as i32, self.r[rb] as i32);
                self.cmp_set(rd >> 2, a < b, a > b, a == b);
            }
            32 => {
                let (a, b) = (self.r[ra], self.r[rb]);
                self.cmp_set(rd >> 2, a < b, a > b, a == b);
            }
            23 => self.r[rd] = mem.r32(ea(self)),
            55 => {
                let a = self.r[ra].wrapping_add(self.r[rb]);
                self.r[rd] = mem.r32(a);
                self.r[ra] = a;
            }
            87 => self.r[rd] = mem.r8(ea(self)) as u32,
            119 => {
                let a = self.r[ra].wrapping_add(self.r[rb]);
                self.r[rd] = mem.r8(a) as u32;
                self.r[ra] = a;
            }
            279 => self.r[rd] = mem.r16(ea(self)) as u32,
            311 => {
                let a = self.r[ra].wrapping_add(self.r[rb]);
                self.r[rd] = mem.r16(a) as u32;
                self.r[ra] = a;
            }
            343 => self.r[rd] = mem.r16(ea(self)) as i16 as i32 as u32,
            375 => {
                let a = self.r[ra].wrapping_add(self.r[rb]);
                self.r[rd] = mem.r16(a) as i16 as i32 as u32;
                self.r[ra] = a;
            }
            534 => self.r[rd] = mem.r32(ea(self)).swap_bytes(),
            790 => self.r[rd] = (mem.r16(ea(self))).swap_bytes() as u32,
            151 => mem.w32(ea(self), self.r[rd]),
            183 => {
                let a = self.r[ra].wrapping_add(self.r[rb]);
                mem.w32(a, self.r[rd]);
                self.r[ra] = a;
            }
            215 => mem.w8(ea(self), self.r[rd] as u8),
            247 => {
                let a = self.r[ra].wrapping_add(self.r[rb]);
                mem.w8(a, self.r[rd] as u8);
                self.r[ra] = a;
            }
            407 => mem.w16(ea(self), self.r[rd] as u16),
            439 => {
                let a = self.r[ra].wrapping_add(self.r[rb]);
                mem.w16(a, self.r[rd] as u16);
                self.r[ra] = a;
            }
            662 => mem.w32(ea(self), self.r[rd].swap_bytes()),
            918 => mem.w16(ea(self), (self.r[rd] as u16).swap_bytes()),
            535 => {
                let v = mem.rf32(ea(self)) as f64;
                self.f[rd] = v;
                self.ps1[rd] = v;
            }
            567 => {
                let a = self.r[ra].wrapping_add(self.r[rb]);
                let v = mem.rf32(a) as f64;
                self.f[rd] = v;
                self.ps1[rd] = v;
                self.r[ra] = a;
            }
            599 => self.f[rd] = f64::from_bits(mem.r64(ea(self))),
            631 => {
                let a = self.r[ra].wrapping_add(self.r[rb]);
                self.f[rd] = f64::from_bits(mem.r64(a));
                self.r[ra] = a;
            }
            663 => mem.wf32(ea(self), self.f[rd] as f32),
            695 => {
                let a = self.r[ra].wrapping_add(self.r[rb]);
                mem.wf32(a, self.f[rd] as f32);
                self.r[ra] = a;
            }
            727 => mem.w64(ea(self), self.f[rd].to_bits()),
            759 => {
                let a = self.r[ra].wrapping_add(self.r[rb]);
                mem.w64(a, self.f[rd].to_bits());
                self.r[ra] = a;
            }
            983 => mem.w32(ea(self), self.f[rd].to_bits() as u32),
            // lswi / stswi are not used by the compiler
            339 => {
                let spr = ((w >> 16) & 31) | (((w >> 11) & 31) << 5);
                self.r[rd] = match spr {
                    1 => ((self.so as u32) << 31) | ((self.ca as u32) << 29),
                    8 => self.lr,
                    9 => self.ctr,
                    912..=919 => self.gqr[(spr - 912) as usize],
                    _ => return Err(format!("mfspr {spr} at {pc:#x}")),
                };
            }
            467 => {
                let spr = ((w >> 16) & 31) | (((w >> 11) & 31) << 5);
                let v = self.r[rd];
                match spr {
                    1 => {
                        self.so = v >> 31 != 0;
                        self.ca = (v >> 29) & 1 != 0;
                    }
                    8 => self.lr = v,
                    9 => self.ctr = v,
                    912..=919 => self.gqr[(spr - 912) as usize] = v,
                    // hardware setup registers (HID, BATs, ...) have no effect here
                    _ => {}
                }
            }
            371 => self.r[rd] = (self.steps & 0xffff_ffff) as u32,
            83 => self.r[rd] = 0,
            19 => {
                let mut v = 0u32;
                for i in 0..8 {
                    v = (v << 4) | self.cr[i] as u32;
                }
                self.r[rd] = v;
            }
            144 => {
                let crm = (w >> 12) & 0xff;
                let v = self.r[rd];
                for i in 0..8 {
                    if crm & (0x80 >> i) != 0 {
                        self.cr[i] = ((v >> (28 - 4 * i)) & 0xf) as u8;
                    }
                }
            }
            // cache / sync instructions
            598 | 854 | 86 | 54 | 246 | 278 | 470 | 982 | 306 | 566 | 1014 | 146 | 210 => {
                if xo == 1014 {
                    // dcbz: zero the 32-byte block
                    let a = ea(self) & !31;
                    mem.fill(a, 32, 0);
                }
            }
            _ => return Err(format!("op31 xo {xo} (word {w:08x}) at {pc:#x}")),
        }
        Ok(())
    }

    fn fp_ops(&mut self, mem: &mut Mem, w: u32) -> Result<(), String> {
        let _ = mem;
        let pc = self.pc;
        let op = w >> 26;
        let rd = ((w >> 21) & 31) as usize;
        let fa = ((w >> 16) & 31) as usize;
        let fb = ((w >> 11) & 31) as usize;
        let fc = ((w >> 6) & 31) as usize;
        let single = op == 59;
        let fin = |x: f64| if single { round_single(x) } else { x };
        let xo5 = (w >> 1) & 31;
        let (a, b, c) = (self.f[fa], self.f[fb], self.f[fc]);
        let set = |s: &mut Cpu, v: f64| {
            s.f[rd] = v;
            if single {
                s.ps1[rd] = v;
            }
        };
        match xo5 {
            18 => set(self, fin(a / b)),
            20 => set(self, fin(a - b)),
            21 => set(self, fin(a + b)),
            25 => set(self, fin(a * c)),
            28 => set(self, fin(a.mul_add(c, -b))),
            29 => set(self, fin(a.mul_add(c, b))),
            30 => set(self, fin(-(a.mul_add(c, -b)))),
            31 => set(self, fin(-(a.mul_add(c, b)))),
            24 if single => set(self, round_single(1.0 / b)),
            26 if !single => {
                // frsqrte
                set(self, 1.0 / b.sqrt());
            }
            23 if !single => {
                // fsel
                set(self, if a >= 0. { c } else { b });
            }
            24 if !single => set(self, round_single(1.0 / b)),
            _ if single => return Err(format!("op59 xo {xo5} at {pc:#x}")),
            _ => {
                let xo = (w >> 1) & 0x3ff;
                match xo {
                    0 | 32 => self.fcmp(rd >> 2, a, b),
                    12 => set(self, round_single(b)),
                    14 | 15 => {
                        let v = if xo == 15 { b.trunc() } else { (b + 0.0).round_ties_even() };
                        let i = if v.is_nan() { i32::MIN } else { v.max(i32::MIN as f64).min(i32::MAX as f64) as i32 };
                        self.f[rd] = f64::from_bits(0xFFF8_0000_0000_0000 | (i as u32 as u64));
                    }
                    40 => self.f[rd] = -b,
                    72 => self.f[rd] = b,
                    264 => self.f[rd] = b.abs(),
                    136 => self.f[rd] = -(b.abs()),
                    583 => self.f[rd] = f64::from_bits(self.fpscr as u64),
                    711 => {
                        // mtfsf: only the FPSCR image kept
                        self.fpscr = self.f[fb].to_bits() as u32;
                    }
                    70 | 38 | 134 | 64 => {}
                    _ => return Err(format!("op63 xo {xo} (word {w:08x}) at {pc:#x}")),
                }
            }
        }
        Ok(())
    }

    fn paired(&mut self, mem: &mut Mem, w: u32) -> Result<(), String> {
        let pc = self.pc;
        let rd = ((w >> 21) & 31) as usize;
        let ra = ((w >> 16) & 31) as usize;
        let rb = ((w >> 11) & 31) as usize;
        let rc = ((w >> 6) & 31) as usize;
        let xo10 = (w >> 1) & 0x3ff;
        let xo5 = (w >> 1) & 31;
        let (a0, a1, b0, b1, c0, c1) = (self.f[ra], self.ps1[ra], self.f[rb], self.ps1[rb], self.f[rc], self.ps1[rc]);
        let s = round_single;
        let set = |x: &mut Cpu, v0: f64, v1: f64| {
            x.f[rd] = v0;
            x.ps1[rd] = v1;
        };
        match xo5 {
            10 => {
                // ps_sum0
                let (r0, r1) = (a0 + b1, c1);
                set(self, s(r0), s(r1));
                return Ok(());
            }
            11 => {
                // ps_sum1
                let (r0, r1) = (c0, a0 + b1);
                set(self, s(r0), s(r1));
                return Ok(());
            }
            12 => {
                set(self, s(a0 * c0), s(a1 * c0));
                return Ok(());
            }
            13 => {
                set(self, s(a0 * c1), s(a1 * c1));
                return Ok(());
            }
            14 => {
                set(self, s(a0 * c0 + b0), s(a1 * c0 + b1));
                return Ok(());
            }
            15 => {
                set(self, s(a0 * c1 + b0), s(a1 * c1 + b1));
                return Ok(());
            }
            18 => {
                set(self, s(a0 / b0), s(a1 / b1));
                return Ok(());
            }
            20 => {
                set(self, s(a0 - b0), s(a1 - b1));
                return Ok(());
            }
            21 => {
                set(self, s(a0 + b0), s(a1 + b1));
                return Ok(());
            }
            23 => {
                // ps_sel
                set(self, if a0 >= 0. { c0 } else { b0 }, if a1 >= 0. { c1 } else { b1 });
                return Ok(());
            }
            25 => {
                set(self, s(a0 * c0), s(a1 * c1));
                return Ok(());
            }
            28 => {
                set(self, s(a0 * c0 - b0), s(a1 * c1 - b1));
                return Ok(());
            }
            29 => {
                set(self, s(a0 * c0 + b0), s(a1 * c1 + b1));
                return Ok(());
            }
            30 => {
                set(self, s(-(a0 * c0 - b0)), s(-(a1 * c1 - b1)));
                return Ok(());
            }
            31 => {
                set(self, s(-(a0 * c0 + b0)), s(-(a1 * c1 + b1)));
                return Ok(());
            }
            _ => {}
        }
        match xo10 {
            40 => set(self, -b0, -b1),
            72 => set(self, b0, b1),
            264 => set(self, b0.abs(), b1.abs()),
            136 => set(self, -(b0.abs()), -(b1.abs())),
            528 => set(self, a0, b0),
            560 => set(self, a0, b1),
            592 => set(self, a1, b0),
            624 => set(self, a1, b1),
            24 => set(self, s(1.0 / b0), s(1.0 / b1)),
            0 => self.fcmp(rd >> 2, a0, b0),
            32 => self.fcmp(rd >> 2, a0, b0),
            38 | 6 | 7 | 39 => {
                // psq_lx / psq_stx / psq_lux / psq_stux
                let i = ((w >> 7) & 7) as usize;
                let two = (w >> 10) & 1 == 0;
                let a = (if ra == 0 { 0 } else { self.r[ra] }).wrapping_add(self.r[rb]);
                let g = self.gqr[i];
                if xo10 == 6 || xo10 == 38 {
                    let (ty, sc) = ((g >> 16) & 7, ((((g >> 24) & 63) << 26) as i32 >> 26));
                    let (v0, v1, _) = self.quant_load(mem, a, ty, sc, two);
                    self.f[rd] = v0;
                    self.ps1[rd] = v1;
                } else {
                    let (ty, sc) = (g & 7, ((((g >> 8) & 63) << 26) as i32 >> 26));
                    let (x, y) = (self.f[rd], self.ps1[rd]);
                    self.quant_store(mem, a, ty, sc, two, x, y);
                }
                if xo10 == 38 || xo10 == 39 {
                    self.r[ra] = a;
                }
            }
            _ => return Err(format!("op4 xo {xo10} (word {w:08x}) at {pc:#x}")),
        }
        Ok(())
    }
}
