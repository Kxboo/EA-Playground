//! Guest address space of the emulated GameCube / Wii process: MEM1 at 0x80000000 and MEM2 at 0x90000000.
//! All accesses are big-endian.  An out-of-range access records a fault (checked by the interpreter loop) and reads as 0.

pub const MEM1_BASE: u32 = 0x8000_0000;
pub const MEM1_SIZE: usize = 0x0200_0000;
pub const MEM2_BASE: u32 = 0x9000_0000;
pub const MEM2_SIZE: usize = 0x0400_0000;

pub struct Mem {
    mem1: Vec<u8>,
    mem2: Vec<u8>,
    pub fault: Option<String>,
    /// Development aid (`EAGL_PPC_WATCH=<hex address>`): writes touching this address are noted for the run loop.
    pub watch: Option<u32>,
    pub watch_hit: Option<(u32, Vec<u8>)>,
}

impl Default for Mem {
    fn default() -> Self {
        Mem { mem1: vec![0; MEM1_SIZE], mem2: vec![0; MEM2_SIZE], fault: None, watch: std::env::var("EAGL_PPC_WATCH").ok().and_then(|v| u32::from_str_radix(v.trim_start_matches("0x"), 16).ok()), watch_hit: None }
    }
}

impl Mem {
    fn region(&self, addr: u32, len: usize) -> Option<&[u8]> {
        if addr >= MEM1_BASE && (addr - MEM1_BASE) as usize + len <= MEM1_SIZE {
            let o = (addr - MEM1_BASE) as usize;
            Some(&self.mem1[o..o + len])
        } else if addr >= MEM2_BASE && (addr - MEM2_BASE) as usize + len <= MEM2_SIZE {
            let o = (addr - MEM2_BASE) as usize;
            Some(&self.mem2[o..o + len])
        } else {
            None
        }
    }
    fn region_mut(&mut self, addr: u32, len: usize) -> Option<&mut [u8]> {
        if addr >= MEM1_BASE && (addr - MEM1_BASE) as usize + len <= MEM1_SIZE {
            let o = (addr - MEM1_BASE) as usize;
            Some(&mut self.mem1[o..o + len])
        } else if addr >= MEM2_BASE && (addr - MEM2_BASE) as usize + len <= MEM2_SIZE {
            let o = (addr - MEM2_BASE) as usize;
            Some(&mut self.mem2[o..o + len])
        } else {
            None
        }
    }
    fn bad(&mut self, what: &str, addr: u32) {
        if self.fault.is_none() {
            self.fault = Some(format!("{what} at {addr:#010x}"));
        }
    }

    pub fn read(&mut self, addr: u32, len: usize) -> Vec<u8> {
        match self.region(addr, len) {
            Some(s) => s.to_vec(),
            None => {
                self.bad("bad read", addr);
                vec![0; len]
            }
        }
    }
    pub fn write(&mut self, addr: u32, data: &[u8]) {
        if let Some(w) = self.watch {
            if w >= addr && w < addr + data.len() as u32 {
                self.watch_hit = Some((addr, data.to_vec()));
            }
        }
        match self.region_mut(addr, data.len()) {
            Some(s) => s.copy_from_slice(data),
            None => self.bad("bad write", addr),
        }
    }
    pub fn r8(&mut self, a: u32) -> u8 {
        match self.region(a, 1) {
            Some(s) => s[0],
            None => {
                self.bad("bad read8", a);
                0
            }
        }
    }
    pub fn r16(&mut self, a: u32) -> u16 {
        match self.region(a, 2) {
            Some(s) => u16::from_be_bytes([s[0], s[1]]),
            None => {
                self.bad("bad read16", a);
                0
            }
        }
    }
    pub fn r32(&mut self, a: u32) -> u32 {
        match self.region(a, 4) {
            Some(s) => u32::from_be_bytes([s[0], s[1], s[2], s[3]]),
            None => {
                self.bad("bad read32", a);
                0
            }
        }
    }
    pub fn r64(&mut self, a: u32) -> u64 {
        match self.region(a, 8) {
            Some(s) => u64::from_be_bytes(s.try_into().unwrap()),
            None => {
                self.bad("bad read64", a);
                0
            }
        }
    }
    pub fn w8(&mut self, a: u32, v: u8) {
        self.write(a, &[v]);
    }
    pub fn w16(&mut self, a: u32, v: u16) {
        self.write(a, &v.to_be_bytes());
    }
    pub fn w32(&mut self, a: u32, v: u32) {
        self.write(a, &v.to_be_bytes());
    }
    pub fn w64(&mut self, a: u32, v: u64) {
        self.write(a, &v.to_be_bytes());
    }
    pub fn rf32(&mut self, a: u32) -> f32 {
        f32::from_bits(self.r32(a))
    }
    pub fn wf32(&mut self, a: u32, v: f32) {
        self.w32(a, v.to_bits());
    }
    /// NUL-terminated string (lossy), at most `max` bytes.
    pub fn cstr(&mut self, a: u32, max: usize) -> String {
        let mut out = vec![];
        for i in 0..max as u32 {
            let b = self.r8(a + i);
            if b == 0 {
                break;
            }
            out.push(b);
        }
        String::from_utf8_lossy(&out).into_owned()
    }
    pub fn fill(&mut self, a: u32, len: usize, byte: u8) {
        match self.region_mut(a, len) {
            Some(s) => s.fill(byte),
            None => self.bad("bad fill", a),
        }
    }
    pub fn copy(&mut self, dst: u32, src: u32, len: usize) {
        let data = self.read(src, len);
        self.write(dst, &data);
    }
}
