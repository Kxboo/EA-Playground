//! Loader for the pinned `playgroundz.elf` (ELF32, big-endian, fixed addresses, with a symbol table).
use super::mem::Mem;
use std::{collections::HashMap, path::PathBuf};

pub struct Symbol {
    pub name: String,
    pub addr: u32,
    pub size: u32,
    pub is_func: bool,
}

pub struct Image {
    pub symbols: Vec<Symbol>,
    pub by_name: HashMap<String, usize>,
    /// Function symbols sorted by address (index into `symbols`).
    pub funcs: Vec<usize>,
    pub text: (u32, u32),
}

fn be32(d: &[u8], o: usize) -> u32 {
    u32::from_be_bytes(d[o..o + 4].try_into().unwrap())
}
fn be16(d: &[u8], o: usize) -> u16 {
    u16::from_be_bytes(d[o..o + 2].try_into().unwrap())
}

pub fn find() -> Option<PathBuf> {
    let root = crate::bridge::root();
    [root.join("Remaster/reference/playgroundz.elf"), root.join("../Remaster/reference/playgroundz.elf")].into_iter().find(|p| p.exists())
}

impl Image {
    /// Load every allocated section into `mem` and read the symbol table.
    pub fn load(path: &std::path::Path, mem: &mut Mem) -> Result<Image, String> {
        let d = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
        if d.len() < 52 || &d[0..4] != b"\x7fELF" {
            return Err("not an ELF".into());
        }
        let shoff = be32(&d, 32) as usize;
        let shentsize = be16(&d, 46) as usize;
        let shnum = be16(&d, 48) as usize;
        let mut sections = vec![];
        for i in 0..shnum {
            let o = shoff + i * shentsize;
            sections.push((be32(&d, o + 4), be32(&d, o + 8), be32(&d, o + 12), be32(&d, o + 16) as usize, be32(&d, o + 20) as usize, be32(&d, o + 24) as usize));
        }
        let mut text = (u32::MAX, 0);
        let mut symtab = None;
        for &(typ, flags, addr, off, size, link) in sections.iter() {
            if typ == 2 {
                symtab = Some((off, size, link));
            }
            if flags & 2 != 0 && addr != 0 && size > 0 {
                if typ == 1 {
                    mem.write(addr, &d[off..off + size]);
                    if flags & 4 != 0 {
                        text.0 = text.0.min(addr);
                        text.1 = text.1.max(addr + size as u32);
                    }
                } else if typ == 8 {
                    mem.fill(addr, size, 0);
                }
            }
        }
        let (so, ss, link) = symtab.ok_or("no symbol table")?;
        let stro = sections[link].3;
        let mut symbols = vec![];
        for k in 0..ss / 16 {
            let o = so + k * 16;
            let name_off = be32(&d, o) as usize;
            let addr = be32(&d, o + 4);
            let size = be32(&d, o + 8);
            let info = d[o + 12];
            let start = stro + name_off;
            let end = d[start..].iter().position(|&b| b == 0).map(|p| start + p).unwrap_or(start);
            let name = String::from_utf8_lossy(&d[start..end]).into_owned();
            if name.is_empty() || addr == 0 {
                continue;
            }
            symbols.push(Symbol { name, addr, size, is_func: info & 0xf == 2 });
        }
        let mut by_name = HashMap::new();
        for (i, s) in symbols.iter().enumerate() {
            by_name.entry(s.name.clone()).or_insert(i);
        }
        let mut funcs: Vec<usize> = symbols.iter().enumerate().filter(|(_, s)| s.is_func && s.size > 0).map(|(i, _)| i).collect();
        funcs.sort_by_key(|&i| symbols[i].addr);
        Ok(Image { symbols, by_name, funcs, text })
    }

    pub fn addr(&self, name: &str) -> Option<u32> {
        self.by_name.get(name).map(|&i| self.symbols[i].addr)
    }

    /// The function symbol containing `addr`.
    pub fn func_at(&self, addr: u32) -> Option<&Symbol> {
        let p = self.funcs.partition_point(|&i| self.symbols[i].addr <= addr);
        if p == 0 {
            return None;
        }
        let s = &self.symbols[self.funcs[p - 1]];
        (addr < s.addr + s.size).then_some(s)
    }
}
