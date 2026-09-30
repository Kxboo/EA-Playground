//! EA Layer 3 decoder: the game's `Snd::CEALayer3` (0x802817f0..0x80286758) re-implemented for MPEG-1 streams.
//!
//! EA re-frames MPEG layer III as one granule per frame with no sync word and no bit reservoir:
//!   `[header byte][gr bit][scfsi 4 bits/ch if gr == 1][side info per channel][scale factors + Huffman data per channel]`
//!   padded to a byte boundary.  Header byte: version(2) sample-rate index(2) channel mode(2, 3 = mono) mode extension(2)
//!   (`CEALayer3::ProcessHeader` 0x80282b68).  Side info fields follow `GetSideInfo` 0x802817f0, big-value/count1 decoding
//!   follows `DecodeHuffman` 0x80282628 with the executable's own trees (mp3_tables.rs).  The remaining stages (dequantise,
//!   M/S stereo, reorder, antialias, IMDCT with window switching, frequency inversion, polyphase synthesis) are the
//!   standard layer-III definitions the engine's `CMpegLayer3Base` computes.
//!   The output is signed 16-bit PCM.
use crate::mp3_tables as t;
use std::f64::consts::PI;

struct Bits<'a>{d:&'a [u8],pos:usize}
impl<'a> Bits<'a>{
    fn new(d:&'a [u8])->Self{Self{d,pos:0}}
    fn bit(&mut self)->u32{let b=self.d.get(self.pos>>3).copied().unwrap_or(0);let v=(b>>(7-(self.pos&7)))&1;self.pos+=1;v as u32}
    fn get(&mut self,n:u32)->u32{let mut v=0;for _ in 0..n{v=(v<<1)|self.bit();}v}
    fn overrun(&self)->bool{self.pos>self.d.len()*8}
}

#[derive(Clone,Copy,Default,Debug)]
struct Granule{part23:usize,big_values:usize,global_gain:i32,scalefac_compress:usize,window_switching:bool,block_type:u8,mixed:bool,table_select:[u8;3],subblock_gain:[u8;3],region0:usize,region1:usize,preflag:bool,scalefac_scale:bool,count1table:usize}

pub struct Frame{pub bytes:usize,pub channels:usize,pub pcm:[[f64;576];2],pub granule:u8}

/// Decoder state carried across frames (overlap buffers, scale factors kept for scfsi, synthesis history).
pub struct Layer3{
    scale_l:[[u8;22];2],scale_s:[[[u8;3];13];2],
    overlap:[[[f64;18];32];2],
    v:[[f64;1024];2],
    d:[f64;512],cos_mat:Vec<f64>,imdct_long:Vec<f64>,imdct_short:Vec<f64>,
    pub stats:Stats,
}
#[derive(Default,Debug,Clone)]
pub struct Stats{pub frames:u64,pub exact_end:u64,pub short_end:u64,pub overrun:u64,pub rewinds:u64}

impl Default for Layer3{fn default()->Self{Self::new()}}

fn pow43(i:usize)->f64{(i as f64).powf(4./3.)}

impl Layer3{
    pub fn new()->Self{
        // Synthesis window D[i] = intwin[j]/65536 * (-1)^(i/64), j = i (i <= 256) or 512 - i (see tools/extract_audio_tables.py).
        let mut d=[0f64;512];
        for i in 0..512{let j=if i<=256{i}else{512-i};d[i]=t::SYNTH_WINDOW_INT[j] as f64/65536.*if (i/64)%2==1{-1.}else{1.};}
        let mut cos_mat=vec![0f64;64*32];
        for i in 0..64{for k in 0..32{cos_mat[i*32+k]=((16+i) as f64*(2*k+1) as f64*PI/64.).cos();}}
        let mut imdct_long=vec![0f64;36*18];
        for i in 0..36{for k in 0..18{imdct_long[i*18+k]=(PI/72.*(2*i+1+18) as f64*(2*k+1) as f64).cos();}}
        let mut imdct_short=vec![0f64;12*6];
        for i in 0..12{for k in 0..6{imdct_short[i*6+k]=(PI/24.*(2*i+1+6) as f64*(2*k+1) as f64).cos();}}
        Self{scale_l:[[0;22];2],scale_s:[[[0;3];13];2],overlap:[[[0.;18];32];2],v:[[0.;1024];2],d,cos_mat,imdct_long,imdct_short,stats:Stats::default()}
    }
    pub fn reset(&mut self){let s=std::mem::take(&mut self.stats);*self=Self::new();self.stats=s;}

    /// Decode one EA frame starting at the header byte of `buf`.
    pub fn decode_frame(&mut self,buf:&[u8])->Result<Frame,String>{
        let mut b=Bits::new(buf);
        let hdr=b.get(8);
        let (version,sr,mode,ext)=((hdr>>6)&3,((hdr>>4)&3) as usize,(hdr>>2)&3,hdr&3);
        if version!=3{return Err(format!("unsupported MPEG version bits {version} (header {hdr:#04x})"))}
        if sr==3{return Err("reserved sample-rate index".into())}
        if ext&1!=0{return Err("intensity stereo frames are not implemented".into())}
        let channels=if mode==3{1}else{2};let ms=ext&2!=0&&channels==2;
        let gr=b.get(1) as usize;
        let mut scfsi=[[false;4];2];
        if gr==1{for ch in 0..channels{for band in 0..4{scfsi[ch][band]=b.get(1)==1;}}}
        let mut g=[Granule::default();2];
        for ch in 0..channels{
            let mut x=Granule{part23:b.get(12) as usize,big_values:b.get(9) as usize,global_gain:b.get(8) as i32,scalefac_compress:b.get(4) as usize,window_switching:b.get(1)==1,..Default::default()};
            if x.window_switching{
                x.block_type=b.get(2) as u8;x.mixed=b.get(1)==1;
                x.table_select[0]=b.get(5) as u8;x.table_select[1]=b.get(5) as u8;
                for w in 0..3{x.subblock_gain[w]=b.get(3) as u8;}
                if x.block_type==0{return Err("window switching with block type 0".into())}
                x.region0=if x.block_type==2&&!x.mixed{8}else{7};x.region1=20-x.region0;
            }else{
                for i in 0..3{x.table_select[i]=b.get(5) as u8;}
                x.region0=b.get(4) as usize;x.region1=b.get(3) as usize;
            }
            x.preflag=b.get(1)==1;x.scalefac_scale=b.get(1)==1;x.count1table=b.get(1) as usize;
            if x.big_values>288{return Err("big_values exceeds 288".into())}
            g[ch]=x;
        }
        let mut xr=[[0f64;576];2];
        for ch in 0..channels{
            let start=b.pos;let end=start+g[ch].part23;
            self.read_scale_factors(&mut b,ch,&g[ch],gr,&scfsi[ch])?;
            let counts=self.huffman(&mut b,&g[ch],end,sr,&mut xr[ch])?;
            self.stats.frames+=1;
            if b.pos==end{self.stats.exact_end+=1}else if b.pos<end{self.stats.short_end+=1;let skip=(end-b.pos) as u32;for _ in 0..skip{b.bit();}}
            let _=counts;
            self.dequantise(ch,&g[ch],sr,&mut xr[ch]);
        }
        if b.overrun(){self.stats.overrun+=1;return Err("frame runs past the end of the data".into())}
        if ms{let k=std::f64::consts::FRAC_1_SQRT_2;for i in 0..576{let (m,s)=(xr[0][i],xr[1][i]);xr[0][i]=(m+s)*k;xr[1][i]=(m-s)*k;}}
        let mut pcm=[[0f64;576];2];
        for ch in 0..channels{
            let mut spec=xr[ch];
            if g[ch].window_switching&&g[ch].block_type==2{self.reorder(&g[ch],sr,&mut spec)}
            if !(g[ch].window_switching&&g[ch].block_type==2&&!g[ch].mixed){self.antialias(&g[ch],&mut spec)}
            let sb=self.hybrid(ch,&g[ch],&spec);
            self.synthesise(ch,&sb,&mut pcm[ch]);
        }
        let bits=b.pos;let bytes=bits.div_ceil(8);
        Ok(Frame{bytes,channels,pcm,granule:gr as u8})
    }

    fn read_scale_factors(&mut self,b:&mut Bits,ch:usize,g:&Granule,gr:usize,scfsi:&[bool;4])->Result<(),String>{
        let (slen1,slen2)=(t::SLEN[g.scalefac_compress] as u32,t::SLEN[16+g.scalefac_compress] as u32);
        if g.window_switching&&g.block_type==2{
            self.scale_l[ch]=[0;22];
            if g.mixed{
                for s in 0..8{self.scale_l[ch][s]=b.get(slen1) as u8;}
                for s in 3..6{for w in 0..3{self.scale_s[ch][s][w]=b.get(slen1) as u8;}}
                for s in 6..12{for w in 0..3{self.scale_s[ch][s][w]=b.get(slen2) as u8;}}
            }else{
                for s in 0..6{for w in 0..3{self.scale_s[ch][s][w]=b.get(slen1) as u8;}}
                for s in 6..12{for w in 0..3{self.scale_s[ch][s][w]=b.get(slen2) as u8;}}
            }
            for w in 0..3{self.scale_s[ch][12][w]=0;}
        }else{
            self.scale_s[ch]=[[0;3];13];
            let groups=[(0usize,6usize,slen1),(6,11,slen1),(11,16,slen2),(16,21,slen2)];
            for (gi,&(lo,hi,slen)) in groups.iter().enumerate(){
                if gr==1&&scfsi[gi]{continue} // reuse the scale factors kept from the first granule
                for s in lo..hi{self.scale_l[ch][s]=b.get(slen) as u8;}
            }
            self.scale_l[ch][21]=0;
        }
        Ok(())
    }

    /// Big values (executable's trees), then count1 quadruples; returns the number of lines decoded.
    fn huffman(&mut self,b:&mut Bits,g:&Granule,end:usize,sr:usize,xr:&mut [f64;576])->Result<usize,String>{
        let gain=2f64.powf((g.global_gain-210) as f64/4.);let _=gain;
        let short=g.window_switching&&g.block_type==2;
        let (r1,r2)=if short{(36,576)}else{(t::SFB_LONG[sr][g.region0+1] as usize,t::SFB_LONG[sr][(g.region0+g.region1+2).min(22)] as usize)};
        let mut i=0usize;let bv=g.big_values*2;
        while i<bv{
            let sel=if i<r1{g.table_select[0]}else if i<r2{g.table_select[1]}else{g.table_select[2]} as usize;
            let (x,y)=match t::HUFF_TABLES[sel]{
                None=>(0i32,0i32),
                Some(tree)=>{
                    let mut p=0usize;
                    let v=loop{let v=*tree.get(p).ok_or("Huffman tree overrun")?;p+=1;if v>=0{break v}if b.bit()==1{p+=(-v) as usize}};
                    let (mut x,mut y)=((v>>4) as i32,(v&15) as i32);let lin=t::HUFF_LINBITS[sel] as u32;
                    if x==15&&lin>0{x+=b.get(lin) as i32}
                    if x!=0&&b.bit()==1{x=-x}
                    if y==15&&lin>0{y+=b.get(lin) as i32}
                    if y!=0&&b.bit()==1{y=-y}
                    (x,y)
                }
            };
            xr[i]=x as f64;xr[i+1]=y as f64;i+=2;
            if b.overrun(){return Err("Huffman data runs past the end".into())}
        }
        // count1 region: 4-bit values of magnitude 0/1 (executable's lookup tables: index = top bits, entry = [flags, length]).
        let (tab,shift):(&[u8],u32)=if g.count1table==0{(&t::COUNT1_TABLE_0,t::COUNT1_SHIFT_0)}else{(&t::COUNT1_TABLE_1,t::COUNT1_SHIFT_1)};
        let idx_bits=32-shift;
        while b.pos<end&&i+4<=576{
            let save=b.pos;let idx={let mut c=Bits{d:b.d,pos:b.pos};c.get(idx_bits) as usize};
            let (flags,len)=(tab[idx*2],tab[idx*2+1] as usize);
            b.pos+=len;
            for (k,mask) in [8u8,4,2,1].iter().enumerate(){
                if flags&mask!=0{xr[i+k]=if b.bit()==1{-1.}else{1.}}
            }
            if b.pos>end{ // last quadruple overshoots the granule: drop it (as `RewindBits` does)
                b.pos=save;self.stats.rewinds+=1;for k in 0..4{xr[i+k]=0.}
                break;
            }
            i+=4;
        }
        Ok(i)
    }

    fn dequantise(&self,ch:usize,g:&Granule,sr:usize,xr:&mut [f64;576]){
        let mult=if g.scalefac_scale{1.}else{0.5};
        let pw=|v:f64|(v).powf(4./3.);let _=pw;
        let mag=|x:f64|{let a=x.abs();let p=pow43(a as usize);if x<0.{-p}else{p}};
        if g.window_switching&&g.block_type==2{
            let sfb=&t::SFB_SHORT[sr];
            let start_sfb=if g.mixed{3}else{0};
            if g.mixed{
                // first two subbands are long blocks (long bands 0..7 of the sample-rate table)
                for s in 0..8{
                    let e=2f64.powf((g.global_gain-210) as f64/4.-mult*self.scale_l[ch][s] as f64);
                    for i in t::SFB_LONG[sr][s] as usize..t::SFB_LONG[sr][s+1] as usize{xr[i]=mag(xr[i])*e;}
                }
            }
            let mut pos=if g.mixed{36}else{0};
            for s in start_sfb..13{
                let width=(sfb[s+1]-sfb[s]) as usize;
                for w in 0..3{
                    let e=2f64.powf((g.global_gain-210) as f64/4.-2.*g.subblock_gain[w] as f64-mult*if s<12{self.scale_s[ch][s][w] as f64}else{0.});
                    for i in 0..width{if pos<576{xr[pos]=mag(xr[pos])*e;}pos+=1;}
                }
            }
        }else{
            for s in 0..22{
                let sf=self.scale_l[ch][s] as f64+if g.preflag{t::PRETAB[s] as f64}else{0.};
                let e=2f64.powf((g.global_gain-210) as f64/4.-mult*sf);
                for i in t::SFB_LONG[sr][s] as usize..(t::SFB_LONG[sr][s+1] as usize).min(576){xr[i]=mag(xr[i])*e;}
            }
        }
    }

    /// Short blocks: bitstream order (sfb, window, line) -> interleaved (line, window) order used by the IMDCT.
    fn reorder(&self,g:&Granule,sr:usize,xr:&mut [f64;576]){
        let sfb=&t::SFB_SHORT[sr];let mut out=*xr;
        let start_sfb=if g.mixed{3}else{0};let base=if g.mixed{36}else{0};
        let mut pos=base;
        for s in start_sfb..13{
            let width=(sfb[s+1]-sfb[s]) as usize;
            for w in 0..3{for f in 0..width{
                let dst=(sfb[s] as usize)*3+w+f*3;
                if pos<576&&dst<576{out[dst]=xr[pos]}
                pos+=1;
            }}
        }
        *xr=out;
    }

    fn antialias(&self,g:&Granule,xr:&mut [f64;576]){
        const C:[f64;8]=[-0.6,-0.535,-0.33,-0.185,-0.095,-0.041,-0.0142,-0.0037];
        let limit=if g.window_switching&&g.block_type==2&&g.mixed{1}else{31};
        for sb in 0..limit{
            for i in 0..8{
                let (cs,ca)={let c=C[i];let n=(1.+c*c).sqrt();(1./n,c/n)};
                let (u,d)=(xr[18*sb+17-i],xr[18*(sb+1)+i]);
                xr[18*sb+17-i]=u*cs-d*ca;xr[18*(sb+1)+i]=d*cs+u*ca;
            }
        }
    }

    /// IMDCT + windowing + overlap-add + frequency inversion; returns 18 time slots x 32 subbands.
    fn hybrid(&mut self,ch:usize,g:&Granule,xr:&[f64;576])->[[f64;32];18]{
        let mut out=[[0f64;32];18];
        for sb in 0..32{
            let line=&xr[18*sb..18*sb+18];
            let bt=if g.window_switching{if g.mixed&&sb<2{0}else{g.block_type}}else{0};
            let mut y=[0f64;36];
            if bt==2{
                for w in 0..3{for i in 0..12{
                    let mut s=0.;for k in 0..6{s+=line[3*k+w]*self.imdct_short[i*6+k];}
                    y[6+6*w+i]+=s*(PI/12.*(i as f64+0.5)).sin();
                }}
            }else{
                for i in 0..36{
                    let mut s=0.;for k in 0..18{s+=line[k]*self.imdct_long[i*18+k];}
                    let w=match bt{
                        0=>(PI/36.*(i as f64+0.5)).sin(),
                        1=>if i<18{(PI/36.*(i as f64+0.5)).sin()}else if i<24{1.}else if i<30{(PI/12.*(i as f64-18.+0.5)).sin()}else{0.},
                        _=>if i<6{0.}else if i<12{(PI/12.*(i as f64-6.+0.5)).sin()}else if i<18{1.}else{(PI/36.*(i as f64+0.5)).sin()},
                    };
                    y[i]=s*w;
                }
            }
            for i in 0..18{
                let mut v=y[i]+self.overlap[ch][sb][i];
                if sb%2==1&&i%2==1{v=-v}
                out[i][sb]=v;self.overlap[ch][sb][i]=y[i+18];
            }
        }
        out
    }

    /// ISO polyphase synthesis filterbank: 18 slots of 32 subband samples -> 576 samples.
    fn synthesise(&mut self,ch:usize,sb:&[[f64;32];18],out:&mut [f64;576]){
        for slot in 0..18{
            let v=&mut self.v[ch];
            v.copy_within(0..960,64);
            for i in 0..64{let mut s=0.;for k in 0..32{s+=self.cos_mat[i*32+k]*sb[slot][k];}v[i]=s;}
            let mut u=[0f64;512];
            for i in 0..8{for j in 0..32{u[i*64+j]=v[i*128+j];u[i*64+32+j]=v[i*128+96+j];}}
            for j in 0..32{
                let mut s=0.;for i in 0..16{s+=u[j+32*i]*self.d[j+32*i];}
                out[slot*32+j]=s;
            }
        }
    }
}

/// Clamp to signed 16-bit PCM.
pub fn to_i16(x:f64)->i16{(x*32768.).round().clamp(-32768.,32767.) as i16}
