//! EA BIGF/BIG4/VIV archives, Nintendo U8 (`.arc`) and RefPack decompression - Rust port of
//! Remaster/src/archives.py + containers.py + research.read_virtual.  Verified against the SHA-256 of all
//! 5,267 records in the corpus inventory (see tests).
use std::path::Path;

pub fn is_refpack(d:&[u8])->bool{d.len()>=2&&d[1]==0xFB&&d[0]&0x3E==0x10}

/// RefPack decompression (bounded; errors instead of panicking on malformed input).
pub fn decompress(data:&[u8])->Result<Vec<u8>,String>{
    if !is_refpack(data){return Ok(data.to_vec())}
    let width=if data[0]&0x80!=0{4}else{3};
    let mut pos=2+if data[0]&1!=0{width}else{0};
    if pos+width>data.len(){return Err("Truncated RefPack header".into())}
    let expected=data[pos..pos+width].iter().fold(0usize,|a,&b|(a<<8)|b as usize);pos+=width;
    if expected>512*1024*1024{return Err("RefPack output exceeds the 512 MiB safety limit".into())}
    let mut out:Vec<u8>=Vec::with_capacity(expected);
    let take=|pos:&mut usize,n:usize|->Result<&[u8],String>{if *pos+n>data.len(){return Err("Truncated RefPack stream".into())}let s=&data[*pos..*pos+n];*pos+=n;Ok(s)};
    loop{
        let c=take(&mut pos,1)?[0] as usize;
        let (literals,count,distance);
        if c>=0xFC{literals=c&3;count=0;distance=0}
        else if c>=0xE0{literals=((c&31)+1)*4;count=0;distance=0}
        else if c>=0xC0{let t=take(&mut pos,3)?;let (a,b,d)=(t[0] as usize,t[1] as usize,t[2] as usize);literals=c&3;count=((c&12)<<6)+d+5;distance=((c&16)<<12)+(a<<8)+b+1}
        else if c>=0x80{let t=take(&mut pos,2)?;let (a,b)=(t[0] as usize,t[1] as usize);literals=a>>6;count=(c&63)+4;distance=((a&63)<<8)+b+1}
        else{let a=take(&mut pos,1)?[0] as usize;literals=c&3;count=((c&28)>>2)+3;distance=((c&96)<<3)+a+1}
        if out.len()+literals+count>expected{return Err("RefPack output exceeds its declared size".into())}
        out.extend_from_slice(take(&mut pos,literals)?);
        if count>0{
            if distance>out.len(){return Err("Invalid RefPack back-reference".into())}
            for _ in 0..count{let b=out[out.len()-distance];out.push(b);}
        }
        if c>=0xFC{break}
    }
    if out.len()!=expected{return Err(format!("RefPack size mismatch: {} / {expected}",out.len()))}
    Ok(out)
}

#[derive(Debug,Clone)] pub struct Entry{pub index:usize,pub name:String,pub offset:usize,pub size:usize}

fn be32(d:&[u8],o:usize)->Result<u32,String>{d.get(o..o+4).map(|b|u32::from_be_bytes(b.try_into().unwrap())).ok_or_else(||format!("read past end at {o:#x}"))}
fn cstr(d:&[u8],o:usize,end:usize)->Result<(String,usize),String>{
    let s=d.get(o..end).ok_or("string range")?;let e=s.iter().position(|&c|c==0).ok_or("Unterminated string")?;
    Ok((String::from_utf8_lossy(&s[..e]).into_owned(),o+e+1))
}

/// BIGF/BIG4 directory: [magic][size LE][count BE][dir end BE] then (offset,size,name\0)*.
pub fn big_entries(d:&[u8])->Result<Vec<Entry>,String>{
    if d.len()<16||!(&d[..4]==b"BIGF"||&d[..4]==b"BIG4"){return Err("Not BIGF/BIG4".into())}
    let (count,end)=(be32(d,8)? as usize,be32(d,12)? as usize);
    if end<16||end>d.len()||count>(end-16)/9{return Err("Invalid BIG directory".into())}
    let mut pos=16;let mut out=vec![];
    for i in 0..count{
        let (offset,size)=(be32(d,pos)? as usize,be32(d,pos+4)? as usize);pos+=8;
        let (name,next)=cstr(d,pos,end)?;pos=next;
        if offset<end{return Err("BIG entry overlaps directory".into())}
        out.push(Entry{index:i,name,offset,size});
    }
    Ok(out)
}

/// Nintendo U8 (`.arc`) archives.
pub fn u8_entries(d:&[u8])->Result<Vec<Entry>,String>{
    if d.len()<16||&d[..4]!=b"U\xaa8-"{return Err("Not Nintendo U8".into())}
    let (root,header_size,data_start)=(be32(d,4)? as usize,be32(d,8)? as usize,be32(d,12)? as usize);
    let (kind,_parent,count)=(be32(d,root)?,be32(d,root+4)?,be32(d,root+8)? as usize);
    if kind>>24!=1||count<1||count>header_size/12{return Err("Invalid U8 root".into())}
    let names=root+count*12;let mut stack:Vec<(String,usize,usize)>=vec![(String::new(),count,0)];let mut out=vec![];
    for i in 1..count{
        while stack.last().is_some_and(|s|i>=s.1){stack.pop();}
        let top=stack.last().ok_or("Invalid U8 directory extent")?.clone();
        let (word,offset,size)=(be32(d,root+i*12)?,be32(d,root+i*12+4)? as usize,be32(d,root+i*12+8)? as usize);
        let (name,_)=cstr(d,names+(word as usize&0xffffff),root+header_size)?;let full=format!("{}{}",top.0,name);
        match word>>24{
            1=>{if !(i<size&&size<=top.1)||offset!=top.2{return Err("Invalid U8 parent or subtree".into())}stack.push((format!("{full}/"),size,i));}
            0=>{if offset<data_start{return Err("U8 file precedes data".into())}out.push(Entry{index:i,name:full,offset,size});}
            _=>return Err("Unknown U8 node type".into()),
        }
    }
    Ok(out)
}

pub fn entries(d:&[u8])->Option<Result<Vec<Entry>,String>>{
    if d.len()>=4&&(&d[..4]==b"BIGF"||&d[..4]==b"BIG4"){Some(big_entries(d))}else if d.len()>=4&&&d[..4]==b"U\xaa8-"{Some(u8_entries(d))}else{None}
}

/// Read `outer.big::inner.viv::file.o` without extracting siblings.  `.bh` files are external directories, not containers.
pub fn read_virtual(source:&str)->Result<(Vec<u8>,String),String>{
    let mut parts=source.split("::");
    let first=parts.next().ok_or("empty source")?;
    let mut data=decompress(&std::fs::read(first).map_err(|e|format!("{first}: {e}"))?)?;
    let mut name=Path::new(first).file_name().map(|n|n.to_string_lossy().into_owned()).unwrap_or_default();
    for member in parts{
        let table=if name.to_lowercase().ends_with(".bh"){None}else{entries(&data)};
        let table=table.ok_or_else(||format!("{name} is not a supported container"))??;
        let want=member.replace('\\',"/");
        let matches:Vec<&Entry>=table.iter().filter(|e|e.name.replace('\\',"/")==want).collect();
        if matches.len()!=1{return Err(format!("Expected one entry named {member:?}; found {}",matches.len()))}
        let e=matches[0];let slice=data.get(e.offset..e.offset+e.size).ok_or("entry outside archive")?;
        data=decompress(slice)?;name=e.name.clone();
    }
    Ok((data,name))
}

/// Recursively visit every file below a loose file (archives are expanded), calling `f(virtual_path, decompressed_bytes)`.
pub fn walk_bytes(source:&str,data:&[u8],name:&str,f:&mut dyn FnMut(&str,&[u8])){
    f(source,data);
    if name.to_lowercase().ends_with(".bh"){return}
    if let Some(Ok(table))=entries(data){
        for e in table{
            let Some(slice)=data.get(e.offset..e.offset+e.size) else{continue};
            let Ok(inner)=decompress(slice) else{continue};
            walk_bytes(&format!("{source}::{}",e.name),&inner,&e.name,f);
        }
    }
}

#[cfg(test)]
mod tests{
    use super::*;
    #[test]
    fn refpack_literal_stop_code(){
        // 0x10FB header, 3-byte size 3, stop code 0xFF (literals = 3) then 3 literal bytes.
        let d=[0x10,0xFB,0,0,3,0xFF,b'a',b'b',b'c'];
        assert_eq!(decompress(&d).unwrap(),b"abc");
        assert_eq!(decompress(b"plain").unwrap(),b"plain");
    }
    #[test]
    fn rejects_garbage(){assert!(big_entries(b"BIGF").is_err());assert!(u8_entries(b"nope").is_err());}
    /// Every record of the recorded corpus inventory must decode to identical bytes (SHA-256) when data is present.
    #[test]
    fn matches_recorded_corpus_hashes(){
        let cov=std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../Remaster/research/coverage.json");
        let Ok(txt)=std::fs::read_to_string(&cov) else{eprintln!("coverage.json absent; skipped");return};
        let j:serde_json::Value=serde_json::from_str(&txt).unwrap();let recs=j["records"].as_array().unwrap();
        if !std::path::Path::new(recs[0]["source"].as_str().unwrap()).exists(){eprintln!("DATA absent; skipped");return}
        let (mut ok,mut bad)=(0,vec![]);
        // Group by outer file so each archive is read once.
        let mut by_outer:std::collections::BTreeMap<String,Vec<(&str,&str)>>=Default::default();
        for r in recs{let s=r["source"].as_str().unwrap();by_outer.entry(s.split("::").next().unwrap().to_string()).or_default().push((s,r["sha256"].as_str().unwrap()));}
        for (outer,list) in by_outer{
            let Ok(bytes)=std::fs::read(&outer) else{bad.push(format!("cannot read {outer}"));continue};
            let Ok(data)=decompress(&bytes) else{bad.push(format!("decompress {outer}"));continue};
            let name=Path::new(&outer).file_name().unwrap().to_string_lossy().into_owned();
            let want:std::collections::HashMap<&str,&str>=list.into_iter().collect();
            let mut seen=std::collections::HashSet::new();
            walk_bytes(&outer,&data,&name,&mut |vp,d|{
                if let Some(h)=want.get(vp){seen.insert(vp.to_string());if crate::sha256::hex(d)==**h{ok+=1}else{bad.push(format!("hash mismatch {vp}"))}}
            });
            for v in want.keys(){if !seen.contains(*v){bad.push(format!("not produced: {v}"))}}
        }
        assert!(bad.is_empty(),"{} ok, {} bad; first: {:?}",ok,bad.len(),&bad[..bad.len().min(5)]);
        eprintln!("archive corpus: {ok} records verified against recorded SHA-256");
        assert!(ok>=5000,"only {ok} records verified");
    }
}
