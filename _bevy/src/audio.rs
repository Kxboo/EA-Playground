//! EA streamed audio (`.asf`/`.ast`): the `SCHl` container and its EA Layer 3 payload.
//!
//! Container (little-endian sizes): `SCHl` header block (platform tag, variable-length patch entries), `SCCl` block count,
//! then `SCDl` data blocks and a final `SCEl`.  Header entries are `tag, length, big-endian value`: 0x85 total samples,
//! 0x82 channels, 0x84 sample rate, 0xA0 codec (0x17 = EA Layer 3 in every music/ambience stream; 0x04 = the speech
//! codec), 0xFD/0xFF delimit the entries.  Each `SCDl` block: u32 BE sample count, u32 0, four marker bytes, then frames
//! `[prefix byte][EA Layer 3 frame]` (see mp3.rs).
use crate::mp3::{self,Layer3};
use crate::skeleton::be32;

#[derive(Debug,Clone,Default)]
pub struct Header{pub samples:u32,pub channels:u8,pub sample_rate:u32,pub codec:u8,pub entries:Vec<(u8,u32)>,/// Entries longer than four bytes (tag, raw bytes), e.g. 0x8f = codec 0x12 coefficients.
    pub long:Vec<(u8,Vec<u8>)>}

fn le32(d:&[u8],o:usize)->Result<usize,String>{d.get(o..o+4).map(|b|u32::from_le_bytes(b.try_into().unwrap()) as usize).ok_or_else(||format!("read past end at {o:#x}"))}

pub fn parse_header(d:&[u8])->Result<(Header,usize),String>{
    if d.get(..4)!=Some(b"SCHl"){return Err("Not an EA SCHl stream".into())}
    let size=le32(d,4)?;
    let body=d.get(8..size).ok_or("SCHl block exceeds file")?;
    Ok((parse_tags(body),size))
}

/// Parse a header body: two platform bytes ("PT"), platform id, flags, then `tag, length, value` entries up to 0xFF.
pub fn parse_tags(body:&[u8])->Header{parse_tags_len(body).0}
/// Like `parse_tags`, also returning the number of bytes up to and including the 0xFF terminator.
pub fn parse_tags_len(body:&[u8])->(Header,usize){
    // Speech streams ("GSTR" platform tag) carry four more bytes before the tags than the "PT.." music headers.
    let mut p=if body.starts_with(b"GSTR"){8usize}else{4};let mut h=Header{channels:1,..Default::default()};
    while p<body.len(){
        let tag=body[p];p+=1;
        match tag{
            0xFF=>break,
            0xFC|0xFD|0xFE=>continue, // stream section delimiters carry no value
            _=>{
                let Some(&n)=body.get(p) else{break};let n=n as usize;p+=1;
                if n>4||p+n>body.len(){ // long entries (e.g. speech tables, 0x8f) are kept raw
                    let end=(p+n).min(body.len());h.long.push((tag,body[p..end].to_vec()));p=end;continue
                }
                let v=body[p..p+n].iter().fold(0u32,|a,&b|(a<<8)|b as u32);p+=n;
                h.entries.push((tag,v));
                match tag{0x85=>h.samples=v,0x82=>h.channels=v as u8,0x84=>h.sample_rate=v,0xA0=>h.codec=v as u8,_=>{}}
            }
        }
    }
    (h,p)
}

/// Payload ranges (start,end) of every `SCDl` block.
pub fn data_blocks(d:&[u8])->Result<Vec<(usize,usize)>,String>{
    let mut p=le32(d,4)?;let mut out=vec![];
    while p+8<=d.len(){
        let tag=&d[p..p+4];let size=le32(d,p+4)?;
        if size<8||p+size>d.len(){break}
        match tag{b"SCDl"=>out.push((p+8,p+size)),b"SCEl"=>break,b"SCCl"=>{},_=>{}}
        p+=size;
    }
    Ok(out)
}

/// Walk one block's chunks: type byte 0x00 = MPEG frame (header byte follows), 0xEE = raw PCM `[EE][u16 offset][u16 count][count x channels x i16]`.
pub fn describe_block(payload:&[u8],channels:usize)->Result<Vec<String>,String>{
    let want=be32(payload,0)? as usize;let buf=payload.get(12..).ok_or("short SCDl block")?;
    let mut dec=Layer3::new();let mut o=0usize;let mut out=vec![format!("want {want} samples, {} bytes",buf.len())];
    while o<buf.len(){
        match buf[o]{
            0x00=>{let f=dec.decode_frame(&buf[o+1..]);match f{Ok(f)=>{out.push(format!("@{o}: frame gr{} {}ch {} bytes",f.granule,f.channels,f.bytes));o+=1+f.bytes}Err(e)=>{out.push(format!("@{o}: frame error {e}"));break}}}
            0xEE=>{let off=u16::from_be_bytes([buf[o+1],buf[o+2]]);let n=u16::from_be_bytes([buf[o+3],buf[o+4]]) as usize;out.push(format!("@{o}: raw pcm offset {off} count {n}"));o+=5+n*channels*2}
            b=>{out.push(format!("@{o}: unknown chunk type {b:#04x} (rest {} bytes)",buf.len()-o));break}
        }
    }
    Ok(out)
}

/// Byte ranges of every `SCHl ... SCEl` stream in a file (`.asf` holds one, `.ast` banks concatenate many, padded with zeros).
pub fn streams(d:&[u8])->Vec<(usize,usize)>{
    let mut out=vec![];let mut p=0usize;
    while p+8<=d.len(){
        if &d[p..p+4]!=b"SCHl"{p+=1;continue}
        let start=p;let mut q=p;
        while q+8<=d.len(){
            let size=match le32(d,q+4){Ok(s)=>s,Err(_)=>break};
            let tag=&d[q..q+4];
            if !matches!(tag,b"SCHl"|b"SCCl"|b"SCDl"|b"SCEl")||size<8||q+size>d.len(){break}
            q+=size;if tag==b"SCEl"{break}
        }
        out.push((start,q));p=q.max(start+1);
    }
    out
}

#[derive(Clone)] pub struct Pcm{pub sample_rate:u32,pub channels:usize,pub samples:Vec<i16>,pub frames:u64,pub stats:mp3::Stats}

/// The engine discards the first 1105 samples of a stream (`li r28,0x451; stw r28,0x1d0` in `CEALayer3Dec::Decode`,
/// 0x802808f8): one granule plus the 529-sample decoder delay.
pub const START_SKIP:usize=1105;

/// Incremental EA Layer 3 decoder: yields one `SCDl` block of interleaved 16-bit PCM at a time (used for streaming
/// playback; `decode` collects all blocks).
pub struct StreamDecoder{d:Vec<u8>,pub header:Header,blocks:Vec<(usize,usize)>,next:usize,dec:Layer3,skip:usize,pub frames:u64}
impl StreamDecoder{
    pub fn new(d:Vec<u8>)->Result<Self,String>{
        let (header,_)=parse_header(&d)?;
        if header.codec!=0x17{return Err(format!("codec {:#x} is not EA Layer 3",header.codec))}
        let blocks=data_blocks(&d)?;
        Ok(Self{d,header,blocks,next:0,dec:Layer3::new(),skip:START_SKIP,frames:0})
    }
    pub fn channels(&self)->usize{self.header.channels.max(1) as usize}
    pub fn stats(&self)->mp3::Stats{self.dec.stats.clone()}
    /// Start again from the first block (loop playback).
    pub fn rewind(&mut self){self.next=0;self.dec.reset();self.skip=START_SKIP;}
    pub fn next_block(&mut self)->Result<Option<Vec<i16>>,String>{
        let Some(&(s,e))=self.blocks.get(self.next) else{return Ok(None)};
        let bi=self.next;self.next+=1;
        let ch=self.channels();let payload=&self.d[s..e];let want=be32(payload,0)? as usize;let buf=payload.get(12..).ok_or("short SCDl block")?;
        let samples=layer3_run(buf,want,ch,&mut self.dec,&mut self.skip,&mut self.frames,bi)?;
        Ok(Some(samples))
    }
}

/// Shared CEALayer3Dec chunk walk. Bank payloads begin at the type byte; streamed
/// SCDl payloads supply the same bytes after their 12-byte block header.
fn layer3_run(buf:&[u8],want:usize,ch:usize,dec:&mut Layer3,skip:&mut usize,frames:&mut u64,bi:usize)->Result<Vec<i16>,String>{
    if !(1..=2).contains(&ch){return Err("EA Layer 3 requires one or two channels".into())}
    let capacity=want.checked_mul(ch).ok_or("EA Layer 3 sample count overflow")?;
        let (mut o,mut emitted)=(0usize,0usize);let mut samples=Vec::new();
        samples.try_reserve_exact(capacity).map_err(|_|"EA Layer 3 sample allocation too large")?;
        while emitted<want&&o<buf.len(){
            if buf[o]!=0{return Err(format!("block {bi}: unexpected chunk type {:#04x} at {o}",buf[o]))}
            let f=dec.decode_frame(&buf[o+1..]).map_err(|er|format!("block {bi} frame at {o}: {er}"))?;
            if f.channels!=ch{return Err(format!("block {bi}: frame has {} channels, stream header says {ch}",f.channels))}
            *frames+=1;o+=1+f.bytes;let mut pcm=f.pcm;
            while buf.get(o)==Some(&0xEE){
                if buf.len()-o<5{return Err(format!("block {bi}: raw PCM header truncated"))}
                let off=u16::from_be_bytes([buf[o+1],buf[o+2]]) as usize;let n=u16::from_be_bytes([buf[o+3],buf[o+4]]) as usize;
                let start=576usize.checked_sub(off).ok_or("raw PCM offset exceeds a granule")?;
                if start+n>576||o+5+n*ch*2>buf.len(){return Err(format!("block {bi}: raw PCM chunk out of range"))}
                for k in 0..n{for c in 0..ch{let p=o+5+(k*ch+c)*2;pcm[c][start+k]=i16::from_be_bytes([buf[p],buf[p+1]]) as f64/32768.;}}
                o+=5+n*ch*2;
            }
            let from=(*skip).min(576);*skip-=from;
            for i in from..576{
                if emitted>=want{break}
                for c in 0..ch{samples.push(mp3::to_i16(pcm[c][i]));}
                emitted+=1;
            }
        }
        if emitted!=want{return Err(format!("block {bi}: produced {emitted} of {want} samples"))}
    Ok(samples)
}

/// Decode a whole stream (or the first `max_blocks` blocks) to interleaved 16-bit PCM.
///
/// Chunks inside a block: type 0x00 = one EA Layer 3 frame; type 0xEE = raw PCM `[u16 offset][u16 count][count x ch x i16]`
/// that overwrites `count` samples of the frame decoded just before it, starting at index `576 - offset`
/// (`Decode` 0x80280a4c..0x80280b78) - EA uses it to splice exact samples at loop points.
pub fn decode(d:&[u8],max_blocks:Option<usize>)->Result<Pcm,String>{
    let mut sd=StreamDecoder::new(d.to_vec())?;let mut samples:Vec<i16>=vec![];let mut n=0usize;
    while max_blocks.is_none_or(|m|n<m){match sd.next_block()?{Some(b)=>samples.extend(b),None=>break}n+=1;}
    Ok(Pcm{sample_rate:sd.header.sample_rate,channels:sd.channels(),samples,frames:sd.frames,stats:sd.stats()})
}

/// One sound of a `BNKb` bank (`banks/*.bnk`): 'BNKb', u8 version, u8 0, u16 count, u32 file size, 8 zero bytes, then `count`
/// u32 values (4 + 0x24 * i, not used by the loader path decoded here) then the 0x28-byte sound headers in the same tag format as `SCHl` (tag 0x88 = data offset).
pub struct BankSound{pub header:Header,pub data_offset:usize}
pub fn parse_bank(d:&[u8])->Result<Vec<BankSound>,String>{
    if d.get(..4)!=Some(b"BNKb"){return Err("Not a BNKb bank".into())}
    let count_bytes=d.get(6..8).ok_or("truncated BNKb header")?;
    let count=u16::from_be_bytes(count_bytes.try_into().unwrap()) as usize;let mut out=vec![];let mut pos=0x14+count*4;
    for _ in 0..count{
        // Sound headers follow the table back to back; each ends with a 0xFF tag and is padded to 4 bytes.
        let body=d.get(pos..).ok_or("sound header outside bank")?;
        // `count` also covers trailing table entries in AEMS banks; real headers begin with the "PT" platform tag.
        if !body.starts_with(b"PT"){break}
        let (header,used)=parse_tags_len(body);pos+=(used+3)&!3;
        let data_offset=header.entries.iter().find(|e|e.0==0x88).map(|e|e.1 as usize).ok_or("sound without a data offset")?;
        out.push(BankSound{header,data_offset});
    }
    Ok(out)
}
/// The `BNKb` sound bank inside an AEMS module bank (`aems/*.abk`, magic `ABKC`): `AddModuleBank` (0x802764dc) hands
/// `SNDbankadd` the bytes at header word 0x20 (offset), of size word 0x24; `resolvemodulebank` (0x8027620c) then relocates the
/// module-graph part.  Returns `None` for banks without samples (the `amb_*.abk` ones stream from `.ast` files).
pub fn abk_bank(d:&[u8])->Result<Option<&[u8]>,String>{
    if d.get(..4)!=Some(b"ABKC"){return Err("Not an ABKC module bank".into())}
    let off=be32(d,0x20)? as usize;let size=be32(d,0x24)? as usize;
    if off==0{return Ok(None)}
    d.get(off..off+size).map(Some).ok_or_else(||"sound bank outside the module bank".into())
}
/// GameCube/Wii DSP-ADPCM (codec 0x12): 8-byte frames -> 14 samples; the first byte is `predictor << 4 | scale`, then 14 signed
/// nibbles (high first): `s = (nibble << scale << 11 + 1024 + c1*h1 + c2*h2) >> 11`.  The eight coefficient pairs (Q11, big-endian
/// i16) are the first 32 bytes of the sound header's tag 0x8f entry.
pub fn decode_dsp(data:&[u8],coefs:&[u8],samples:usize)->Result<Vec<i16>,String>{
    if coefs.len()<32{return Err("DSP-ADPCM sound without coefficients".into())}
    let c:Vec<i32>=(0..16).map(|i|i16::from_be_bytes([coefs[2*i],coefs[2*i+1]]) as i32).collect();
    let (mut h1,mut h2)=(0i32,0i32);let mut out=Vec::with_capacity(samples+14);
    for frame in data.chunks(8){
        if out.len()>=samples{break}
        if frame.len()<8{return Err("DSP-ADPCM data ends inside a frame".into())}
        let (pred,scale)=((frame[0]>>4) as usize&7,(frame[0]&15) as u32);
        let (c1,c2)=(c[pred*2],c[pred*2+1]);
        for k in 0..14{
            let b=frame[1+k/2];let nib=if k%2==0{b>>4}else{b&15};let nib=((nib as i32)<<28)>>28;
            let v=(((nib<<scale)<<11)+1024+c1*h1+c2*h2)>>11;let v=v.clamp(-32768,32767);
            h2=h1;h1=v;out.push(v as i16);
        }
    }
    if out.len()<samples{return Err("DSP-ADPCM data too short".into())}
    out.truncate(samples);Ok(out)
}
/// Decode sound `i` of a bank (EA-XA, DSP-ADPCM, and EA Layer 3).
pub fn decode_bank_sound(d:&[u8],i:usize)->Result<Pcm,String>{
    let sounds=parse_bank(d)?;let s=sounds.get(i).ok_or("no such sound")?;
    if s.header.codec==0x17{
        // CEALayer3Dec::Feed (0x802808dc) accepts a byte pointer/size and sample count;
        // Decode (0x80280988..0x802809ac) starts its first frame at pointer + 1 and
        // sets a 0x451-sample skip. BNKb data has no SCDl count/offset preamble.
        // Bound reads to the next sound, including banks whose offsets share data.
        let end=sounds.iter().map(|x|x.data_offset).filter(|&o|o>s.data_offset).min().unwrap_or(d.len());
        let data=d.get(s.data_offset..end).ok_or("sound data outside bank")?;
        let ch=s.header.channels.max(1) as usize;
        let mut dec=Layer3::new();let mut skip=START_SKIP;let mut frames=0;
        let samples=layer3_run(data,s.header.samples as usize,ch,&mut dec,&mut skip,&mut frames,i)?;
        return Ok(Pcm{sample_rate:s.header.sample_rate,channels:ch,samples,frames,stats:dec.stats})
    }
    if s.header.codec==0x12{
        let coefs=s.header.long.iter().find(|e|e.0==0x8f).map(|e|e.1.as_slice()).ok_or("no coefficient entry")?;
        let data=d.get(s.data_offset..).ok_or("sound data outside bank")?;
        let samples=decode_dsp(data,coefs,s.header.samples as usize)?;
        return Ok(Pcm{sample_rate:s.header.sample_rate,channels:1,samples,frames:0,stats:mp3::Stats::default()})
    }
    if s.header.codec!=0x0a{return Err(format!("codec {:#x} has no bank decoder",s.header.codec))}
    let mut pos=s.data_offset;let mut hist=(0f32,0f32);
    let samples=xa_run(d,&mut pos,s.header.samples as usize,&mut hist)?;
    Ok(Pcm{sample_rate:s.header.sample_rate,channels:1,samples,frames:0,stats:mp3::Stats::default()})
}

/// EA-XA ADPCM (`decodexac` 0x80280dfc): codec 0x0A.  A block is 15 bytes -> 28 samples: header byte (high nibble selects
/// the predictor pair in `XA_FILTER`, low nibble the shift row in `XA_TABLE`), then 14 bytes of two 4-bit codes
/// (high nibble first): `s = table[row][nibble] + prev * c1 + prev2 * c2`.  A block starting with 0xEE is raw:
/// `[EE][i16 prev][i16 prev2][28 x i16]` (`process_raw_block` 0x80280ce4).  Mono streams only.
/// Decode `want` samples of EA-XA blocks starting at `pos` (predictor state carried in `hist`).
pub fn xa_run(data:&[u8],pos:&mut usize,want:usize,hist:&mut (f32,f32))->Result<Vec<i16>,String>{
    use crate::mp3_tables::{XA_FILTER,XA_TABLE};
    let s16=|d:&[u8],o:usize|->Result<f32,String>{d.get(o..o+2).map(|b|i16::from_be_bytes([b[0],b[1]]) as f32).ok_or_else(||"EA-XA raw block truncated".to_string())};
    let clamp=|v:f32|v.round().clamp(-32768.,32767.) as i16;
    let (mut h1,mut h2)=*hist;let mut run:Vec<i16>=Vec::with_capacity(want+28);
    while run.len()<want{
        let Some(&first)=data.get(*pos) else{return Err("EA-XA data ends early".into())};
        if first==0xEE{
            h1=s16(data,*pos+1)?;h2=s16(data,*pos+3)?;
            for k in 0..28{run.push(clamp(s16(data,*pos+5+k*2)?));}
            *pos+=61;
        }else{
            let hdr=first as usize;let (c1,c2)=(XA_FILTER[hdr>>4&3],XA_FILTER[4+(hdr>>4&3)]);let row=(hdr&15)*16;
            let bytes=data.get(*pos+1..*pos+15).ok_or("EA-XA block truncated")?;
            for &b in bytes{for nib in [b>>4,b&15]{
                let v=XA_TABLE[row+nib as usize]+h1*c1+h2*c2;h2=h1;h1=v;run.push(clamp(v));
            }}
            *pos+=15;
        }
    }
    run.truncate(want);*hist=(h1,h2);Ok(run)
}

pub fn decode_xa(d:&[u8])->Result<Pcm,String>{
    let (h,_)=parse_header(d)?;
    if h.codec!=0x0a{return Err(format!("codec {:#x} is not EA-XA",h.codec))}
    let ch=h.channels.max(1) as usize;
    // Stereo blocks hold each channel's run of ADPCM blocks one after the other; every channel keeps its own predictor.
    let mut out:Vec<i16>=vec![];let mut hist=vec![(0f32,0f32);ch];
    for (bi,(s,e)) in data_blocks(d)?.into_iter().enumerate(){
        let payload=&d[s..e];let want=be32(payload,0)? as usize;
        // After the sample count come one u32 offset per channel (relative to the data that follows them).
        let data=payload.get(4+4*ch..).ok_or("short SCDl block")?;let mut runs:Vec<Vec<i16>>=vec![];
        for c in 0..ch{
            let mut pos=be32(payload,4+4*c)? as usize;
            runs.push(xa_run(data,&mut pos,want,&mut hist[c]).map_err(|er|format!("block {bi}: {er}"))?);
        }
        for i in 0..want{for r in &runs{out.push(r[i]);}}
    }
    Ok(Pcm{sample_rate:h.sample_rate,channels:ch,samples:out,frames:0,stats:mp3::Stats::default()})
}

/// EA MicroTalk speech (codec 0x04, `spchdat.viv` `.dat` files).  Derived from `CMTBLKDec::Feed/Decode` (0x80287874/0x80287914)
/// and checked against the data: an `SCDl` block is `[u32 samples][u32 0][u8 header flag]` followed by chunks
/// `[type 0x00|0xEE][one byte-aligned 432-sample frame]`; a 0xEE chunk continues with `[u16 offset][u16 count][count x i16]`
/// raw samples that replace `frame[offset..]`.  The first block's flag is 1: the 15 header bits (reduced bandwidth,
/// multipulse threshold, gain base and step) precede its first frame.  Decoder state carries over between blocks.
pub fn decode_utk(d:&[u8])->Result<Pcm,String>{
    let (h,_)=parse_header(d)?;
    if h.codec!=0x04{return Err(format!("codec {:#x} is not MicroTalk",h.codec))}
    let mut utk:Option<crate::utk::Utk>=None;let mut out:Vec<i16>=vec![];
    for (bi,(s,e)) in data_blocks(d)?.into_iter().enumerate(){
        let pl=&d[s..e];let want=be32(pl,0)? as usize;
        let mut t=9usize;let mut done=0usize;
        while done<want{
            let ty=*pl.get(t).ok_or_else(||format!("block {bi}: data ends after {done} of {want} samples"))?;
            if ty!=0&&ty!=0xEE{return Err(format!("block {bi}: chunk type {ty:#x} at {t}"))}
            match utk.as_mut(){
                None=>utk=Some(crate::utk::Utk::new(pl.to_vec(),t+1,pl[8]!=0)?),
                Some(u)=>{if done==0{u.load(pl.to_vec())}u.seek(t+1)?}
            }
            let u=utk.as_mut().unwrap();
            u.decode_frame()?;
            let p=u.position()-1;
            if ty==0xEE{
                let off=u16::from_be_bytes(pl.get(p..p+2).ok_or("PCM header truncated")?.try_into().unwrap()) as usize;
                let cnt=u16::from_be_bytes(pl.get(p+2..p+4).ok_or("PCM header truncated")?.try_into().unwrap()) as usize;
                let raw=pl.get(p+4..p+4+cnt*2).ok_or("PCM splice truncated")?;
                let fr=u.frame_mut();if off+cnt>fr.len(){return Err("PCM splice outside the frame".into())}
                for i in 0..cnt{fr[off+i]=i16::from_be_bytes([raw[2*i],raw[2*i+1]]) as f32;}
                t=p+4+cnt*2;
            }else{t=p}
            let n=(want-done).min(crate::utk::FRAME);
            out.extend(u.frame()[..n].iter().map(|&v|v.round().clamp(-32768.,32767.) as i16));
            done+=n;
        }
    }
    Ok(Pcm{sample_rate:h.sample_rate,channels:1,samples:out,frames:0,stats:mp3::Stats::default()})
}

/// RIFF/WAVE (16-bit PCM) bytes for a decoded stream.
pub fn to_wav(p:&Pcm)->Vec<u8>{
    let data_len=(p.samples.len()*2) as u32;let mut w=Vec::with_capacity(44+data_len as usize);
    w.extend_from_slice(b"RIFF");w.extend_from_slice(&(36+data_len).to_le_bytes());w.extend_from_slice(b"WAVEfmt ");
    w.extend_from_slice(&16u32.to_le_bytes());w.extend_from_slice(&1u16.to_le_bytes());w.extend_from_slice(&(p.channels as u16).to_le_bytes());
    w.extend_from_slice(&p.sample_rate.to_le_bytes());w.extend_from_slice(&(p.sample_rate*p.channels as u32*2).to_le_bytes());
    w.extend_from_slice(&((p.channels*2) as u16).to_le_bytes());w.extend_from_slice(&16u16.to_le_bytes());
    w.extend_from_slice(b"data");w.extend_from_slice(&data_len.to_le_bytes());
    for s in &p.samples{w.extend_from_slice(&s.to_le_bytes());}
    w
}

#[cfg(test)]
mod tests{
    use super::*;
    fn silent_layer3_chunk()->Vec<u8>{
        // MPEG-1/32kHz/mono, granule 0, zero part2_3 and big_values, long block.
        let mut bits=Vec::new();
        for (value,n) in [(0xec,8),(0,1),(0,12),(0,9),(210,8),(0,4),(0,1),(0,15),(0,4),(0,3),(0,1),(0,1),(0,1)]{
            for i in (0..n).rev(){bits.push(((value>>i)&1) as u8)}
        }
        let mut chunk=vec![0];
        chunk.extend(bits.chunks(8).map(|b|b.iter().enumerate().fold(0,|v,(i,&bit)|v|(bit<<(7-i)))));
        chunk
    }
    #[test] fn bank_layer3_delay_and_raw_splice(){
        let mut data=silent_layer3_chunk();data.extend(silent_layer3_chunk());
        // Skip 576 + 529 samples; the first returned samples are at 529 and 530.
        data.extend([0xee,0,47,0,2,0x12,0x34,0xfe,0xdc]);
        let mut dec=Layer3::new();let mut skip=START_SKIP;let mut frames=0;
        let pcm=layer3_run(&data,2,1,&mut dec,&mut skip,&mut frames,0).unwrap();
        assert_eq!(pcm,[0x1234,-292]);assert_eq!((skip,frames),(0,2));
        assert_eq!(dec.stats.exact_end,2);
        data.pop();
        assert!(layer3_run(&data,2,1,&mut Layer3::new(),&mut {START_SKIP},&mut 0,0).unwrap_err().contains("out of range"));
    }
    #[test] fn bank_layer3_rejects_incomplete_splice_after_sample_target(){
        // Two granules leave exactly 47 samples after the decoder delay. A following
        // splice must still be validated even though those samples satisfy the target.
        for tail in 1..5{
            let mut data=silent_layer3_chunk();data.extend(silent_layer3_chunk());
            data.extend_from_slice(&[0xee,0,47,0][..tail]);
            let mut skip=START_SKIP;let mut frames=0;
            let err=layer3_run(&data,47,1,&mut Layer3::new(),&mut skip,&mut frames,0).unwrap_err();
            assert!(err.contains("raw PCM header truncated"),"{tail} byte suffix: {err}");
        }
    }
    #[test] fn bank_layer3_rejects_truncation_and_versions(){
        assert!(parse_bank(b"BNKb").is_err());
        let mut skip=0;let mut frames=0;
        assert!(layer3_run(&[0,0xec],1,1,&mut Layer3::new(),&mut skip,&mut frames,0).is_err());
        assert!(layer3_run(&[0,0x0c],1,1,&mut Layer3::new(),&mut skip,&mut frames,0).unwrap_err().contains("unsupported MPEG version bits 0"));
        assert!(layer3_run(&[1],1,1,&mut Layer3::new(),&mut skip,&mut frames,0).unwrap_err().contains("unexpected chunk type"));
    }
    #[test] fn abk_layer3_banks(){
        let dir=crate::bridge::data_root().join("files/data/audio/aems");
        let Ok(rd)=std::fs::read_dir(dir) else{eprintln!("DATA absent; skipped");return};
        let (mut decoded,mut mpeg2)=(0,0);
        for e in rd.flatten(){
            let p=e.path();if p.extension().is_none_or(|x|x!="abk"){continue}
            let d=std::fs::read(&p).unwrap();let Some(bank)=abk_bank(&d).unwrap() else{continue};
            for (i,s) in parse_bank(bank).unwrap().iter().enumerate(){
                if s.header.codec!=0x17{continue}
                if bank[s.data_offset+1]>>6==2{mpeg2+=1;}
                let pcm=decode_bank_sound(bank,i).unwrap_or_else(|e|panic!("{} #{i}: {e}",p.display()));
                eprintln!("{} #{i}: {} samples/ch @{}Hz, stats {:?}",p.file_name().unwrap().to_string_lossy(),s.header.samples,pcm.sample_rate,pcm.stats);
                assert_eq!(pcm.channels,s.header.channels.max(1) as usize);
                assert_eq!(pcm.samples.len(),s.header.samples as usize*pcm.channels);
                assert_eq!(pcm.stats.exact_end,pcm.stats.frames);
                assert_eq!(pcm.stats.overrun+pcm.stats.rewinds,0);
                let clipped=pcm.samples.iter().filter(|&&x|x==i16::MAX||x==i16::MIN).count();
                assert!(clipped*100<pcm.samples.len(),"{} #{i}: {clipped} clipped",p.display());
                decoded+=1;
            }
        }
        eprintln!("ABKC EA Layer 3: {decoded} sounds decoded exactly, including {mpeg2} MPEG-2 sounds");
        assert_eq!((decoded,mpeg2),(19,12));
    }
    fn music(name:&str)->Option<Vec<u8>>{
        let p=crate::bridge::data_root().join("files").join("data").join("audio").join("music").join(name);
        std::fs::read(p).ok()
    }
    #[test] fn header_of_a_music_track(){
        let Some(d)=music("dartshootout.asf") else{eprintln!("DATA absent; skipped");return};
        let (h,size)=parse_header(&d).unwrap();
        assert_eq!((h.channels,h.sample_rate,h.codec),(2,44100,0x17));assert_eq!(size,40);
        let blocks=data_blocks(&d).unwrap();assert_eq!(blocks.len(),2438);
        let total:usize=blocks.iter().map(|&(s,_)|be32(&d[s..],0).unwrap() as usize).sum();
        eprintln!("header samples {} vs sum of block counts {total}",h.samples);
        assert_eq!(total as u32,h.samples,"block sample counts must add up to the header total");
    }
    #[test] fn decodes_the_start_of_a_track(){
        let Some(d)=music("dartshootout.asf") else{eprintln!("DATA absent; skipped");return};
        let pcm=decode(&d,Some(40)).unwrap();
        let n=pcm.samples.len()/pcm.channels;
        let rms=(pcm.samples.iter().map(|&s|(s as f64).powi(2)).sum::<f64>()/pcm.samples.len() as f64).sqrt();
        let clipped=pcm.samples.iter().filter(|&&s|s==i16::MAX||s==i16::MIN).count();
        eprintln!("{} samples/ch, rms {rms:.0}, clipped {clipped}, frames {}, stats {:?}",n,pcm.frames,pcm.stats);
        assert!(rms>200.&&rms<20000.);
    }

    /// Every music track decodes end to end: the sample count equals the header's, every granule's Huffman data ends
    /// exactly at its declared `part2_3_length`, the output is not clipped and shows no seam at granule boundaries.
    #[test] fn all_music_decodes_exactly(){
        let dir=crate::bridge::data_root().join("files").join("data").join("audio").join("music");
        let Ok(rd)=std::fs::read_dir(&dir) else{eprintln!("DATA absent; skipped");return};
        let (mut tracks,mut granules,mut seconds)=(0,0u64,0f64);
        for e in rd.filter_map(|e|e.ok()){
            let p=e.path();if p.extension().is_none_or(|x|x!="asf"){continue}
            let d=std::fs::read(&p).unwrap();let (h,_)=parse_header(&d).unwrap();
            let pcm=decode(&d,None).unwrap_or_else(|er|panic!("{}: {er}",p.display()));
            let n=pcm.samples.len()/pcm.channels;
            assert_eq!(n as u32,h.samples,"{}: sample count",p.display());
            assert_eq!(pcm.stats.exact_end,pcm.stats.frames,"{}: granules not ending on part2_3_length",p.display());
            assert_eq!(pcm.stats.rewinds+pcm.stats.overrun,0,"{}",p.display());
            let clipped=pcm.samples.iter().filter(|&&s|s==i16::MAX||s==i16::MIN).count();
            assert!((clipped as f64)/(pcm.samples.len() as f64)<1e-3,"{}: {clipped} clipped samples",p.display());
            // Seam check on the left channel: mean |x[n]-x[n-1]| at granule ends vs elsewhere.
            let (mut sb,mut nb,mut so,mut no)=(0f64,0u64,0f64,0u64);
            for k in 1..n{let d=((pcm.samples[k*2] as f64)-(pcm.samples[(k-1)*2] as f64)).abs();if k%576==0{sb+=d;nb+=1}else{so+=d;no+=1}}
            let ratio=(sb/nb as f64)/(so/no as f64);
            assert!(ratio>0.8&&ratio<1.25,"{}: granule seam ratio {ratio:.2}",p.display());
            tracks+=1;granules+=pcm.stats.frames;seconds+=n as f64/pcm.sample_rate as f64;
        }
        eprintln!("{tracks} tracks, {granules} channel-granules, {seconds:.0} s of audio decoded exactly");
        assert_eq!(tracks,14);
    }
    /// Ambience banks hold several streams; those coded with EA Layer 3 decode, the rest are reported as another codec.
    #[test] fn ambience_streams(){
        let dir=crate::bridge::data_root().join("files").join("data").join("audio").join("aems");
        if !dir.exists(){eprintln!("DATA absent; skipped");return}
        let (mut ok,mut other)=(0,0);
        for name in ["amb_natureforest.ast","amb_park.ast","amb_stadium.ast","amb_schoolyard.ast","mg_paperairplanes.ast"]{
            let d=std::fs::read(dir.join(name)).unwrap();
            for (a,b) in streams(&d){
                match decode(&d[a..b],None){Ok(p)=>{assert_eq!(p.stats.exact_end,p.stats.frames,"{name}");ok+=1}Err(e)=>{assert!(e.contains("is not EA Layer 3"),"{name}: {e}");other+=1}}
            }
        }
        eprintln!("{ok} ambience streams decoded, {other} use another codec");
        assert!(ok>=8);
    }

    /// Every EA-XA ambience stream decodes to exactly the header's sample count without clipping or filter blow-up.
    #[test] fn xa_streams(){
        let dir=crate::bridge::data_root().join("files").join("data").join("audio").join("aems");
        if !dir.exists(){eprintln!("DATA absent; skipped");return}
        let mut n=0;let mut seconds=0f64;
        for name in ["amb_natureforest.ast","amb_park.ast","amb_stadium.ast","amb_schoolyard.ast","mg_paperairplanes.ast"]{
            let d=std::fs::read(dir.join(name)).unwrap();
            for (a,b) in streams(&d){
                let (h,_)=parse_header(&d[a..b]).unwrap();if h.codec!=0x0a{continue}
                let p=decode_xa(&d[a..b]).unwrap_or_else(|e|panic!("{name}: {e}"));
                assert_eq!((p.samples.len()/p.channels) as u32,h.samples,"{name}: sample count");
                let clipped=p.samples.iter().filter(|&&s|s==i16::MAX||s==i16::MIN).count();
                assert!((clipped as f64)/(p.samples.len() as f64)<1e-3,"{name}: clipped {clipped}");
                n+=1;seconds+=p.samples.len() as f64/p.sample_rate as f64;
            }
        }
        eprintln!("{n} EA-XA streams, {seconds:.0} s");
        assert!(n>=30);
    }

    /// Every sound of every `.bnk` bank decodes as EA-XA to its header's sample count.
    #[test] fn bank_sounds(){
        let dir=crate::bridge::data_root().join("files").join("data").join("audio").join("banks");
        let Ok(rd)=std::fs::read_dir(&dir) else{eprintln!("DATA absent; skipped");return};
        let (mut banks,mut sounds,mut other)=(0,0,0);
        for e in rd.filter_map(|e|e.ok()){
            let p=e.path();if p.extension().is_none_or(|x|x!="bnk"){continue}
            let d=std::fs::read(&p).unwrap();banks+=1;
            for (i,snd) in parse_bank(&d).unwrap_or_else(|er|panic!("{}: {er}",p.display())).iter().enumerate(){
                if snd.header.codec!=0x0a{other+=1;continue}
                let pcm=decode_bank_sound(&d,i).unwrap_or_else(|er|panic!("{} sound {i}: {er}",p.display()));
                assert_eq!(pcm.samples.len() as u32,snd.header.samples);
                let clipped=pcm.samples.iter().filter(|&&s|s==i16::MAX||s==i16::MIN).count();
                assert!((clipped as f64)/(pcm.samples.len().max(1) as f64)<2e-3,"{} sound {i}: {clipped} clipped",p.display());
                sounds+=1;
            }
        }
        eprintln!("{banks} banks, {sounds} EA-XA sounds decoded, {other} with another codec");
        assert_eq!(banks,10);assert!(sounds>=50);
    }

    /// Every speech file in `spchdat.viv` decodes: chunk framing holds through every block (types 0/0xEE, frames end
    /// on the declared sample counts), the sample total matches the header, the output stays finite and unclipped.
    #[test] fn speech_streams(){
        let p=crate::bridge::data_root().join("files").join("data").join("audio").join("speech").join("spchdat.viv");
        let Ok(bytes)=std::fs::read(&p) else{eprintln!("DATA absent; skipped");return};
        let data=crate::archive::decompress(&bytes).unwrap();
        let entries=crate::archive::entries(&data).unwrap().unwrap();
        let mut n=0;
        for ent in entries.iter().filter(|e|e.name.ends_with(".dat")){
            let d=crate::archive::decompress(&data[ent.offset..ent.offset+ent.size]).unwrap();
            let (h,_)=parse_header(&d).unwrap();
            if h.codec!=0x04{continue}
            let pcm=decode_utk(&d).unwrap_or_else(|e|panic!("{}: {e}",ent.name));
            assert_eq!(pcm.samples.len() as u32,h.samples,"{}",ent.name);
            let rms=(pcm.samples.iter().map(|&s|(s as f64).powi(2)).sum::<f64>()/pcm.samples.len() as f64).sqrt();
            let clipped=pcm.samples.iter().filter(|&&s|s==i16::MAX||s==i16::MIN).count();
            eprintln!("{}: {} samples @ {} Hz, rms {rms:.0}, clipped {clipped}",ent.name,pcm.samples.len(),h.sample_rate);
            // Speech is low-pass; white noise (a mis-framed stream) would give a first-difference ratio near sqrt(2).
            let diff=(pcm.samples.windows(2).map(|w|((w[1]-w[0]) as f64).powi(2)).sum::<f64>()/pcm.samples.len() as f64).sqrt();
            eprintln!("   first-difference / rms = {:.2}",diff/rms);
            assert!(diff/rms<1.0,"{} looks like noise",ent.name);
            assert!(rms>20.&&rms<12000.,"{} rms {rms}",ent.name);
            assert!(clipped*200<pcm.samples.len(),"{} clipped {clipped}",ent.name);
            n+=1;
        }
        assert!(n>0);
    }

    /// Sound banks embedded in the `.abk` module banks: list codecs, decode what has a decoder.
    #[test] fn abk_banks(){
        let dir=crate::bridge::data_root().join("files").join("data").join("audio").join("aems");
        if !dir.exists(){eprintln!("DATA absent; skipped");return}
        let mut codecs=std::collections::BTreeMap::<u8,usize>::new();let (mut ok,mut dsp,mut smooth,mut loud)=(0,0,0,0);
        for e in std::fs::read_dir(&dir).unwrap().flatten(){
            let p=e.path();if p.extension().is_none_or(|x|x!="abk"){continue}
            let d=std::fs::read(&p).unwrap();
            let Some(bank)=abk_bank(&d).unwrap() else{continue};
            let sounds=parse_bank(bank).unwrap();
            for (i,s) in sounds.iter().enumerate(){
                *codecs.entry(s.header.codec).or_default()+=1;
                if matches!(s.header.codec,0x0a|0x12|0x17){let pcm=decode_bank_sound(bank,i).unwrap_or_else(|er|panic!("{} #{i}: {er}",p.display()));assert_eq!(pcm.samples.len(),s.header.samples as usize*pcm.channels);
                    let n=pcm.samples.len() as f64;let rms=(pcm.samples.iter().map(|&x|(x as f64).powi(2)).sum::<f64>()/n).sqrt();
                    let clipped=pcm.samples.iter().filter(|&&x|x==i16::MAX||x==i16::MIN).count();
                    assert!(rms<20000.&&clipped*20<pcm.samples.len(),"{} #{i}: rms {rms:.0} clipped {clipped}",p.display());
                    if s.header.codec==0x12{dsp+=1;if rms>30.{let d=(pcm.samples.windows(2).map(|w|((w[1]-w[0]) as f64).powi(2)).sum::<f64>()/n).sqrt();smooth+=(d/rms<1.2) as usize;loud+=1}}
                    ok+=1}
            }
        }
        eprintln!("abk sound codecs {codecs:?}: {ok} sounds decoded ({dsp} DSP-ADPCM, {smooth}/{loud} audible ones not noise-like)");
        assert!(dsp>200&&smooth*10>=loud*9);
    }
}
