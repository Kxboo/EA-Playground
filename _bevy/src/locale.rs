//! Localisation tables (`locale/*.loc` + `string.idx`) - the game's `TRCLocale` (0x8024d144..0x8024d5b8).
//!
//! `.loc`: `LOCH` header (20 bytes, little-endian fields) then a `LOCL` chunk: size, 0, string count, then `count`
//! offsets (relative to the `LOCL` chunk) to NUL-terminated UTF-16LE strings (`LoadLocaleFile` / `LOCALE_getstr`).
//! `string.idx`: version 0x20040519, entry count, then `count` big-endian (hash, string index) pairs sorted by hash.
//! `FindStringIndex` hashes the key with `ComputeHash` (h = -1; h = h*33 + (signed char)c) and `bsearch`es the table.
use crate::skeleton::{be32,le32};

pub fn compute_hash(key:&str)->u32{
    // `ComputeHash__9TRCLocaleFPCc` 0x8024d500: r5 = -1; per byte: r5 = r5 + (r5 << 5) + (signed)byte.
    key.bytes().fold(0xFFFF_FFFFu32,|h,c|h.wrapping_add(h<<5).wrapping_add(c as i8 as i32 as u32))
}

pub struct Locale{pub strings:Vec<String>,index:Vec<(u32,u32)>}

impl Locale{
    pub fn parse(loc:&[u8],idx:&[u8])->Result<Self,String>{
        if loc.get(..4)!=Some(b"LOCH"){return Err("Not a LOCH file".into())}
        let chunk=le32(loc,16)? as usize;
        if loc.get(chunk..chunk+4)!=Some(b"LOCL"){return Err("LOCL chunk missing".into())}
        let (size,count)=(le32(loc,chunk+4)? as usize,le32(loc,chunk+12)? as usize);
        if chunk+size>loc.len()||count>size/4{return Err("LOCL table exceeds file".into())}
        let mut strings=vec![];
        for i in 0..count{
            let off=chunk+le32(loc,chunk+16+i*4)? as usize;
            let bytes=loc.get(off..).ok_or("string offset outside file")?;
            let units:Vec<u16>=bytes.chunks_exact(2).map(|c|u16::from_le_bytes([c[0],c[1]])).take_while(|&u|u!=0).collect();
            strings.push(String::from_utf16(&units).map_err(|_|format!("string {i} is not valid UTF-16"))?);
        }
        if be32(idx,0)?!=0x2004_0519{return Err("Unknown string.idx version".into())}
        let n=be32(idx,4)? as usize;
        if 8+n*8>idx.len(){return Err("string.idx table exceeds file".into())}
        let index:Vec<(u32,u32)>=(0..n).map(|i|Ok((be32(idx,8+i*8)?,be32(idx,12+i*8)?))).collect::<Result<_,String>>()?;
        if index.windows(2).any(|w|w[0].0>=w[1].0){return Err("string.idx is not sorted by hash".into())}
        if index.iter().any(|&(_,i)|i as usize>=strings.len()){return Err("string.idx references a missing string".into())}
        Ok(Self{strings,index})
    }
    /// `TRCLocale::FindStringIndex` + `LOCALE_getstr`.
    pub fn get(&self,key:&str)->Option<&str>{self.get_hash(compute_hash(key))}
    /// `Locale::GetString(int)` (0x803ae8c4): lookup by the precomputed key hash (the id stored in conversation files).
    pub fn get_hash(&self,h:u32)->Option<&str>{
        self.index.binary_search_by_key(&h,|e|e.0).ok().map(|i|self.strings[self.index[i].1 as usize].as_str())
    }
    pub fn len(&self)->usize{self.index.len()}
    /// (key hash, string index) pairs of `string.idx`, sorted by hash.
    pub fn index(&self)->&[(u32,u32)]{&self.index}
}

#[cfg(test)]
mod tests{
    use super::*;
    #[test] fn hash_matches_the_recovered_routine(){
        // Hand-evaluated from the disassembly: empty string returns the seed; one char c gives -1*33 + c.
        assert_eq!(compute_hash(""),0xFFFF_FFFF);
        assert_eq!(compute_hash("A"),(0xFFFF_FFFFu32.wrapping_mul(33)).wrapping_add(65));
    }
    /// Keys that appear as strings in the executable resolve to sensible text in every shipped locale.
    #[test] fn shipped_locales_decode_and_known_keys_resolve(){
        let root=crate::bridge::data_root().join("files").join("data").join("locale");
        if !root.join("string.idx").exists(){eprintln!("DATA absent; skipped");return}
        let mut n=0;
        // `locale/` (1283 UI strings) and `locale/trc/` (system-error strings, its own index).
        for (dir,expect) in [(root.clone(),1283usize),(root.join("trc"),64)]{
            let idx=std::fs::read(dir.join("string.idx")).unwrap();
            for e in std::fs::read_dir(&dir).unwrap().filter_map(|e|e.ok()){
                let p=e.path();if p.extension().is_none_or(|x|x!="loc"){continue}
                let l=Locale::parse(&std::fs::read(&p).unwrap(),&idx).unwrap_or_else(|e|panic!("{}: {e}",p.display()));
                assert_eq!(l.len(),expect,"{}",p.display());n+=1;
                if p.file_name().unwrap()=="eng_us.loc"&&expect==1283{
                    assert_eq!(l.get("T_RotationsToWin"),Some("ROTATIONS TO WIN"));assert_eq!(l.get("T_RC_Title"),Some("REPORT CARD"));assert_eq!(l.get("B_Woods"),Some("WOODS"));
                }
            }
        }
        assert!(n>=10,"only {n} locales");
        eprintln!("{n} locale files decoded");
    }
}
