//! EAGLAnim animation banks (`.anm`) - Rust port of Remaster/src/legacy/{decoder,exporter}/*.
//!
//! A bank is a little-endian ELF container (`__AnimationBank:::` symbol) holding a table of clip blocks.  Every clip
//! block points (self-relocated pointers) at up to two track markers whose tag selects one of the engine's codec
//! classes (`SetAnimMemoryMap`/`EvalSQT` of `FnDeltaQFast`, `FnDeltaF3`, `FnDeltaF1`, `FnDeltaSingleQ`,
//! `FnStatelessQ`, `FnStatelessF3`).  All codecs store fixed-size blocks: a reference sample per channel followed by
//! (2^shift - 1) samples of small deltas that are accumulated forward.  Channel to bone tables address the engine's
//! 0x30-byte SQT pose record (`bone * 12 + word`), and `sBoneMask` (recovered.rs `BONE_MASK`) gates translation writes.
//!
//! All arithmetic is done in f64 in the same operation order as the reference decoders so the differential test can
//! compare against the Python output.
//!
//! Corrections from the executable's evaluators (the Python reference shares the old behaviour):
//! * `FnStatelessF3` samples are signed 16-bit times the channel scale (`EvalSQTfast` 0x803fdc1c), not offset binary.
//! * `FnStatelessQ`/`F3` extra static channels and sparse keyframe time tables (`EvalSQT` 0x803fd220) are decoded;
//!   sparse clips are resampled per frame with the evaluator's component-wise interpolation.
//! * `FnCompoundChannel` (container 0x0f, 0x803ff53c) applies every child track, lower index last; the reference only
//!   read the first two and missed 125 player F1 tracks and both prop animations.
use crate::recovered::BONE_MASK;
use crate::skeleton::{be16,be32,bef32,le32,Container,Skeleton};
use std::collections::{BTreeMap,HashMap};

pub const MAX_BONES:usize=68;
pub const CLIP_FPS:f64=30.;
pub type Samples<T>=Vec<Option<T>>;

#[derive(Debug,Clone,Copy)]
pub struct Marker{pub abs:usize,pub tag:u8,pub family:u16,pub dense_ptr:Option<usize>}
#[derive(Debug,Clone)]
pub struct ClipBlock{pub index:usize,pub rel:usize,pub abs:usize,pub container_tag:u8,pub family:u16,pub primary:Option<Marker>,pub secondary:Option<Marker>,pub whole_clip:bool}

pub struct Bank{data:Vec<u8>,reloc:HashMap<usize,usize>,data_start:usize,pub blocks:Vec<ClipBlock>,pub names:Vec<String>}

#[derive(Debug,Clone)]
pub struct Clip{pub index:usize,pub name:String,pub sample_count:usize,pub native_timing:Option<crate::animation_function::CompoundFunction>,pub codec:&'static str,
    pub rot:BTreeMap<usize,Samples<[f64;4]>>,pub trans:BTreeMap<usize,Samples<[f64;3]>>,pub scale:BTreeMap<usize,Samples<[f64;3]>>,pub caveats:Vec<String>}
impl Clip{pub fn sample_rate(&self)->f64{self.native_timing.as_ref().map_or(CLIP_FPS,|t|t.fps as f64)}
    pub fn duration(&self)->f64{let count=self.native_timing.as_ref().map_or(self.sample_count,|t|t.samples as usize);(count.saturating_sub(1)) as f64/self.sample_rate()}}

fn rd_be16(d:&[u8],o:usize)->Result<usize,String>{Ok(be16(d,o)? as usize)}
fn byte(d:&[u8],o:usize)->Result<u8,String>{d.get(o).copied().ok_or_else(||format!("read past end at {o:#x}"))}

fn parse_marker(d:&[u8],reloc:&HashMap<usize,usize>,data_start:usize,rel:usize)->Option<Marker>{parse_marker_in(d,reloc,data_start,rel,None)}
/// `container_family`: a compound child carries its container's family (the bank's target checksum).
fn parse_marker_in(d:&[u8],reloc:&HashMap<usize,usize>,data_start:usize,rel:usize,container_family:Option<u16>)->Option<Marker>{
    let abs=data_start+rel;
    if abs+0x12>d.len()||d[abs]!=0{return None}
    let (tag,family)=(d[abs+1],u16::from_be_bytes([d[abs+2],d[abs+3]]));
    // Real markers only use these family suffixes (0x1414 is the RC-car container/markers) or their container's.
    if !matches!(family,0xc6e4|0x0606|0xe580|0xe664|0x1414)&&Some(family)!=container_family{return None}
    if !(0x11..=0x17).contains(&tag){return None}
    Some(Marker{abs,tag,family,dense_ptr:reloc.get(&(rel+12)).copied()})
}

impl Bank{
    /// Compound map +0xa is length; +4 relocates the sorted AttributeBlock.
    /// Attribute ID 1 supplies FPS (native UseFPS/GetLength/GetAttributes).
    pub fn compound_function(&self,index:usize)->Result<crate::animation_function::CompoundFunction,String>{
        let cb=self.blocks.get(index).ok_or("compound clip index outside bank")?;
        if cb.container_tag!=0x0f{return Err("clip is not FnCompoundChannel".into())}
        let mut attributes=vec![];
        if let Some(rel)=self.reloc.get(&(cb.rel+4)){
            let p=self.data_start+rel;
            let count=be32(&self.data,p)? as usize;
            if count>65536{return Err("attribute block count exceeds ID domain".into())}
            for i in 0..count{let entry=p+4+i*8;attributes.push((be16(&self.data,entry)?,byte(&self.data,entry+4)?));}
            if attributes.windows(2).any(|a|a[0].0>a[1].0){return Err("attribute block is not sorted".into())}
        }
        // Explicit fresh timing instance; native allocator defaults are not inferred.
        let mut function=crate::animation_function::CompoundFunction{samples:be16(&self.data,cb.abs+10)?,use_fps:false,fps:0};
        function.set_use_fps(true,&attributes);
        Ok(function)
    }
    pub fn parse(data:Vec<u8>)->Result<Self,String>{
        let (reloc,data_start,data_size,bank_off)={
            let c=Container::parse(&data)?;
            let sym=c.symbols()?.into_iter().find(|(n,_)|n.starts_with("__AnimationBank:::")).ok_or("no bank header found")?;
            (c.self_relocations(),c.data_start,c.data_size,c.data_start+sym.1 as usize)
        };
        if bank_off+24>data.len(){return Err("bank header outside file".into())}
        let (clips,table_a,table_b)=(be32(&data,bank_off+4)? as usize,le32(&data,bank_off+16)? as usize,le32(&data,bank_off+20)? as usize);
        if clips>100_000{return Err("implausible clip count".into())}
        let data_end=data_start+data_size;
        let mut names=vec![];
        for i in 0..clips{
            let rel=*reloc.get(&(table_b+i*4)).ok_or_else(||format!("Clip {i}: unresolved name-table pointer"))?;
            if rel>=data_size{return Err(format!("Clip {i}: unresolved name-table pointer"))}
            let off=data_start+rel;let end=data[off..data_end.min(data.len())].iter().position(|&c|c==0).ok_or_else(||format!("Clip {i}: unterminated name"))?;
            names.push(String::from_utf8_lossy(&data[off..off+end]).into_owned());
        }
        let mut blocks=vec![];
        for i in 0..clips{
            let rel=*reloc.get(&(table_a+i*4)).ok_or_else(||format!("Clip {i}: unresolved clip-table pointer"))?;
            let abs=data_start+rel;
            if abs+0x20>data.len(){return Err(format!("Clip {i}: invalid block"))}
            let (container_tag,family)=(data[abs+1],u16::from_be_bytes([data[abs+2],data[abs+3]]));
            let whole_clip=container_tag==0x16;
            let primary=reloc.get(&(rel+0x0C)).and_then(|&p|parse_marker(&data,&reloc,data_start,p));
            let secondary=reloc.get(&(rel+0x10)).and_then(|&p|parse_marker(&data,&reloc,data_start,p));
            blocks.push(ClipBlock{index:i,rel,abs,container_tag,family,primary,secondary,whole_clip});
        }
        Ok(Self{data,reloc,data_start,blocks,names})
    }
}

// ---------------------------------------------------------------------------------------------------------------
// codecs
// ---------------------------------------------------------------------------------------------------------------

struct Coded<T>{sample_count:usize,channel_count:usize,frames:Vec<Samples<T>>}

/// Common block geometry: `block_size = round_down_even(ref_region + (2^shift-1)*delta_region + 1)`.
struct Blocks{block_len:usize,n_deltas:usize,ref_region:usize,block_size:usize,n_blocks:usize}
fn blocks(sample_count:usize,channel_count:usize,shift:u8,ref_bytes:usize,delta_bytes:usize)->Result<Blocks,String>{
    if shift>16{return Err(format!("implausible block shift {shift}"))}
    let block_len=1usize<<shift;let n_deltas=block_len-1;
    let ref_region=channel_count*ref_bytes;let raw=ref_region+n_deltas*channel_count*delta_bytes+1;
    Ok(Blocks{block_len,n_deltas,ref_region,block_size:raw-raw%2,n_blocks:sample_count.div_ceil(block_len)})
}
fn deltas_this(b:&Blocks,blk:usize,sample_count:usize)->usize{
    if blk==b.n_blocks-1{((sample_count as i64-1-(blk*b.block_len) as i64).max(0) as usize).min(b.n_deltas)}else{b.n_deltas}
}
fn finish<T:Clone>(mut by_channel:Vec<Vec<T>>,sample_count:usize)->Vec<Samples<T>>{
    by_channel.iter_mut().map(|c|(0..sample_count).map(|s|c.get(s).cloned()).collect()).collect()
}

/// `FnDeltaQFast` (0x12): 12-bit reference quaternions + 6-bit deltas with per-channel pivot/scale.
fn delta_qfast(d:&[u8],m:&Marker)->Result<Coded<[f64;4]>,String>{
    let a=m.abs;let sample_count=rd_be16(d,a+4)?;let channel_count=byte(d,a+6)? as usize;let shift=byte(d,a+0x10)?;
    let payload=a+0x12;let stream_start=payload+channel_count*16;
    let mut pivots=vec![];let mut scales=vec![];
    for ch in 0..channel_count{
        let o=payload+ch*16;let mut u=[0f64;8];for k in 0..8{u[k]=be16(d,o+k*2)? as f64;}
        pivots.push([u[0]*(2.0/65535.0)-1.0,u[1]*(2.0/65535.0)-1.0,u[2]*(2.0/65535.0)-1.0,u[3]*(2.0/65535.0)-1.0]);
        scales.push([u[4]*(2.0/65535.0),u[5]*(2.0/65535.0),u[6]*(2.0/65535.0),u[7]*(2.0/65535.0)]);
    }
    let b=blocks(sample_count,channel_count,shift,6,3)?;
    let mut by=vec![Vec::new();channel_count];
    let mut off=stream_start;
    for blk in 0..b.n_blocks{
        if off+b.block_size>d.len(){break}
        let dn=deltas_this(&b,blk,sample_count);
        for ch in 0..channel_count{
            let ro=off+ch*6;if ro+6>d.len(){continue}
            let (x,y,z)=(be16(d,ro)?,be16(d,ro+2)?,be16(d,ro+4)?);
            let raw=[((x>>4)&0xfff) as f64,((y>>4)&0xfff) as f64,((z>>4)&0xfff) as f64,((((x&0xf)<<8)|((y&0xf)<<4)|(z&0xf))) as f64];
            let mut qs=[0f64;4];for k in 0..4{qs[k]=raw[k]*(2.0/4095.0)-1.0;}
            by[ch].push(qs);
            for s in 0..dn{
                let so=off+b.ref_region+s*channel_count*3+ch*3;if so+3>d.len(){break}
                let (b0,b1,b2)=(d[so],d[so+1],d[so+2]);
                let r=[((b0>>2)&0x3f) as f64,((b1>>2)&0x3f) as f64,((b2>>2)&0x3f) as f64,((((b0&3)<<4)|((b1&3)<<2)|(b2&3))) as f64];
                let (p,sc)=(&pivots[ch],&scales[ch]);
                let mut n=qs;for k in 0..4{n[k]=qs[k]+(p[k]+r[k]*(1.0/63.0)*sc[k]);}
                by[ch].push(n);qs=n;
            }
        }
        off+=b.block_size;
    }
    Ok(Coded{sample_count,channel_count,frames:finish(by,sample_count)})
}

/// `FnDeltaF3` (0x14) and `FnDeltaF1` (0x15): per-channel float basis, 16-bit references, 8-bit deltas accumulated
/// across blocks (the per-block reference only matters for random access).  `N` is the component count (3 or 1).
fn delta_float<const N:usize>(d:&[u8],m:&Marker)->Result<Coded<[f64;N]>,String>{
    let a=m.abs;let sample_count=rd_be16(d,a+0x0c)?;let channel_count=rd_be16(d,a+0x0e)?;let shift=byte(d,a+0x10)?;
    let stride=if N==3{0x24}else{0x0c};let table=a+0x14;let stream_start=table+channel_count*stride;
    // basis: (ref_base, ref_scale, delta_base, delta_scale) per axis
    let mut basis=vec![];
    for ch in 0..channel_count{
        let o=table+ch*stride;let mut ax=[[0f64;4];N];
        for i in 0..N{
            let (e,bb,w0,w1)=if N==3{(bef32(d,o+i*4)?,bef32(d,o+0x0c+i*4)?,be16(d,o+0x18+i*2)? as f64,be16(d,o+0x1e+i*2)? as f64)}
                else{(bef32(d,o)?,bef32(d,o+4)?,be16(d,o+8)? as f64,be16(d,o+10)? as f64)};
            let pivot=w0*(2.0*(1.0/65535.0))-1.0;let scale_w1=w1*(2.0*(1.0/65535.0));
            ax[i]=[e,bb*(1.0/65534.0),bb*pivot,bb*scale_w1*(1.0/255.0)];
        }
        basis.push(ax);
    }
    let b=blocks(sample_count,channel_count,shift,2*N,N)?;
    let mut by:Vec<Vec<[f64;N]>>=vec![Vec::new();channel_count];
    let mut carry:Vec<Option<[f64;N]>>=vec![None;channel_count];
    let mut off=stream_start;
    for blk in 0..b.n_blocks{
        if off+b.block_size>d.len(){break}
        let dn=deltas_this(&b,blk,sample_count);
        for ch in 0..channel_count{
            let ro=off+ch*2*N;if ro+2*N>d.len(){continue}
            let mut qs=[0f64;N];for i in 0..N{qs[i]=basis[ch][i][0]+be16(d,ro+i*2)? as f64*basis[ch][i][1];}
            if let Some(c)=carry[ch]{qs=c}
            by[ch].push(qs);
            for s in 0..dn{
                let so=off+b.ref_region+s*channel_count*N+ch*N;if so+N>d.len(){break}
                let mut n=qs;for i in 0..N{n[i]=qs[i]+(basis[ch][i][2]+d[so+i] as f64*basis[ch][i][3]);}
                by[ch].push(n);qs=n;
            }
            carry[ch]=Some(qs);
        }
        off+=b.block_size;
    }
    Ok(Coded{sample_count,channel_count,frames:finish(by,sample_count)})
}

fn euler_to_quat(a0:f64,a1:f64,a2:f64)->[f64;4]{
    let (h0,h1,h2)=(a0*0.5,a1*0.5,a2*0.5);
    let (sx,sy,sz)=(h0.sin(),h1.sin(),h2.sin());let (cx,cy,cz)=(h0.cos(),h1.cos(),h2.cos());
    [cx*cy*sz-sx*sy*cz,sx*cy*cz+cx*sy*sz,cx*sy*cz-sx*cy*sz,cx*cy*cz+sx*sy*sz]
}
fn hamilton(a:[f64;4],b:[f64;4])->[f64;4]{
    let ([ax,ay,az,aw],[bx,by,bz,bw])=(a,b);
    [aw*bx+ax*bw+ay*bz-az*by,aw*by-ax*bz+ay*bw+az*bx,aw*bz+ax*by-ay*bx+az*bw,aw*bw-ax*bx-ay*by-az*bz]
}

/// `FnDeltaSingleQ` (0x13): one rotation angle + one scalar per channel, rebuilt into a quaternion through
/// `EulerToQuat` (0x8040873c) and two cached bind quaternions.
fn delta_singleq(d:&[u8],m:&Marker)->Result<Coded<[f64;4]>,String>{
    let a=m.abs;let sample_count=rd_be16(d,a+4)?;let channel_count=byte(d,a+6)? as usize;let shift=byte(d,a+7)?;
    // GetArrays__DeltaSingleQ: MinRange table at map+0x10, 14-byte records.
    let payload=a+0x10;const TWO_PI_OVER:f64=2.0*std::f64::consts::PI*(1.0/65535.0);
    struct Rec{flag:usize,db1:f64,db2:f64,ds1:f64,ds2:f64,q_a:[f64;4],q_b:[f64;4]}
    let mut recs=vec![];
    for ch in 0..channel_count{
        let o=payload+ch*14;let u=|k:usize|be16(d,o+k*2).map(|v|v as f64);
        let raw_flag=byte(d,o+0x0c)?;let flag=if raw_flag==0||raw_flag==1{raw_flag as usize}else{2};
        let (a0,a1)=(u(0)?*TWO_PI_OVER-std::f64::consts::PI,u(1)?*TWO_PI_OVER-std::f64::consts::PI);
        let mut v0=[0.;3];v0[flag]=a0;let mut v1=[0.;3];v1[flag]=a1;
        recs.push(Rec{flag,db1:u(2)?*(1.0/32768.0)-1.0,db2:u(3)?*(1.0/32768.0)-1.0,ds1:u(4)?*(1.0/32768.0),ds2:u(5)?*(1.0/32768.0),q_a:euler_to_quat(v0[0],v0[1],v0[2]),q_b:euler_to_quat(v1[0],v1[1],v1[2])});
    }
    let stream_start=payload+channel_count*14;
    let b=blocks(sample_count,channel_count,shift,2,1)?;
    let mut by=vec![Vec::new();channel_count];
    let emit=|r:&Rec,acc:&[f64;4]|->[f64;4]{match r.flag{0=>hamilton(*acc,r.q_b),1=>hamilton(hamilton(*acc,r.q_a),r.q_b),_=>hamilton(r.q_a,*acc)}};
    let mut off=stream_start;
    for blk in 0..b.n_blocks{
        if off+b.block_size>d.len(){break}
        let dn=deltas_this(&b,blk,sample_count);
        let mut acc=vec![[0f64;4];channel_count];
        for ch in 0..channel_count{
            let r=&recs[ch];acc[ch][r.flag]=byte(d,off+ch*2)? as f64*(2.0*(1.0/255.0))-1.0;acc[ch][3]=byte(d,off+ch*2+1)? as f64*(2.0*(1.0/255.0))-1.0;
        }
        for ch in 0..channel_count{by[ch].push(emit(&recs[ch],&acc[ch]));}
        for s in 0..dn{
            for ch in 0..channel_count{
                let so=off+b.ref_region+s*channel_count+ch;if so>=d.len(){break}
                let r=&recs[ch];let raw=d[so];let (hi,lo)=((raw>>4) as f64,(raw&0xf) as f64);
                acc[ch][r.flag]+=hi*(r.ds1/15.0)+r.db1;acc[ch][3]+=lo*(r.ds2/15.0)+r.db2;
                by[ch].push(emit(r,&acc[ch]));
            }
        }
        off+=b.block_size;
    }
    Ok(Coded{sample_count,channel_count,frames:finish(by,sample_count)})
}

/// `unpack_u16_trick`: `rlwinm r3,u16,15,2,16; rlwimi r3,u16,16,0,0` reinterpreted as an IEEE float.
fn unpack_u16(v:u16)->f64{
    let v=v as u32;
    let mut r3=v.rotate_left(15)&mask(2,16);
    let rot2=v.rotate_left(16);
    r3=(r3&!0x8000_0000)|(rot2&0x8000_0000);
    f32::from_bits(r3) as f64
}
fn mask(mb:u32,me:u32)->u32{(mb..=me).fold(0,|m,b|m|1<<(31-b))}

struct Stateless<T>{keyframes:usize,bone_table:Option<Vec<Option<usize>>>,frames:Vec<Vec<T>>,
    /// Extra static channels (constant values) with their bone slots, and the sparse keyframe time table (u16 frame of keyframe k+1).
    statics:Vec<(Option<usize>,T)>,times:Option<Vec<u16>>}

/// `FnStatelessQ` (0x16): the clip block itself is the map; 4 x u16 quaternion fields per keyframe/channel.
fn stateless_q(d:&[u8],reloc:&HashMap<usize,usize>,ds:usize,cb:&ClipBlock)->Result<Stateless<[f64;4]>,String>{
    let a=cb.abs;let keyframes=rd_be16(d,a+0x14)?;let channels=byte(d,a+0x16)? as usize;let extra=byte(d,a+0x17)? as usize;
    let base=a+0x18;
    if base+keyframes*channels*8>d.len(){return Err("record table runs past EOF".into())}
    let tbl=reloc.get(&(a-ds+0x0c)).map(|&r|ds+r).filter(|&t|t+channels+extra<=d.len());
    let table=tbl.map(|t|d[t..t+channels].iter().map(|&b|Some(b as usize)).collect());
    let mut frames=vec![];
    for ch in 0..channels{
        let mut v=vec![];
        for k in 0..keyframes{
            let f=|i:usize|be16(d,base+(k*channels+ch)*8+i*2).map(unpack_u16);
            v.push([f(0)?,f(1)?,f(2)?,f(3)?]);
        }
        frames.push(v);
    }
    // `FnStatelessQ::EvalSQT` (0x803fd220): `extra` constant quaternions follow the keyframe records; their bones continue
    // the channel table.  +0x08 points at the sparse time table when keyframes are not one per frame.
    let sbase=base+keyframes*channels*8;let mut statics=vec![];
    for i in 0..extra{let f=|k:usize|be16(d,sbase+8*i+2*k).map(unpack_u16);statics.push((tbl.map(|t|d[t+channels+i] as usize),[f(0)?,f(1)?,f(2)?,f(3)?]))}
    let times=reloc.get(&(a-ds+0x08)).map(|&r|ds+r).map(|t|(0..keyframes.saturating_sub(1)).map(|k|be16(d,t+2*k)).collect::<Result<Vec<_>,_>>()).transpose()?;
    Ok(Stateless{keyframes,bone_table:table,frames,statics,times})
}

/// `FnStatelessF3` (0x17): per-channel scale basis, 3 x offset-binary u16 per keyframe/channel.
fn stateless_f3(d:&[u8],reloc:&HashMap<usize,usize>,ds:usize,m:&Marker)->Result<Stateless<[f64;3]>,String>{
    let a=m.abs;let keyframes=rd_be16(d,a+0x10)?;let channels=byte(d,a+0x12)? as usize;let extra=byte(d,a+0x13)? as usize;
    let basis=a+0x18;let kf_table=basis+channels*0x20;let stride=channels*6;
    if kf_table+keyframes*stride>d.len(){return Err("keyframe table runs past EOF".into())}
    let tbl=reloc.get(&(a-ds+0x0c)).map(|&r|ds+r).filter(|&t|t+(channels+extra)*2<=d.len());
    let slot=|t:usize,i:usize|{let e=be16(d,t+i*2).unwrap() as i64-8;if e>=0&&e%12==0{Some((e/12) as usize)}else{None}};
    let table=tbl.map(|t|(0..channels).map(|i|slot(t,i)).collect::<Vec<_>>());
    let mut scales=vec![];
    for ch in 0..channels{let o=basis+ch*0x20+0x10;scales.push([bef32(d,o)?,bef32(d,o+4)?,bef32(d,o+8)?]);}
    let mut frames=vec![];
    for ch in 0..channels{
        let mut v=vec![];
        for k in 0..keyframes{
            let o=kf_table+k*stride+ch*6;let s=&scales[ch];
            // `FnStatelessF3::EvalSQTfast` (0x803fdc1c) / `EvalSQT` (0x803fe43c): `lha` (signed 16-bit) times the
            // channel scale at basis+0x10; no offset is applied.
            let f=|i:usize|be16(d,o+i*2).map(|r|(r as i16) as f64*s[i]);
            v.push([f(0)?,f(1)?,f(2)?]);
        }
        frames.push(v);
    }
    // Extra constant translations: three f32 each, 4-byte aligned after the keyframe table (`EvalSQTfast`).
    let sbase=(kf_table+keyframes*stride+3)&!3;let mut statics=vec![];
    for i in 0..extra{statics.push((tbl.and_then(|t|slot(t,channels+i)),[bef32(d,sbase+12*i)?,bef32(d,sbase+12*i+4)?,bef32(d,sbase+12*i+8)?]))}
    let times=reloc.get(&(a-ds+0x08)).map(|&r|ds+r).map(|t|(0..keyframes.saturating_sub(1)).map(|k|be16(d,t+2*k)).collect::<Result<Vec<_>,_>>()).transpose()?;
    Ok(Stateless{keyframes,bone_table:table,frames,statics,times})
}

/// Sparse keyframes -> one sample per frame, as `FnStatelessQ::EvalSQT` evaluates them: keyframe 0 at frame 0, keyframe
/// k+1 at `times[k]`; between keys each component is interpolated linearly (no renormalisation).
fn resample<const N:usize>(keys:&[[f64;N]],times:&Option<Vec<u16>>)->Vec<[f64;N]>{
    let Some(t)=times else{return keys.to_vec()};
    if keys.is_empty(){return vec![]}
    let kt:Vec<f64>=std::iter::once(0.).chain(t.iter().map(|&x|x as f64)).collect();
    let last=*kt.last().unwrap() as usize;
    (0..=last).map(|f|{
        let f=f as f64;let k=kt.iter().rposition(|&x|x<=f).unwrap_or(0).min(keys.len()-1);
        if k+1>=keys.len()||kt[k+1]==kt[k]{return keys[k]}
        let a=(f-kt[k])/(kt[k+1]-kt[k]);std::array::from_fn(|i|keys[k][i]+a*(keys[k+1][i]-keys[k][i]))
    }).collect()
}

// ---------------------------------------------------------------------------------------------------------------
// channel -> bone tables (shared 0x30-byte SQT pose record: word 0-2 scale, 4-7 rotation, 8-10 translation)
// ---------------------------------------------------------------------------------------------------------------

#[derive(PartialEq,Debug,Clone,Copy)]
enum Field{Scale,Pad,Quat,Trans}
fn pose_field(raw:usize)->(usize,Field,usize){
    let (bone,rem)=(raw/12,raw%12);
    match rem{0..=2=>(bone,Field::Scale,rem),3=>(bone,Field::Pad,0),4..=7=>(bone,Field::Quat,rem-4),8..=10=>(bone,Field::Trans,rem-8),_=>(bone,Field::Pad,0)}
}

type Rot=BTreeMap<usize,Samples<[f64;4]>>;type Vec3s=BTreeMap<usize,Samples<[f64;3]>>;

impl Bank{
    /// `u8` entries at marker+0x0C, bone index directly.
    fn qfast_bones(&self,m:&Marker,channels:usize)->Option<Vec<usize>>{
        let abs=self.data_start+m.dense_ptr?;
        (abs+channels<=self.data.len()).then(||self.data[abs..abs+channels].iter().map(|&b|b as usize).collect())
    }
    /// FnDeltaF3 / FnStatelessF3: u16 entries at marker+4; only translation-slot entries carry a bone, scale-slot
    /// entries are routed to scale animation, anything else is skipped with a caveat.
    fn vector_bones(&self,m:&Marker,channels:usize)->Option<(Vec<Option<usize>>,Vec<String>,BTreeMap<usize,usize>)>{
        let p=*self.reloc.get(&(m.abs-self.data_start+4))?;let abs=self.data_start+p;
        if abs+channels*2>self.data.len(){return None}
        let (mut out,mut notes,mut scale)=(vec![],vec![],BTreeMap::new());
        for i in 0..channels{
            let raw=be16(&self.data,abs+i*2).ok()? as usize;let (bone,field,axis)=pose_field(raw);
            match field{
                Field::Scale=>{out.push(None);scale.insert(i,bone);notes.push(format!("channel {i}: bone {bone} animated SCALE (exported as scale channel)"));}
                Field::Trans=>out.push(Some(bone)),
                _=>{out.push(None);notes.push(format!("channel {i}: addresses bone {bone}'s {field:?}.{axis} field, not translation -- skipped (not yet supported)"));}
            }
        }
        Some((out,notes,scale))
    }
    /// FnDeltaF1: single-axis codec, the axis really selects X/Y/Z; any non-translation entry rejects the table.
    fn scalar_bones(&self,m:&Marker,channels:usize)->Option<Vec<(usize,usize)>>{
        let p=*self.reloc.get(&(m.abs-self.data_start+4))?;let abs=self.data_start+p;
        if abs+channels*2>self.data.len(){return None}
        (0..channels).map(|i|{let (bone,field,axis)=pose_field(be16(&self.data,abs+i*2).ok()? as usize);(field==Field::Trans&&bone<MAX_BONES).then_some((bone,axis))}).collect()
    }

    /// `FnCompoundChannel` (container tag 0x0f, `EvalSQT` 0x803ff53c): u16 child count at +8, child pointers from +0xc.
    /// Children are evaluated from the last to the first into one pose, so a lower index overwrites what a higher one
    /// wrote for the same pose words.  Each child is decoded with its own codec.
    fn compound(&self,skel:&Skeleton,cb:&ClipBlock,name:String)->Result<Clip,String>{
        let d=&self.data[..];let n_bones=skel.bones.len();
        let n=be16(d,cb.abs+8)? as usize;if n>16{return Err(format!("compound channel count {n}"))}
        let kids:Vec<Marker>=(0..n).map(|i|self.reloc.get(&(cb.rel+0x0c+4*i)).and_then(|&p|parse_marker_in(d,&self.reloc,self.data_start,p,Some(cb.family))).ok_or_else(||format!("compound child {i} unresolved"))).collect::<Result<_,_>>()?;
        let (mut rot,mut trans,mut scale)=(Rot::new(),Vec3s::new(),Vec3s::new());let mut caveats=vec![];let mut samples=0usize;
        let mut codecs=vec![];
        for m in kids.iter().rev(){
            match m.tag{
                0x12|0x13=>{
                    let c=if m.tag==0x12{delta_qfast(d,m)?}else{delta_singleq(d,m)?};
                    let bones=self.qfast_bones(m,c.channel_count).ok_or("could not resolve compound rotation bone table")?;
                    for (ch,&b) in bones.iter().enumerate(){if b<n_bones{rot.insert(b,c.frames[ch].clone());}}
                    samples=samples.max(c.sample_count);codecs.push(if m.tag==0x12{"FnDeltaQFast"}else{"FnDeltaSingleQ"});
                }
                0x14=>{
                    let t=delta_float::<3>(d,m)?;samples=samples.max(t.sample_count);codecs.push("FnDeltaF3");
                    if t.channel_count==0{caveats.push("empty FnDeltaF3 child (0 channels) writes nothing".into());continue}
                    let (tb,notes,scale_ch)=self.vector_bones(m,t.channel_count).ok_or_else(||format!("could not resolve compound F3 bone table ({} channels)",t.channel_count))?;
                    caveats.extend(notes);
                    for (ch,b) in tb.iter().enumerate(){if let Some(b)=b{if *b<n_bones{trans.insert(*b,t.frames[ch].clone());}}}
                    for (ch,&b) in &scale_ch{if b<n_bones{scale.insert(b,t.frames[*ch].clone());}}
                }
                0x15=>{
                    // FnDeltaF1 writes each scalar channel to one pose word (bone*12 + word): scale 0-2, rotation 4-7,
                    // translation 8-10.  Unwritten words keep the value already in the pose.
                    let t=delta_float::<1>(d,m)?;samples=samples.max(t.sample_count);codecs.push("FnDeltaF1");
                    let p=*self.reloc.get(&(m.abs-self.data_start+4)).ok_or("compound F1 channel table unresolved")?;let abs=self.data_start+p;
                    for ch in 0..t.channel_count{
                        let (bone,field,axis)=pose_field(be16(d,abs+ch*2)? as usize);
                        if bone>=n_bones{continue}
                        let b=&skel.bones[bone];
                        match field{
                            Field::Trans=>{let cur=trans.entry(bone).or_insert_with(||vec![Some(b.trans);t.sample_count]);for (s,v) in t.frames[ch].iter().enumerate(){if let (Some(v),Some(Some(c)))=(v,cur.get_mut(s)){c[axis]=v[0]}}}
                            Field::Scale=>{let cur=scale.entry(bone).or_insert_with(||vec![Some(b.scale);t.sample_count]);for (s,v) in t.frames[ch].iter().enumerate(){if let (Some(v),Some(Some(c)))=(v,cur.get_mut(s)){c[axis]=v[0]}}}
                            Field::Quat=>{let cur=rot.entry(bone).or_insert_with(||vec![Some(b.quat);t.sample_count]);for (s,v) in t.frames[ch].iter().enumerate(){if let (Some(v),Some(Some(c)))=(v,cur.get_mut(s)){c[axis]=v[0]}}}
                            Field::Pad=>caveats.push(format!("F1 channel {ch} writes padding word of bone {bone}")),
                        }
                    }
                }
                t=>return Err(format!("compound child tag {t:#x} not supported")),
            }
        }
        let fix=|v:&mut Samples<[f64;3]>,fill:[f64;3]|{v.resize(samples,Some(fill))};
        for (b,v) in trans.iter_mut(){let last=v.last().copied().flatten().unwrap_or(skel.bones[*b].trans);fix(v,last)}
        for (b,v) in scale.iter_mut(){let last=v.last().copied().flatten().unwrap_or(skel.bones[*b].scale);fix(v,last)}
        for v in rot.values_mut(){let last=v.last().copied().flatten().unwrap_or([0.,0.,0.,1.]);v.resize(samples,Some(last))}
        codecs.reverse();
        let codec:&'static str=Box::leak(format!("FnCompoundChannel({})",codecs.join("+")).into_boxed_str());
        let timing=self.compound_function(cb.index)?;
        if samples!=timing.samples as usize{caveats.push(format!("native container length {} differs from decoded child timeline {samples}; delta sparse time evaluation remains unverified",timing.samples));}
        Ok(Clip{index:cb.index,name,sample_count:samples,native_timing:Some(timing),codec,rot,trans,scale,caveats})
    }

    fn decode_raw(&self,skel:&Skeleton,cb:&ClipBlock)->Result<Clip,String>{
        let d=&self.data[..];let ds=self.data_start;let n_bones=skel.bones.len();
        let name=self.names.get(cb.index).cloned().unwrap_or_else(||format!("clip_{}",cb.index));
        let mut caveats:Vec<String>=vec![];let mut scale:Vec3s=BTreeMap::new();
        let clip=|codec,sample_count,rot,trans,scale,caveats|Clip{index:cb.index,name:name.clone(),sample_count,native_timing:None,codec,rot,trans,scale,caveats};
        let some=|v:&[[f64;4]]|->Samples<[f64;4]>{v.iter().map(|x|Some(*x)).collect()};
        let some3=|v:&[[f64;3]]|->Samples<[f64;3]>{v.iter().map(|x|Some(*x)).collect()};
        if cb.whole_clip{
            let rot_r=stateless_q(d,&self.reloc,ds,cb).map_err(|e|format!("FnStatelessQ decode failed: {e}"))?;
            let table=rot_r.bone_table.as_ref().ok_or("could not resolve FnStatelessQ bone table")?;
            let mut rot=Rot::new();
            let rs:Vec<Vec<[f64;4]>>=rot_r.frames.iter().map(|f|resample(f,&rot_r.times)).collect();
            let samples=match &rot_r.times{Some(t)=>t.last().map(|&x|x as usize+1).unwrap_or(1),None=>rot_r.keyframes};
            for (ch,bone) in table.iter().enumerate(){let bone=bone.unwrap();if bone>=n_bones{continue}rot.insert(bone,some(&rs[ch]));}
            for (bone,q) in &rot_r.statics{let Some(b)=*bone else{caveats.push("static rotation channel without a bone slot".into());continue};if b>=n_bones{continue}rot.insert(b,vec![Some(*q);samples]);}
            let mut trans=Vec3s::new();let mut trans_kf=None;
            match cb.secondary.filter(|m|m.tag==0x17){
                Some(m)=>match stateless_f3(d,&self.reloc,ds,&m){
                    Err(e)=>caveats.push(format!("FnStatelessF3 decode failed: {e} -- bind-pose translation used throughout")),
                    Ok(t)=>match &t.bone_table{
                        None=>caveats.push("could not resolve FnStatelessF3 bone table -- bind-pose translation used throughout".into()),
                        // The nested F3 track is evaluated with the rotation track's key index and fraction.
                        Some(bt)=>{trans_kf=Some(t.keyframes);for (ch,bone) in bt.iter().enumerate(){let Some(b)=bone else{continue};if *b>=n_bones{continue}trans.insert(*b,some3(&resample(&t.frames[ch],&rot_r.times)));}
                            for (bone,v) in &t.statics{let Some(b)=*bone else{caveats.push("static translation channel without a translation slot".into());continue};if b>=n_bones{continue}trans.insert(b,vec![Some(*v);samples]);}}
                    }
                },
                None=>caveats.push("no nested FnStatelessF3 secondary track -- bind-pose translation used throughout".into()),
            }
            if !trans.is_empty(){if let Some(k)=trans_kf{if k!=rot_r.keyframes{caveats.push(format!("translation keyframe_count ({k}) != rotation keyframe_count ({}) -- NOT resampled",rot_r.keyframes))}}}
            return Ok(clip(if rot_r.times.is_some(){"whole_clip sparse (FnStatelessQ+F3)"}else{"whole_clip (FnStatelessQ+F3)"},samples,rot,trans,scale,caveats));
        }
        if cb.container_tag==0x0f{return self.compound(skel,cb,name.clone())}
        if cb.primary.is_some_and(|m|m.tag==0x13){
            let prim=cb.primary.unwrap();
            let sq=delta_singleq(d,&prim)?;
            let sq_bones=self.qfast_bones(&prim,sq.channel_count);
            let sec=cb.secondary.ok_or("SingleQ clip without a secondary QFast track")?;
            let qf=delta_qfast(d,&sec)?;let qf_bones=self.qfast_bones(&sec,qf.channel_count);
            let mut rot=Rot::new();
            match sq_bones{None=>caveats.push("could not resolve FnDeltaSingleQ bone table".into()),Some(bs)=>for (ch,&bone) in bs.iter().enumerate(){if bone>=n_bones{continue}rot.insert(bone,sq.frames[ch].clone());}}
            match qf_bones{None=>caveats.push("could not resolve secondary FnDeltaQFast bone table".into()),Some(bs)=>{
                let overlap:Vec<usize>=bs.iter().copied().filter(|b|rot.contains_key(b)).collect();
                if !overlap.is_empty(){caveats.push(format!("UNEXPECTED bone overlap between SingleQ and QFast tracks: {overlap:?}"))}
                for (ch,&bone) in bs.iter().enumerate(){if bone>=n_bones{continue}rot.insert(bone,qf.frames[ch].clone());}
            }}
            if sq.sample_count!=qf.sample_count{caveats.push(format!("sample_count mismatch: SingleQ={} QFast={} -- NOT resampled",sq.sample_count,qf.sample_count))}
            return Ok(clip("FnDeltaSingleQ+QFast",sq.sample_count,rot,Vec3s::new(),scale,caveats));
        }
        let (Some(prim),Some(sec))=(cb.primary.filter(|m|m.tag==0x12),cb.secondary.filter(|m|matches!(m.tag,0x14|0x15))) else{
            return Err(format!("unsupported combo primary={:?} secondary={:?}",cb.primary.map(|m|m.tag),cb.secondary.map(|m|m.tag)));
        };
        let rot_r=delta_qfast(d,&prim)?;
        let bones=self.qfast_bones(&prim,rot_r.channel_count).ok_or("could not resolve rotation channel->bone table")?;
        let mut rot=Rot::new();
        for (ch,&bone) in bones.iter().enumerate(){if bone>=n_bones{continue}rot.insert(bone,rot_r.frames[ch].clone());}
        let mut trans=Vec3s::new();
        if sec.tag==0x14{
            let t=delta_float::<3>(d,&sec)?;
            if t.channel_count==0{caveats.push("CHANNEL_COUNT==0 (anomaly) -- no translation channels, bind-pose translation used throughout".into())}
            else{match self.vector_bones(&sec,t.channel_count){
                None=>caveats.push("could not resolve F3 channel->bone table -- bind-pose translation used throughout".into()),
                Some((tb,notes,scale_ch))=>{
                    caveats.extend(notes);
                    for (ch,bone) in tb.iter().enumerate(){let Some(b)=bone else{continue};if *b>=n_bones{continue}trans.insert(*b,t.frames[ch].clone());}
                    for (ch,&bone) in &scale_ch{if bone>=n_bones{continue}scale.insert(bone,t.frames[*ch].clone());}
                }
            }}
        }else{
            let t=delta_float::<1>(d,&sec)?;
            match self.scalar_bones(&sec,t.channel_count){
                None=>caveats.push("could not resolve F1 channel->bone table -- bind-pose translation used throughout".into()),
                Some(table)=>{
                    let n=t.sample_count;let mut per:BTreeMap<usize,BTreeMap<usize,usize>>=BTreeMap::new();
                    for (ch,&(bone,axis)) in table.iter().enumerate(){if bone>=n_bones||axis>2{continue}per.entry(bone).or_default().insert(axis,ch);}
                    for (bone,axes) in per{
                        if axes.len()<3{caveats.push(format!("bone {bone} ({}): only {}/3 axes present in F1 table -- missing axis held at bind pose",skel.bones[bone].name,axes.len()))}
                        let bind=skel.bones[bone].trans;
                        let mut v:Samples<[f64;3]>=vec![];
                        for s in 0..n{
                            let mut o=[0f64;3];
                            for ax in 0..3{o[ax]=match axes.get(&ax){Some(&ch)=>t.frames[ch][s].ok_or("missing F1 sample")?[0],None=>bind[ax]};}
                            v.push(Some(o));
                        }
                        trans.insert(bone,v);
                    }
                }
            }
        }
        Ok(clip("FnDeltaQFast+F3/F1",rot_r.sample_count,rot,trans,scale,caveats))
    }

    /// Full clip decode: raw decode, the engine's `sBoneMask` translation gate (player skeleton only: the table is
    /// specific to the 68-bone player rig) and the consistency checks the reference pipeline applies.
    pub fn decode(&self,index:usize,skel:&Skeleton)->Result<Clip,String>{
        let cb=self.blocks.get(index).ok_or_else(||format!("no clip {index}"))?;
        let mut c=self.decode_raw(skel,cb)?;
        if skel.is_player(){
            let dropped:Vec<usize>=c.trans.keys().copied().filter(|&b|b<BONE_MASK.len()&&BONE_MASK[b]==0).collect();
            if !dropped.is_empty(){
                for b in &dropped{c.trans.remove(b);}
                c.caveats.push(format!("sBoneMask: dropped translation channel(s) for bone(s) {dropped:?} (engine does not apply translation writes to these bones)"));
            }
        }
        let n=c.sample_count;
        if n<1{return Err("Clip has no samples".into())}
        fn check<T:AsRef<[f64]>>(field:&str,m:&BTreeMap<usize,Samples<T>>,n:usize,bones:usize)->Result<(),String>{
            for (b,v) in m{
                if *b>=bones||v.len()!=n{return Err(format!("{field}: invalid bone {b} or mismatched sample count"))}
                if v.iter().flatten().any(|s|s.as_ref().iter().any(|x|!x.is_finite())){return Err(format!("{field}: non-finite sample values"))}
            }
            Ok(())
        }
        check("rot_by_bone",&c.rot,n,skel.bones.len())?;check("trans_by_bone",&c.trans,n,skel.bones.len())?;check("scale_by_bone",&c.scale,n,skel.bones.len())?;
        Ok(c)
    }
}

/// glTF/Bevy need unit rotations without sign flips between neighbours; the raw codec samples are kept in `Clip`.
pub fn normalized(samples:&Samples<[f64;4]>)->Result<Vec<[f32;4]>,String>{
    let mut out:Vec<[f64;4]>=vec![];
    for s in samples{
        let q=s.unwrap_or([0.,0.,0.,1.]);let norm=q.iter().map(|x|x*x).sum::<f64>().sqrt();
        if !norm.is_finite()||norm<1e-10{return Err("Invalid quaternion".into())}
        let mut q=[q[0]/norm,q[1]/norm,q[2]/norm,q[3]/norm];
        if let Some(p)=out.last(){if p.iter().zip(q.iter()).map(|(a,b)|a*b).sum::<f64>()<0.{q=[-q[0],-q[1],-q[2],-q[3]]}}
        out.push(q);
    }
    Ok(out.iter().map(|q|[q[0] as f32,q[1] as f32,q[2] as f32,q[3] as f32]).collect())
}

#[cfg(test)]
mod tests{
    use super::*;
    pub fn viv_path(bank:&str)->String{crate::bridge::data_root().join("files").join("data").join(bank.split_once("::").unwrap().0.replace('/',"\\")).to_string_lossy().into_owned()}
    fn approx(a:f64,b:f64)->bool{(a-b).abs()<=1e-7*(1.+b.abs())}
    fn summary<T:AsRef<[f64]>>(m:&BTreeMap<usize,Samples<T>>)->Vec<(usize,usize,f64)>{
        m.iter().map(|(b,v)|(*b,v.len(),v.iter().flatten().flat_map(|s|s.as_ref().iter().copied()).sum::<f64>())).collect()
    }
    #[test]
    fn unpack_matches_recorded_values(){
        // Values recorded when the trick was first verified against the disassembly.
        assert!((unpack_u16(0xfa7d)+0.0465).abs()<1e-3,"{}",unpack_u16(0xfa7d));
        assert!((unpack_u16(0x7eff)-0.998).abs()<1e-3,"{}",unpack_u16(0x7eff));
    }
    /// Every clip of every corpus bank must decode (or fail) exactly like the Python reference.
    /// Every clip the Python reference decodes must still decode, and every track the reference produced must be
    /// reproduced sample-for-sample, except where the executable's evaluators prove the reference wrong:
    /// * whole-clip translations are signed 16-bit x scale (`FnStatelessF3::EvalSQTfast` 0x803fdc1c), not offset binary;
    /// * extra static channels and every child of a `FnCompoundChannel` (0x803ff53c) are applied, lower index last, so
    ///   compound clips gain the tracks the reference ignored (their added bones are not in the reference);
    /// * with both SingleQ and QFast children, SingleQ (index 0) is evaluated last and wins on shared bones.
    /// The two prop banks the reference could not decode (compound + sparse) now decode as well.
    #[test]
    fn matches_python_reference_for_every_clip(){
        let p=std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data/anim_golden.json");
        let Ok(txt)=std::fs::read_to_string(p) else{eprintln!("anim_golden.json absent; skipped");return};
        let g:serde_json::Value=serde_json::from_str(&txt).unwrap();
        let (mut ok,mut newly,mut exempt)=(0,0,0);let mut bad=vec![];
        for (bank,entry) in g.as_object().unwrap(){
            let viv=viv_path(bank);
            let Ok((data,_))=crate::archive::read_virtual(&format!("{viv}::{}",bank.split_once("::").unwrap().1)) else{eprintln!("DATA absent; skipped");return};
            let stem=std::path::Path::new(bank.split_once("::").unwrap().0).file_stem().unwrap().to_string_lossy().into_owned();
            let ske_name=if stem.contains("player"){"player_skel.ske".to_string()}else{format!("{stem}_skel.ske")};
            let ske=Skeleton::parse(&crate::archive::read_virtual(&format!("{viv}::{ske_name}")).unwrap().0).unwrap();
            let bank_data=Bank::parse(data).unwrap();
            for w in entry["clips"].as_array().unwrap(){
                let i=w["index"].as_u64().unwrap() as usize;
                let c=match bank_data.decode(i,&ske){Ok(c)=>c,Err(e)=>{bad.push(format!("{bank} clip {i}: {e}"));continue}};
                if !w["ok"].as_bool().unwrap(){newly+=1;continue}
                let whole=w["codec"].as_str().unwrap().starts_with("whole_clip");
                let order=c.codec.contains("SingleQ")&&c.codec.contains("QFast");
                let f1_added=c.codec.contains("FnDeltaF1")&&!w["codec"].as_str().unwrap().contains("F1");
                let mut errs=vec![];
                for (field,got) in [("rot",summary(&c.rot)),("trans",summary(&c.trans)),("scale",summary(&c.scale))]{
                    let got:std::collections::HashMap<usize,(usize,f64)>=got.into_iter().map(|(b,n,s)|(b,(n,s))).collect();
                    for x in w[field].as_array().unwrap(){
                        let b=x[0].as_u64().unwrap() as usize;
                        let Some(&(n,sum))=got.get(&b) else{errs.push(format!("{field} bone {b} missing"));continue};
                        let same=n as u64==x[1].as_u64().unwrap()&&approx(sum,x[2].as_f64().unwrap());
                        let allowed=(field=="trans"&&(whole||f1_added))||(field=="rot"&&order);
                        if !same{if allowed{exempt+=1}else{errs.push(format!("{field} bone {b}: {n} samples sum {sum} vs {x}"))}}
                    }
                }
                if errs.is_empty(){ok+=1}else{bad.push(format!("{bank} clip {i} ({}): {errs:?}",c.codec))}
            }
        }
        assert!(bad.is_empty(),"{} ok, {} bad; first: {:?}",ok,bad.len(),&bad[..bad.len().min(5)]);
        eprintln!("animation corpus: {ok} clips reproduce the Python reference ({exempt} evidence-backed track differences), {newly} clips newly decoded");
        assert!(ok>=266&&newly==2);
    }

    /// Whole clips: the signed decode puts the root on its bind translation (StoryTeller, frame 0), the old offset-binary
    /// reading put it 32768 x scale away.
    #[test]
    fn whole_clip_root_translation_matches_bind_pose(){
        let viv=crate::bridge::data_root().join("files/data/characters/player_anims.viv").to_string_lossy().into_owned();
        let Ok((data,_))=crate::archive::read_virtual(&format!("{viv}::player_anims.anm")) else{eprintln!("DATA absent; skipped");return};
        let ske=Skeleton::parse(&crate::archive::read_virtual(&format!("{viv}::player_skel.ske")).unwrap().0).unwrap();
        let b=Bank::parse(data).unwrap();
        let i=b.names.iter().position(|n|n=="S_AI_StoryTeller").unwrap();
        let c=b.decode(i,&ske).unwrap();
        let root=c.trans[&0][0].unwrap();let bind=ske.bones[0].trans;
        for k in 0..3{assert!((root[k]-bind[k]).abs()<0.01,"root {root:?} bind {bind:?}")}
    }
}
