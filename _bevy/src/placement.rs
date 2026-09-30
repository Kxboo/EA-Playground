//! Level placement data: marker sets (`.mkr`) and RC-track checkpoint lanes (`.cpt`).
//!
//! `.mkr` (`AssetManager::ProcessMarkerSet` 0x803cc530): big-endian u32 count, then `count` x 0x4c-byte markers:
//! u8 type + 3 pad bytes, u32 (per-marker value A), u32 (value B), rmMatrix4 (16 big-endian floats, row-major,
//! translation in the last row) - `DSObjectManager::Populate` reads the position from that row.
//!
//! `.cpt` (`AssetManager::ProcessCheckpointSet` 0x803cc66c): u32 `n`, `n` x 0x64-byte records (skipped by the loader,
//! empty in every shipped track), u32 lane count, then per lane: u32 point count, u32 lane index, count x (x,y,z) f32.
use crate::skeleton::be32;

#[derive(Debug,Clone)]
pub struct Marker{pub kind:u8,pub a:u32,pub b:u32,pub matrix:[f32;16]}
impl Marker{pub fn position(&self)->[f32;3]{[self.matrix[12],self.matrix[13],self.matrix[14]]}}

fn bef(d:&[u8],o:usize)->Result<f32,String>{Ok(f32::from_bits(be32(d,o)?))}

pub fn parse_markers(d:&[u8])->Result<Vec<Marker>,String>{
    let n=be32(d,0)? as usize;
    if 4+n.checked_mul(0x4c).ok_or("marker count overflow")?!=d.len(){return Err(format!("marker file is {} bytes, expected {} for {n} markers",d.len(),4+n*0x4c))}
    (0..n).map(|i|{
        let o=4+i*0x4c;let mut m=[0f32;16];for k in 0..16{m[k]=bef(d,o+12+k*4)?;}
        Ok(Marker{kind:d[o],a:be32(d,o+4)?,b:be32(d,o+8)?,matrix:m})
    }).collect()
}

#[derive(Debug,Clone)] pub struct Lane{pub index:u32,pub points:Vec<[f32;3]>}
#[derive(Debug,Clone)] pub struct Checkpoints{pub records:usize,pub lanes:Vec<Lane>}

pub fn parse_checkpoints(d:&[u8])->Result<Checkpoints,String>{
    if d.len()==8&&d.iter().all(|&b|b==0){return Ok(Checkpoints{records:0,lanes:vec![]})} // placeholder files of the shadow tracks
    let records=be32(d,0)? as usize;let mut p=4+records.checked_mul(0x64).ok_or("record count overflow")?;
    let n=be32(d,p)? as usize;p+=4;let mut lanes=vec![];
    for _ in 0..n{
        let (count,index)=(be32(d,p)? as usize,be32(d,p+4)?);p+=8;
        if p+count.checked_mul(12).ok_or("point count overflow")?>d.len(){return Err("lane points exceed file".into())}
        let points=(0..count).map(|i|Ok([bef(d,p+i*12)?,bef(d,p+i*12+4)?,bef(d,p+i*12+8)?])).collect::<Result<Vec<_>,String>>()?;
        p+=count*12;lanes.push(Lane{index,points});
    }
    if p!=d.len(){return Err(format!("{} trailing bytes after the last lane",d.len()-p))}
    Ok(Checkpoints{records,lanes})
}

#[cfg(test)]
mod tests{
    use super::*;
    #[test] fn rejects_wrong_sizes(){assert!(parse_markers(&[0,0,0,1]).is_err());assert!(parse_checkpoints(&[0,0,0,0,0,0,0,9]).is_err())}
    /// Every shipped marker and checkpoint file parses to exactly its length and holds sane geometry.
    #[test] fn all_shipped_files_decode(){
        let cov=std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../Remaster/research/coverage.json");
        let Ok(txt)=std::fs::read_to_string(&cov) else{eprintln!("coverage.json absent; skipped");return};
        let j:serde_json::Value=serde_json::from_str(&txt).unwrap();let recs=j["records"].as_array().unwrap();
        if !std::path::Path::new(recs[0]["source"].as_str().unwrap()).exists(){eprintln!("DATA absent; skipped");return}
        let (mut mk,mut markers,mut cp,mut lanes,mut points)=(0,0,0,0,0);
        let mut kinds=std::collections::BTreeMap::<u8,usize>::new();
        for r in recs{
            let ext=r["extension"].as_str().unwrap();let src=r["source"].as_str().unwrap();
            if ext==".mkr"{
                let (d,_)=crate::archive::read_virtual(src).unwrap();let m=parse_markers(&d).unwrap_or_else(|e|panic!("{src}: {e}"));
                for x in &m{
                    *kinds.entry(x.kind).or_default()+=1;
                    assert!(x.matrix.iter().all(|v|v.is_finite()),"{src}: non-finite matrix");
                    assert!((x.matrix[15]-1.).abs()<1e-4,"{src}: matrix is not homogeneous ({})",x.matrix[15]);
                }
                mk+=1;markers+=m.len();
            }else if ext==".cpt"{
                let (d,_)=crate::archive::read_virtual(src).unwrap();let c=parse_checkpoints(&d).unwrap_or_else(|e|panic!("{src}: {e}"));
                for l in &c.lanes{for p in &l.points{assert!(p.iter().all(|v|v.is_finite()&&v.abs()<10_000.),"{src}: bad lane point {p:?}")}}
                cp+=1;lanes+=c.lanes.len();points+=c.lanes.iter().map(|l|l.points.len()).sum::<usize>();
            }
        }
        eprintln!("{mk} marker files / {markers} markers (types {kinds:?}); {cp} checkpoint files / {lanes} lanes / {points} points");
        assert_eq!(mk,49);assert_eq!(cp,10);
    }
}
