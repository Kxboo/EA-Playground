use std::{io::{BufRead,BufReader,Write},path::{Path,PathBuf},process::{Child,Command,Stdio},sync::{Arc,Mutex,mpsc,atomic::{AtomicU64,Ordering}}};
use bevy::prelude::Resource;
use serde_json::{Value,json};

pub fn root()->PathBuf {
    if let Ok(p)=std::env::var("EAGL_WORKSPACE") {return PathBuf::from(p)}
    let exe=std::env::current_exe().unwrap();
    for p in exe.ancestors().skip(1) {
        if p.join("tools/decoder_bridge.py").exists() || p.join("EAGL-Decoder.exe").exists() {return p.to_path_buf()}
    }
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn command(base:&Path)->Command {
    let packaged=base.join("EAGL-Decoder.exe");
    let mut c=if packaged.exists(){Command::new(packaged)}else{
        let mut c=Command::new("py");c.args(["-3.14","-u"]).arg(base.join("tools/decoder_bridge.py"));c
    };
    c.current_dir(base).stdin(Stdio::piped()).stdout(Stdio::piped());
    #[cfg(windows)] {use std::os::windows::process::CommandExt;c.creation_flags(0x08000000);}
    c
}

pub fn headless(req:Value)->Result<Value,String> {
    let mut child=command(&root()).spawn().map_err(|e|e.to_string())?;
    let mut input=child.stdin.take().unwrap();
    writeln!(input,"{req}").map_err(|e|e.to_string())?;drop(input);
    let mut line=String::new();
    BufReader::new(child.stdout.take().unwrap()).read_line(&mut line).map_err(|e|e.to_string())?;
    let _=child.wait();
    serde_json::from_str(&line).map_err(|e|format!("Decoder protocol: {e}"))
}

#[derive(Resource)]
pub struct Bridge {
    tx:mpsc::Sender<Value>,pub rx:Mutex<mpsc::Receiver<Value>>,
    latest:Arc<AtomicU64>,child:Arc<Mutex<Option<Child>>>,
}
impl Bridge {
    pub fn start()->Self {
        let (tx,requests)=mpsc::channel::<Value>();let (results,rx)=mpsc::channel();
        let latest=Arc::new(AtomicU64::new(0));let newest=latest.clone();
        let child=Arc::new(Mutex::new(None));let process=child.clone();
        std::thread::spawn(move||{
            let run=||->Result<(),String>{
                let base=root();std::fs::create_dir_all(base.join("logs")).ok();
                let log=std::fs::OpenOptions::new().create(true).append(true).open(base.join("logs/decoder.log")).map_err(|e|e.to_string())?;
                let mut p=command(&base).stderr(Stdio::from(log)).spawn().map_err(|e|e.to_string())?;
                let mut input=p.stdin.take().unwrap();let mut output=BufReader::new(p.stdout.take().unwrap());
                *process.lock().unwrap()=Some(p);
                for req in requests {
                    let id=req["id"].as_u64().unwrap_or(0);
                    if id!=0 && id!=newest.load(Ordering::Acquire){continue}
                    writeln!(input,"{req}").map_err(|e|e.to_string())?;input.flush().map_err(|e|e.to_string())?;
                    let mut line=String::new();output.read_line(&mut line).map_err(|e|e.to_string())?;
                    if line.is_empty(){return Err("Decoder stopped; see logs/decoder.log".into())}
                    let response:Value=serde_json::from_str(&line).map_err(|e|e.to_string())?;
                    if id==0 || id==newest.load(Ordering::Acquire){let _=results.send(response);}
                }
                Ok(())
            };
            if let Err(error)=run(){let _=results.send(json!({"id":newest.load(Ordering::Acquire),"ok":false,"error":error}));}
            if let Some(mut p)=process.lock().unwrap().take(){let _=p.kill();let _=p.wait();}
        });
        Self{tx,rx:Mutex::new(rx),latest,child}
    }
    pub fn request(&self,mut req:Value)->u64 {
        let id=self.latest.fetch_add(1,Ordering::AcqRel)+1;req["id"]=json!(id);
        let _=self.tx.send(req);id
    }
    pub fn catalog(&self){let _=self.tx.send(json!({"id":0,"command":"catalog"}));}
}
impl Drop for Bridge {
    fn drop(&mut self){if let Some(mut p)=self.child.lock().unwrap().take(){let _=p.kill();let _=p.wait();}}
}
