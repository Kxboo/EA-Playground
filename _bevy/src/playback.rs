//! Streaming PCM playback through the Windows `winmm` wave-out API (no audio crate is needed).
//! A worker thread decodes the EA Layer 3 stream block by block, keeps a few buffers queued and loops at the end.
//! On other platforms (or without an audio device) playback silently does nothing; `Status` reports what happened.
use crate::audio::StreamDecoder;
use std::sync::{atomic::{AtomicBool,AtomicU64,AtomicU8,Ordering},Arc};

#[derive(Default)]
pub struct Status{pub state:AtomicU8,pub samples_played:AtomicU64,pub loops:AtomicU64}
impl Status{
    pub const IDLE:u8=0;pub const PLAYING:u8=1;pub const NO_DEVICE:u8=2;pub const ERROR:u8=3;pub const STOPPED:u8=4;
    pub fn describe(&self)->&'static str{match self.state.load(Ordering::Relaxed){1=>"playing",2=>"no audio device",3=>"decode error",4=>"stopped",_=>"starting"}}
}

pub struct Music{stop:Arc<AtomicBool>,pub status:Arc<Status>,handle:Option<std::thread::JoinHandle<()>>}
impl Music{
    /// Start looping the `.asf` file at `path` (read on the worker thread); `volume` is 0..1 (applied to the samples).
    pub fn start(path:std::path::PathBuf,volume:f32)->Music{
        let stop=Arc::new(AtomicBool::new(false));let status=Arc::new(Status::default());
        let (s2,st2)=(stop.clone(),status.clone());
        let handle=std::thread::Builder::new().name("music".into()).spawn(move||run(path,volume,s2,st2)).ok();
        Music{stop,status,handle}
    }
}
impl Drop for Music{
    fn drop(&mut self){self.stop.store(true,Ordering::Relaxed);if let Some(h)=self.handle.take(){let _=h.join();}}
}

#[cfg(windows)]
mod sys{
    use std::ffi::c_void;
    #[repr(C)] pub struct WaveFormatEx{pub tag:u16,pub channels:u16,pub rate:u32,pub bytes_per_sec:u32,pub block_align:u16,pub bits:u16,pub size:u16}
    #[repr(C)] pub struct WaveHdr{pub data:*mut u8,pub len:u32,pub recorded:u32,pub user:usize,pub flags:u32,pub loops:u32,pub next:*mut WaveHdr,pub reserved:usize}
    pub const WAVE_MAPPER:u32=0xFFFF_FFFF;pub const WHDR_DONE:u32=1;
    #[link(name="winmm")]
    unsafe extern "system"{
        pub fn waveOutOpen(h:*mut *mut c_void,dev:u32,fmt:*const WaveFormatEx,cb:usize,inst:usize,flags:u32)->u32;
        pub fn waveOutPrepareHeader(h:*mut c_void,hdr:*mut WaveHdr,size:u32)->u32;
        pub fn waveOutUnprepareHeader(h:*mut c_void,hdr:*mut WaveHdr,size:u32)->u32;
        pub fn waveOutWrite(h:*mut c_void,hdr:*mut WaveHdr,size:u32)->u32;
        pub fn waveOutReset(h:*mut c_void)->u32;
        pub fn waveOutClose(h:*mut c_void)->u32;
    }
}

#[cfg(windows)]
fn run(path:std::path::PathBuf,volume:f32,stop:Arc<AtomicBool>,status:Arc<Status>){
    use std::{ffi::c_void,time::Duration};
    let Ok(stream)=std::fs::read(&path) else{status.state.store(Status::ERROR,Ordering::Relaxed);return};
    let mut dec=match StreamDecoder::new(stream){Ok(d)=>d,Err(_)=>{status.state.store(Status::ERROR,Ordering::Relaxed);return}};
    let ch=dec.channels();
    let fmt=sys::WaveFormatEx{tag:1,channels:ch as u16,rate:dec.header.sample_rate,bytes_per_sec:dec.header.sample_rate*ch as u32*2,block_align:(ch*2) as u16,bits:16,size:0};
    let mut h:*mut c_void=std::ptr::null_mut();
    if unsafe{sys::waveOutOpen(&mut h,sys::WAVE_MAPPER,&fmt,0,0,0)}!=0{status.state.store(Status::NO_DEVICE,Ordering::Relaxed);return}
    const N:usize=4;let mut bufs:Vec<Vec<i16>>=(0..N).map(|_|Vec::new()).collect();
    let mut hdrs:Vec<sys::WaveHdr>=(0..N).map(|_|sys::WaveHdr{data:std::ptr::null_mut(),len:0,recorded:0,user:0,flags:sys::WHDR_DONE,loops:0,next:std::ptr::null_mut(),reserved:0}).collect();
    let hs=std::mem::size_of::<sys::WaveHdr>() as u32;let mut prepared=[false;N];
    let mut pending:Vec<i16>=Vec::new();let mut eof=false;
    status.state.store(Status::PLAYING,Ordering::Relaxed);
    while !stop.load(Ordering::Relaxed){
        for i in 0..N{
            if hdrs[i].flags&sys::WHDR_DONE==0{continue}
            if prepared[i]{unsafe{sys::waveOutUnprepareHeader(h,&mut hdrs[i],hs);}prepared[i]=false;}
            // refill ~0.25 s
            let target=dec.header.sample_rate as usize*ch/4;
            while pending.len()<target&&!eof{
                match dec.next_block(){
                    Ok(Some(b))=>pending.extend(b),
                    Ok(None)=>{dec.rewind();status.loops.fetch_add(1,Ordering::Relaxed);}
                    Err(_)=>{status.state.store(Status::ERROR,Ordering::Relaxed);eof=true}
                }
            }
            if pending.is_empty(){continue}
            let take=pending.len().min(target);
            bufs[i].clear();bufs[i].extend(pending.drain(..take).map(|s|(s as f32*volume) as i16));
            hdrs[i]=sys::WaveHdr{data:bufs[i].as_mut_ptr() as *mut u8,len:(bufs[i].len()*2) as u32,recorded:0,user:0,flags:0,loops:0,next:std::ptr::null_mut(),reserved:0};
            unsafe{
                if sys::waveOutPrepareHeader(h,&mut hdrs[i],hs)==0{prepared[i]=true;
                    if sys::waveOutWrite(h,&mut hdrs[i],hs)==0{status.samples_played.fetch_add((take/ch) as u64,Ordering::Relaxed);}else{hdrs[i].flags=sys::WHDR_DONE}
                }
            }
        }
        std::thread::sleep(Duration::from_millis(15));
    }
    unsafe{sys::waveOutReset(h);for i in 0..N{if prepared[i]{sys::waveOutUnprepareHeader(h,&mut hdrs[i],hs);}}sys::waveOutClose(h);}
    status.state.store(Status::STOPPED,Ordering::Relaxed);
}

/// Play decoded PCM once on the default device and return when it has finished (call from a worker thread).
#[cfg(windows)]
pub fn play_once(pcm:&crate::audio::Pcm,volume:f32){
    use std::{ffi::c_void,time::Duration};
    let ch=pcm.channels.max(1);
    let fmt=sys::WaveFormatEx{tag:1,channels:ch as u16,rate:pcm.sample_rate,bytes_per_sec:pcm.sample_rate*ch as u32*2,block_align:(ch*2) as u16,bits:16,size:0};
    let mut h:*mut c_void=std::ptr::null_mut();
    if unsafe{sys::waveOutOpen(&mut h,sys::WAVE_MAPPER,&fmt,0,0,0)}!=0{return}
    let mut buf:Vec<i16>=pcm.samples.iter().map(|s|(*s as f32*volume) as i16).collect();
    let hs=std::mem::size_of::<sys::WaveHdr>() as u32;
    let mut hdr=sys::WaveHdr{data:buf.as_mut_ptr() as *mut u8,len:(buf.len()*2) as u32,recorded:0,user:0,flags:0,loops:0,next:std::ptr::null_mut(),reserved:0};
    unsafe{
        if sys::waveOutPrepareHeader(h,&mut hdr,hs)==0{
            if sys::waveOutWrite(h,&mut hdr,hs)==0{
                let mut waited=0;
                while hdr.flags&sys::WHDR_DONE==0&&waited<20_000{std::thread::sleep(Duration::from_millis(20));waited+=20;}
            }
            sys::waveOutReset(h);sys::waveOutUnprepareHeader(h,&mut hdr,hs);
        }
        sys::waveOutClose(h);
    }
}
#[cfg(not(windows))]
pub fn play_once(_pcm:&crate::audio::Pcm,_volume:f32){}

#[cfg(not(windows))]
fn run(_path:std::path::PathBuf,_volume:f32,_stop:Arc<AtomicBool>,status:Arc<Status>){status.state.store(Status::NO_DEVICE,Ordering::Relaxed);}

/// A sound looping until dropped; its playback rate (pitch) and volume can change while it plays.
pub struct Loop{stop:std::sync::Arc<std::sync::atomic::AtomicBool>,rate:std::sync::Arc<std::sync::atomic::AtomicU32>,volume:std::sync::Arc<std::sync::atomic::AtomicU32>}
impl Loop{
    pub fn set(&self,rate:f32,volume:f32){
        use std::sync::atomic::Ordering;
        self.rate.store(rate.clamp(0.25,4.).to_bits(),Ordering::Relaxed);self.volume.store(volume.clamp(0.,2.).to_bits(),Ordering::Relaxed);
    }
}
impl Drop for Loop{fn drop(&mut self){self.stop.store(true,std::sync::atomic::Ordering::Relaxed);}}

/// Start looping `pcm` (resampled in software in short chunks, so rate and volume follow [`Loop::set`]).
pub fn play_loop(pcm:std::sync::Arc<crate::audio::Pcm>,rate:f32,volume:f32)->Loop{
    use std::sync::{Arc,atomic::{AtomicBool,AtomicU32}};
    let l=Loop{stop:Arc::new(AtomicBool::new(false)),rate:Arc::new(AtomicU32::new(rate.to_bits())),volume:Arc::new(AtomicU32::new(volume.to_bits()))};
    let (stop,r,v)=(l.stop.clone(),l.rate.clone(),l.volume.clone());
    std::thread::spawn(move||loop_thread(&pcm,&stop,&r,&v));
    l
}
#[cfg(windows)]
fn loop_thread(pcm:&crate::audio::Pcm,stop:&std::sync::atomic::AtomicBool,rate:&std::sync::atomic::AtomicU32,volume:&std::sync::atomic::AtomicU32){
    use std::{ffi::c_void,sync::atomic::Ordering,time::Duration};
    let ch=pcm.channels.max(1);
    let frames=pcm.samples.len()/ch;
    if frames==0||std::env::args().any(|a|a=="--mute"){return}
    let fmt=sys::WaveFormatEx{tag:1,channels:ch as u16,rate:pcm.sample_rate,bytes_per_sec:pcm.sample_rate*ch as u32*2,block_align:(ch*2) as u16,bits:16,size:0};
    let mut h:*mut c_void=std::ptr::null_mut();
    if unsafe{sys::waveOutOpen(&mut h,sys::WAVE_MAPPER,&fmt,0,0,0)}!=0{return}
    let chunk=(pcm.sample_rate as usize/20).max(64); // 50 ms
    let hs=std::mem::size_of::<sys::WaveHdr>() as u32;
    let mut bufs=[vec![0i16;chunk*ch],vec![0i16;chunk*ch]];
    let mut hdrs:[sys::WaveHdr;2]=[sys::WaveHdr{data:std::ptr::null_mut(),len:0,recorded:0,user:0,flags:sys::WHDR_DONE,loops:0,next:std::ptr::null_mut(),reserved:0},sys::WaveHdr{data:std::ptr::null_mut(),len:0,recorded:0,user:0,flags:sys::WHDR_DONE,loops:0,next:std::ptr::null_mut(),reserved:0}];
    let mut pos=0f64;
    let mut prepared=[false;2];
    'outer: while !stop.load(Ordering::Relaxed){
        for i in 0..2{
            if hdrs[i].flags&sys::WHDR_DONE==0{continue}
            if prepared[i]{unsafe{sys::waveOutUnprepareHeader(h,&mut hdrs[i],hs);}prepared[i]=false;}
            if stop.load(Ordering::Relaxed){break 'outer}
            let (r,vol)=(f32::from_bits(rate.load(Ordering::Relaxed)) as f64,f32::from_bits(volume.load(Ordering::Relaxed)));
            for f in 0..chunk{
                let k=pos as usize%frames;
                for c in 0..ch{bufs[i][f*ch+c]=(pcm.samples[k*ch+c] as f32*vol).clamp(-32768.,32767.) as i16;}
                pos=(pos+r)%frames as f64;
            }
            hdrs[i]=sys::WaveHdr{data:bufs[i].as_mut_ptr() as *mut u8,len:(chunk*ch*2) as u32,recorded:0,user:0,flags:0,loops:0,next:std::ptr::null_mut(),reserved:0};
            unsafe{
                if sys::waveOutPrepareHeader(h,&mut hdrs[i],hs)!=0{break 'outer}
                prepared[i]=true;
                if sys::waveOutWrite(h,&mut hdrs[i],hs)!=0{break 'outer}
            }
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    unsafe{
        sys::waveOutReset(h);
        for i in 0..2{if prepared[i]{sys::waveOutUnprepareHeader(h,&mut hdrs[i],hs);}}
        sys::waveOutClose(h);
    }
}
#[cfg(not(windows))]
fn loop_thread(_pcm:&crate::audio::Pcm,_stop:&std::sync::atomic::AtomicBool,_rate:&std::sync::atomic::AtomicU32,_volume:&std::sync::atomic::AtomicU32){}
