//! The game's file system (`FILE_*` / `FILESYS_*`): loose files below the DATA tree plus mounted `.big` archives.
use super::{hooks::R, MgHost};
use crate::{archive, gekko::Vm};

type V = Vm<MgHost>;

#[derive(Default)]
pub struct Vfs {
    mounts: Vec<Mount>,
    handles: Vec<Option<Vec<u8>>>,
}

struct Mount {
    path: String,
    data: Vec<u8>,
    table: Vec<archive::Entry>,
}

fn norm(name: &str) -> String {
    let n = name.replace('\\', "/").to_lowercase();
    n.trim_start_matches('/').trim_start_matches("data/").to_string()
}

/// Resolve a game path against the extracted DATA tree.
pub fn resolve_game_path(path: &str) -> Option<std::path::PathBuf> {
    let p = path.replace('\\', "/");
    let p = p.trim_start_matches('/').trim_start_matches("data/").to_string();
    let root = crate::bridge::data_root().join("files");
    [root.join("data").join(&p), root.join(&p), root.join("data").join(p.to_lowercase())].into_iter().find(|c| c.is_file())
}

impl Vfs {
    /// Mount an archive file; returns its handle (index + 1).
    pub fn mount(&mut self, path: &str) -> Option<usize> {
        let file = resolve_game_path(path)?;
        let raw = std::fs::read(&file).ok()?;
        let data = archive::decompress(&raw).ok()?;
        let table = archive::big_entries(&data).ok()?;
        self.mounts.push(Mount { path: path.to_string(), data, table });
        Some(self.mounts.len())
    }
    pub fn unmount(&mut self, handle: usize) {
        if let Some(m) = self.mounts.get_mut(handle.wrapping_sub(1)) {
            m.data.clear();
            m.table.clear();
        }
    }

    /// Stored (possibly compressed) bytes of `name`.
    pub fn read(&self, name: &str) -> Option<Vec<u8>> {
        if let Some(p) = resolve_game_path(name) {
            return std::fs::read(p).ok();
        }
        let want = norm(name);
        for m in self.mounts.iter().rev() {
            for e in &m.table {
                let en = norm(&e.name);
                if en == want || en.ends_with(&format!("/{want}")) || want.ends_with(&format!("/{en}")) {
                    return m.data.get(e.offset..e.offset + e.size).map(|s| s.to_vec());
                }
            }
        }
        None
    }
    pub fn size(&self, name: &str) -> Option<usize> {
        if let Some(p) = resolve_game_path(name) {
            return std::fs::metadata(p).ok().map(|m| m.len() as usize);
        }
        self.read(name).map(|d| d.len())
    }
}

fn name_arg(vm: &mut V, i: usize) -> String {
    let p = vm.a(i);
    vm.st.mem.cstr(p, 260)
}

/// `FILE_exists(name)`
fn file_exists(h: &mut MgHost, vm: &mut V) -> R {
    let n = name_arg(vm, 0);
    vm.ret(h.vfs.size(&n).is_some() as u32);
    Ok(())
}
/// `FILE_size(name)`: stored size, 0 when the file does not exist.
fn file_size(h: &mut MgHost, vm: &mut V) -> R {
    let n = name_arg(vm, 0);
    let s = h.vfs.size(&n);
    if s.is_none() {
        h.log.push(format!("FILE_size: missing {n}"));
    }
    vm.ret(s.unwrap_or(0) as u32);
    Ok(())
}
/// `FILE_sizez(name)`: size after RefPack decompression.
fn file_sizez(h: &mut MgHost, vm: &mut V) -> R {
    let n = name_arg(vm, 0);
    let s = h.vfs.read(&n).and_then(|d| archive::decompress(&d).ok()).map(|d| d.len());
    vm.ret(s.unwrap_or(0) as u32);
    Ok(())
}
/// `FILE_loadat(name, dst, size)`: copy the stored bytes to `dst`; returns the byte count.
fn file_loadat(h: &mut MgHost, vm: &mut V) -> R {
    let n = name_arg(vm, 0);
    let (dst, cap) = (vm.a(1), vm.a(2) as usize);
    match h.vfs.read(&n) {
        Some(d) => {
            let k = d.len().min(cap.max(d.len()));
            vm.st.mem.write(dst, &d[..k]);
            vm.ret(k as u32);
        }
        None => {
            h.log.push(format!("FILE_loadat: missing {n}"));
            vm.ret(0);
        }
    }
    Ok(())
}
/// `FILE_loadatz(name, dst, size)`: decompress while loading.
fn file_loadatz(h: &mut MgHost, vm: &mut V) -> R {
    let n = name_arg(vm, 0);
    let dst = vm.a(1);
    match h.vfs.read(&n).and_then(|d| archive::decompress(&d).ok()) {
        Some(d) => {
            vm.st.mem.write(dst, &d);
            vm.ret(d.len() as u32);
        }
        None => vm.ret(0),
    }
    Ok(())
}
/// `FILE_loadsize(name, int* size_out, flags)`: the file in a fresh buffer; returns the buffer (0 when missing).
fn file_loadsize(h: &mut MgHost, vm: &mut V) -> R {
    let n = name_arg(vm, 0);
    let out = vm.a(1);
    match h.vfs.read(&n) {
        Some(d) => {
            let p = vm.alloc(d.len() as u32 + 4, 32);
            vm.st.mem.write(p, &d);
            if out != 0 {
                vm.w32(out, d.len() as u32);
            }
            vm.ret(p);
        }
        None => {
            h.log.push(format!("FILE_loadsize: missing {n}"));
            if out != 0 {
                vm.w32(out, 0);
            }
            vm.ret(0);
        }
    }
    Ok(())
}
fn file_loadsizez(h: &mut MgHost, vm: &mut V) -> R {
    let n = name_arg(vm, 0);
    let out = vm.a(1);
    match h.vfs.read(&n).and_then(|d| archive::decompress(&d).ok()) {
        Some(d) => {
            let p = vm.alloc(d.len() as u32 + 4, 32);
            vm.st.mem.write(p, &d);
            if out != 0 {
                vm.w32(out, d.len() as u32);
            }
            vm.ret(p);
        }
        None => {
            if out != 0 {
                vm.w32(out, 0);
            }
            vm.ret(0);
        }
    }
    Ok(())
}
/// `FILESYS_opensync(name, mode, priority, int* handle)`: read access only; success flag in r3.
fn opensync(h: &mut MgHost, vm: &mut V) -> R {
    let n = name_arg(vm, 0);
    let out = vm.a(3);
    match h.vfs.read(&n) {
        Some(d) => {
            h.vfs.handles.push(Some(d));
            if out != 0 {
                vm.w32(out, 0x100 + h.vfs.handles.len() as u32);
            }
            vm.ret(1);
        }
        None => {
            h.log.push(format!("FILESYS_opensync: missing {n}"));
            if out != 0 {
                vm.w32(out, 0);
            }
            vm.ret(0);
        }
    }
    Ok(())
}
fn handle_data(h: &MgHost, handle: u32) -> Option<&Vec<u8>> {
    h.vfs.handles.get((handle as usize).checked_sub(0x101)?)?.as_ref()
}
/// `FILESYS_readsync(handle, offset, buf, length, priority)`: bytes transferred.
fn readsync(h: &mut MgHost, vm: &mut V) -> R {
    let (handle, off, buf, len) = (vm.a(0), vm.a(1) as usize, vm.a(2), vm.a(3) as usize);
    let chunk = handle_data(h, handle).map(|d| d.get(off..).map(|s| s[..len.min(s.len())].to_vec()).unwrap_or_default());
    match chunk {
        Some(c) => {
            vm.st.mem.write(buf, &c);
            vm.ret(c.len() as u32);
        }
        None => vm.ret(0),
    }
    Ok(())
}
fn closesync(h: &mut MgHost, vm: &mut V) -> R {
    let handle = vm.a(0);
    if let Some(i) = (handle as usize).checked_sub(0x101) {
        if let Some(slot) = h.vfs.handles.get_mut(i) {
            *slot = None;
        }
    }
    vm.ret(1);
    Ok(())
}
fn sizesync(h: &mut MgHost, vm: &mut V) -> R {
    let n = handle_data(h, vm.a(0)).map(|d| d.len()).unwrap_or(0);
    vm.ret(n as u32);
    Ok(())
}
fn existssync(h: &mut MgHost, vm: &mut V) -> R {
    let n = name_arg(vm, 0);
    vm.ret(h.vfs.size(&n).is_some() as u32);
    Ok(())
}
/// `FILESYS_addbigsync(path, flags, priority, int* handle)`: success flag in r3.
fn addbig(h: &mut MgHost, vm: &mut V) -> R {
    let n = name_arg(vm, 0);
    let out = vm.a(3);
    match h.vfs.mount(&n) {
        Some(handle) => {
            h.log.push(format!("mounted {n} as {handle}"));
            if out != 0 {
                vm.w32(out, handle as u32);
            }
            vm.ret(1);
        }
        None => {
            h.log.push(format!("FILESYS_addbigsync: cannot mount {n}"));
            if out != 0 {
                vm.w32(out, 0);
            }
            vm.ret(0);
        }
    }
    Ok(())
}
fn delbig(h: &mut MgHost, vm: &mut V) -> R {
    h.vfs.unmount(vm.a(0) as usize);
    vm.ret(1);
    Ok(())
}

pub fn install(vm: &mut V) {
    for (n, f) in [
        ("FILE_exists", file_exists as fn(&mut MgHost, &mut V) -> R),
        ("FILE_size", file_size),
        ("FILE_sizez", file_sizez),
        ("FILE_loadat", file_loadat),
        ("FILE_loadsize", file_loadsize),
        ("FILE_loadsizez", file_loadsizez),
        ("FILE_loadatz", file_loadatz),
        ("FILESYS_addbigsync", addbig),
        ("FILESYS_opensync", opensync),
        ("FILESYS_readsync", readsync),
        ("FILESYS_closesync", closesync),
        ("FILESYS_sizesync", sizesync),
        ("FILESYS_existssync", existssync),
        ("FILESYS_delbigsync", delbig),
    ] {
        if !vm.hook(n, f) {
            vm.log_missing_symbol(n);
        }
    }
}
