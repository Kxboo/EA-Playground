//! Conversation trees (`conversation/*.con`) - the game's `Conversation::Load` (0x802f2710) and the node loaders
//! `ConversationRoot/Dialog/Response::Load` (0x802f22a8, 0x802f23e4, 0x802f25f8).
//!
//! Layout (little-endian ints via `geti`, single bytes via `ReadByte`):
//!   byte flag; byte name_len; name bytes; byte roots; byte dialogs; byte responses;
//!   then `roots` x Root, `dialogs` x Dialog, `responses` x Response.
//!   Root / Dialog: u32 text id (hash of the locale key), i32 value, byte n, n child-id bytes.
//!   Response:      u32 text id, i32 value, byte action.
//! Text ids are looked up with `Locale::GetString(int)`, i.e. by the key hash in `string.idx`.
use crate::locale::Locale;

/// `ConversationResponse::GetText`: ids 1..=6 are fixed labels, the ASCII table at 0x804ccefc (read from the executable).
pub const RESPONSE_LABELS:[&str;6]=["Yes","No","DONE","GAUNTLET","STORE","Next"];

#[derive(Debug,Clone)] pub struct Node{pub text_id:u32,pub value:i32,pub children:Vec<u8>}
#[derive(Debug,Clone)] pub struct Response{pub text_id:u32,pub value:i32,pub action:i8}
#[derive(Debug,Clone)]
pub struct Conversation{pub flag:i8,pub name:String,pub roots:Vec<Node>,pub dialogs:Vec<Node>,pub responses:Vec<Response>,pub trailing:usize}

struct Rd<'a>{d:&'a [u8],p:usize}
impl<'a> Rd<'a>{
    fn byte(&mut self)->Result<u8,String>{let b=*self.d.get(self.p).ok_or("conversation truncated")?;self.p+=1;Ok(b)}
    fn u32(&mut self)->Result<u32,String>{let s=self.d.get(self.p..self.p+4).ok_or("conversation truncated")?;self.p+=4;Ok(u32::from_le_bytes(s.try_into().unwrap()))}
    fn node(&mut self)->Result<Node,String>{
        let (text_id,value)=(self.u32()?,self.u32()? as i32);let n=self.byte()? as i8;
        let children=(0..n.max(0)).map(|_|self.byte()).collect::<Result<_,_>>()?;
        Ok(Node{text_id,value,children})
    }
}

pub fn parse(d:&[u8])->Result<Conversation,String>{
    let mut r=Rd{d,p:0};
    let flag=r.byte()? as i8;let len=r.byte()? as usize;
    let name=String::from_utf8_lossy(d.get(r.p..r.p+len).ok_or("conversation truncated")?).into_owned();r.p+=len;
    let (nr,nd,nq)=(r.byte()? as i8,r.byte()? as i8,r.byte()? as i8);
    let roots=(0..nr.max(0)).map(|_|r.node()).collect::<Result<Vec<_>,_>>()?;
    let dialogs=(0..nd.max(0)).map(|_|r.node()).collect::<Result<Vec<_>,_>>()?;
    let responses=(0..nq.max(0)).map(|_|Ok(Response{text_id:r.u32()?,value:r.u32()? as i32,action:r.byte()? as i8})).collect::<Result<Vec<_>,String>>()?;
    Ok(Conversation{flag,name,roots,dialogs,responses,trailing:d.len()-r.p})
}

impl Conversation{
    /// Readable outline: every node's text (or its hash when the locale has no entry).
    pub fn outline(&self,loc:&Locale)->String{
        let t=|id:u32|loc.get_hash(id).map(str::to_owned).unwrap_or_else(||format!("<{id:#010x}>"));
        let mut s=format!("{} (flag {})\n",self.name,self.flag);
        for (i,n) in self.roots.iter().enumerate(){s+=&format!("  root {i}: {}  -> children {:?}\n",t(n.text_id),n.children);}
        for (i,n) in self.dialogs.iter().enumerate(){s+=&format!("  dialog {i}: {}  -> children {:?}\n",t(n.text_id),n.children);}
        for (i,q) in self.responses.iter().enumerate(){s+=&format!("  response {i}: {}  (value {}, action {})\n",t(q.text_id),q.value,q.action);}
        s
    }
}

#[cfg(test)]
mod tests{
    use super::*;
    #[test] fn rejects_truncated(){assert!(parse(&[0,9,b'K']).is_err())}
    /// Every conversation file parses exactly, and (with the US locale) node texts resolve.
    #[test] fn all_conversation_files_decode(){
        let root=crate::bridge::data_root().join("files").join("data");
        let viv=root.join("conversation").join("conversation.viv");
        let Ok(bytes)=std::fs::read(&viv) else{eprintln!("DATA absent; skipped");return};
        let data=crate::archive::decompress(&bytes).unwrap();
        let entries=crate::archive::entries(&data).unwrap().unwrap();
        let loc=Locale::parse(&std::fs::read(root.join("locale").join("eng_us.loc")).unwrap(),&std::fs::read(root.join("locale").join("string.idx")).unwrap()).unwrap();
        let (mut files,mut nodes,mut resolved,mut trailing)=(0,0,0,0);
        for e in entries.iter().filter(|e|e.name.to_lowercase().ends_with(".con")){
            let c=parse(&crate::archive::decompress(&data[e.offset..e.offset+e.size]).unwrap()).unwrap_or_else(|er|panic!("{}: {er}",e.name));
            files+=1;trailing+=c.trailing;
            for n in c.roots.iter().chain(c.dialogs.iter()){nodes+=1;if loc.get_hash(n.text_id).is_some(){resolved+=1}}
            if e.name=="AbbeyConversation.con"{eprintln!("{}",c.outline(&loc));}
        }
        eprintln!("{files} conversation files, {nodes} text nodes ({resolved} resolve in eng_us.loc), {trailing} trailing bytes");
        assert_eq!(files,32);assert!(resolved*2>nodes,"most node text ids must resolve");
    }
}
