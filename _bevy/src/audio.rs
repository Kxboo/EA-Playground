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
pub struct Header{pub samples:u32,pub channels:u8,pub sample_rate:u32,pub codec:u8,pub entries:Vec<(u8,u32)>}

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
    let mut p=4usize;let mut h=Header{channels:1,..Default::default()};
    while p<body.len(){
        let tag=body[p];p+=1;
        match tag{
            0xFF=>break,
            0xFC|0xFD|0xFE=>continue, // stream section delimiters carry no value
            _=>{
                let Some(&n)=body.get(p) else{break};let n=n as usize;p+=1;
                if n>4||p+n>body.len(){ // long entries (e.g. 0x13, speech tables) are skipped by length
                    p=(p+n).min(body.len());continue
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

pub struct Pcm{pub sample_rate:u32,pub channels:usize,pub samples:Vec<i16>,pub frames:u64,pub stats:mp3::Stats}

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
        let (mut o,mut emitted)=(0usize,0usize);let mut samples=Vec::with_capacity(want*ch);
        while emitted<want&&o<buf.len(){
            if buf[o]!=0{return Err(format!("block {bi}: unexpected chunk type {:#04x} at {o}",buf[o]))}
            let f=self.dec.decode_frame(&buf[o+1..]).map_err(|er|format!("block {bi} frame at {o}: {er}"))?;
            if f.channels!=ch{return Err(format!("block {bi}: frame has {} channels, stream header says {ch}",f.channels))}
            self.frames+=1;o+=1+f.bytes;let mut pcm=f.pcm;
            while o+5<=buf.len()&&buf[o]==0xEE{
                let off=u16::from_be_bytes([buf[o+1],buf[o+2]]) as usize;let n=u16::from_be_bytes([buf[o+3],buf[o+4]]) as usize;
                let start=576usize.checked_sub(off).ok_or("raw PCM offset exceeds a granule")?;
                if start+n>576||o+5+n*ch*2>buf.len(){return Err(format!("block {bi}: raw PCM chunk out of range"))}
                for k in 0..n{for c in 0..ch{let p=o+5+(k*ch+c)*2;pcm[c][start+k]=i16::from_be_bytes([buf[p],buf[p+1]]) as f64/32768.;}}
                o+=5+n*ch*2;
            }
            let from=self.skip.min(576);self.skip-=from;
            for i in from..576{
                if emitted>=want{break}
                for c in 0..ch{samples.push(mp3::to_i16(pcm[c][i]));}
                emitted+=1;
            }
        }
        if emitted!=want{return Err(format!("block {bi}: produced {emitted} of {want} samples"))}
        Ok(Some(samples))
    }
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
    let count=u16::from_be_bytes([d[6],d[7]]) as usize;let mut out=vec![];let mut pos=0x14+count*4;
    for _ in 0..count{
        // Sound headers follow the table back to back; each ends with a 0xFF tag and is padded to 4 bytes.
        let body=d.get(pos..).ok_or("sound header outside bank")?;
        let (header,used)=parse_tags_len(body);pos+=(used+3)&!3;
        let data_offset=header.entries.iter().find(|e|e.0==0x88).map(|e|e.1 as usize).ok_or("sound without a data offset")?;
        out.push(BankSound{header,data_offset});
    }
    Ok(out)
}
/// Decode sound `i` of a bank (EA-XA only).
pub fn decode_bank_sound(d:&[u8],i:usize)->Result<Pcm,String>{
    let sounds=parse_bank(d)?;let s=sounds.get(i).ok_or("no such sound")?;
    if s.header.codec!=0x0a{return Err(format!("codec {:#x} is not EA-XA",s.header.codec))}
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
}
