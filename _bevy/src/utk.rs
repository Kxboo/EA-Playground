//! EA MicroTalk (UTK) speech codec: `decodemut` (0x80287208), `filter` (0x80286b58), `readsamples` (0x8028692c),
//! `initmut` (0x8028770c) of the game's sound engine, ported operation for operation in single precision (fused
//! multiply-adds where the PowerPC code uses them) with the executable's own tables (mp3_tables.rs).
//!
//! A frame is 432 samples (4 sub-frames of 108).  Bits are read least-significant first.  Per frame: 12 reflection
//! coefficient targets (6,6,6,6 bits then 8 x 5 bits, interpolated over 4 steps), then per sub-frame an 8-bit pitch lag
//! into a 756-float history (324 carried over + the frame so far), a 4-bit adaptive gain, a 6-bit fixed gain index and a
//! 108-sample excitation (2-bit ternary codes or the variable-length pulse code); the sum drives a 12th-order
//! all-pole filter built from the interpolated reflection coefficients.
use crate::mp3_tables::{UTK_COEFF,UTK_DECODE,UTK_INDEX};

pub const FRAME:usize=432;
const SUB:usize=108;

pub struct Utk{
    data:Vec<u8>,ptr:usize,value:u32,count:i32,
    reduced_bw:bool,thresh:i32,gains:[f32;64],rc:[f32;12],hist:[f32;12],
    /// Adaptive-codebook history (last 324 unfiltered samples of the previous frame) and the frame being built.
    adapt:[f32;324],frame:[f32;FRAME],
}

impl Utk{
    /// Start a bitstream at `data[pos]`; `header` reads the 15 stream-header bits (`initmut` with its flag set).
    pub fn new(data:Vec<u8>,pos:usize,header:bool)->Result<Self,String>{
        let mut u=Self{data,ptr:0,value:0,count:8,reduced_bw:false,thresh:0,gains:[0.;64],rc:[0.;12],hist:[0.;12],adapt:[0.;324],frame:[0.;FRAME]};
        u.value=*u.data.get(pos).ok_or("empty UTK data")? as u32;u.ptr=pos+1;
        if header{
            u.reduced_bw=u.bits(1)!=0;
            u.thresh=32-u.bits(4) as i32;
            let base=8.0f32*((u.bits(4)+1) as f32);
            u.gains[0]=base;
            let step=0.001f32.mul_add(u.bits(6) as f32,1.04);
            for i in 0..63{u.gains[i+1]=step*u.gains[i];}
        }
        Ok(u)
    }
    /// Restart the bit reader at a byte position (start of the next frame).
    pub fn seek(&mut self,pos:usize)->Result<(),String>{
        self.value=*self.data.get(pos).ok_or("UTK data ends")? as u32;self.ptr=pos+1;self.count=8;Ok(())
    }
    /// Replace the input (next SCDl block) keeping the decoder state; the caller seeks to the first frame.
    pub fn load(&mut self,data:Vec<u8>){self.data=data}
    /// Index of the byte the reader will fetch next; the following chunk's type byte is at `position() - 1`.
    pub fn position(&self)->usize{self.ptr}

    fn bits(&mut self,n:u32)->u32{
        let v=self.value&((1u32<<n)-1);
        self.value>>=n;self.count-=n as i32;
        if self.count<8{
            let b=self.data.get(self.ptr).copied().unwrap_or(0) as u32;self.ptr+=1;
            self.value|=b<<self.count as u32;self.count+=8;
        }
        v
    }
    fn discard(&mut self,n:u32){self.bits(n);}

    /// `readsamples`: 108 (or 54 with stride 2) excitation values into `out[0], out[stride], ...`.
    fn excitation(&mut self,multipulse:bool,out:&mut [f32],stride:usize){
        let mut pos=0usize;
        if multipulse{
            let mut state=0usize;
            while pos<SUB{
                let code=(self.value&0xff) as usize;
                let idx=UTK_INDEX[state*256+code] as usize;
                let (next,nbits,val)=UTK_DECODE[idx];
                state=next as usize;self.discard(nbits as u32);
                if idx>3{out[pos]=val;pos+=stride}
                else if idx>1{
                    let mut n=(self.bits(6)+7) as usize;
                    if pos+stride*n>SUB{n=(SUB-pos)/stride}
                    for _ in 0..n{out[pos]=0.;pos+=stride}
                }else{
                    let mut m=7i32;while self.bits(1)==1{m+=1}
                    out[pos]=if self.bits(1)==1{m as f32}else{-(m as f32)};pos+=stride;
                }
            }
        }else{
            while pos<SUB{
                match self.value&3{1=>{out[pos]=-2.;self.discard(2)}3=>{out[pos]=2.;self.discard(2)}_=>{out[pos]=0.;self.discard(1)}}
                pos+=stride;
            }
        }
    }

    /// `filter`: reflection coefficients -> direct-form coefficients (the recursion of 0x80286b58), then `count` x 12
    /// samples of the all-pole synthesis filter applied in place to `frame[start..]`.
    fn filter(&mut self,start:usize,count:usize){
        let rc=self.rc;
        let mut t=[0f32;13];let mut a=[0f32;12];let mut b=[0f32;12];
        for j in (0..=10).rev(){t[j+1]=rc[j];}
        t[0]=1.0;
        for i in 0..12{
            let mut f2=(-rc[11])*t[11];
            for j in (0..=10).rev(){
                let f0=t[j];let f1=rc[j];
                f2=(-f1).mul_add(f0,f2);
                t[j+1]=f2.mul_add(f1,f0);
            }
            t[0]=f2;b[i]=f2;
            for k in 0..i{f2=(-a[k]).mul_add(b[i-k-1],f2);}
            a[i]=f2;
        }
        let base=start;
        for blk in 0..count{
            for s in 0..12{
                let n=base+blk*12+s;
                let mut acc=self.frame[n];
                for k in 0..12{acc=a[k].mul_add(self.hist[k],acc);}
                self.frame[n]=acc;
                self.hist.copy_within(0..11,1);self.hist[0]=acc;
            }
        }
    }

    /// Decode one frame; the samples are `frame()`.
    pub fn decode_frame(&mut self)->Result<(),String>{
        let mut delta=[0f32;12];
        let idx=self.bits(6) as i32;
        let x=self.thresh^idx;
        let multipulse=((x>>1)-(x&self.thresh))<0;
        delta[0]=0.25f32*(UTK_COEFF[idx as usize]-self.rc[0]);
        for i in 1..4{let ix=self.bits(6) as usize;delta[i]=0.25f32*(UTK_COEFF[ix]-self.rc[i]);}
        for i in 4..12{let ix=(self.bits(5)+16) as usize;delta[i]=0.25f32*(UTK_COEFF[ix]-self.rc[i]);}
        for k in 0..4{
            let lag=self.bits(8) as i32;
            let base=(k as i32+2)*SUB as i32-lag;
            let adapt=0.066666670143604f32*(self.bits(4) as f32);
            let mut fixed=self.gains[self.bits(6) as usize];
            // exc has 5 floats of padding on each side for the half-band interpolation
            let mut exc=[0f32;5+SUB+5];
            if !self.reduced_bw{
                self.excitation(multipulse,&mut exc[5..5+SUB],1);
            }else{
                let a=self.bits(1) as usize;let b=self.bits(1)!=0;
                self.excitation(multipulse,&mut exc[5+a..5+SUB],2);
                if b{
                    let mut i=0;while i<SUB{exc[5+i+1-a]=0.;i+=2}
                }else{
                    for i in 0..5{exc[i]=0.;exc[5+SUB+i]=0.;}
                    for i in 0..54{
                        let m=5+1-a+2*i;
                        let f0=exc[m-3]+exc[m+3];let f1=exc[m-1]+exc[m+1];let f2=exc[m-5]+exc[m+5];
                        let mut v=-0.1145915612578392f32*f0;
                        v=0.5973859429359436f32.mul_add(f1,v);v=0.018032679334282875f32.mul_add(f2,v);
                        exc[m]=v;
                    }
                    fixed*=0.5;
                }
            }
            for i in 0..SUB{
                let p=base+i as i32;
                let h=if p<0{return Err("UTK pitch lag reaches before the history".into())}else if (p as usize)<324{self.adapt[p as usize]}else{self.frame[p as usize-324]};
                self.frame[k*SUB+i]=fixed.mul_add(exc[5+i],adapt*h);
            }
        }
        // The last 324 (still unfiltered) samples become the next frame's history (memmove at 0x80287530).
        self.adapt.copy_from_slice(&self.frame[SUB..]);
        // Interpolate the reflection coefficients in 4 steps while filtering 12, 12, 12 and 396 samples.
        for (start,count) in [(0usize,1usize),(12,1),(24,1),(36,33)]{
            for i in 0..12{self.rc[i]+=delta[i];}
            self.filter(start,count);
        }
        Ok(())
    }
    pub fn frame(&self)->&[f32;FRAME]{&self.frame}
    /// The output buffer, for the raw-PCM splice of `CMTBLKDec::Decode` (0x80287a08).
    pub fn frame_mut(&mut self)->&mut [f32;FRAME]{&mut self.frame}
}
