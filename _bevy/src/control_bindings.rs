//! Native controls*.csv loader, checked against Controller::Initialize (0x8032c6c4).
//! The original CSV grammar is unquoted, matches column names, trims each field's
//! left edge and the whole line's right edge. Unknown tokens keep converter defaults.
use crate::controller::Binding;

const ACTIONS:&str=include_str!("../../GameMap/data/enums/input_action_events.tsv");
const STATES:&str=include_str!("../../GameMap/data/enums/input_controller_states.tsv");
const KINDS:&str=include_str!("../../GameMap/data/enums/input_button_event_types.tsv");
const BUTTONS:&str=include_str!("../../GameMap/data/enums/input_buttons.tsv");
const COLUMNS:[&str;7]=["ACTION_EVENT","CONTROLLER_STATE","CONTROLLER_STATE_TRANSITION","CONTROLLER_EVENT","MOD1","MOD2","BUTTON"];
pub const FILES:[&str;14]=["controls.csv","controlsmg21.csv","controlsmgbughunt.csv","controlsmgdartshootout.csv","controlsmgdodgeball.csv","controlsmgdribbling.csv","controlsmgfootie.csv","controlsmgpaperairplanes.csv","controlsmgquickdraw.csv","controlsmgrccars.csv","controlsmgtemplate.csv","controlsmgtetherball.csv","controlsmgwallball.csv","controlsmgfreethrow.csv"];

fn convert(token:&[u8],table:&str,fallback:u32)->u32{
    table.lines().filter(|l|!l.starts_with('#')).find_map(|l|{
        let mut fields=l.split('\t');let number=fields.next()?;let name=fields.next()?;
        (name.as_bytes()==token).then(||number.parse::<u32>().ok()).flatten()
    }).unwrap_or(fallback)
}
fn space(b:u8)->bool{!(0x21..=0x7e).contains(&b)}
fn left(s:&[u8])->&[u8]{&s[s.iter().position(|&b|!space(b)).unwrap_or(s.len())..]}
fn right(s:&[u8])->&[u8]{&s[..s.iter().rposition(|&b|!space(b)).map_or(0,|i|i+1)]}

/// Memory-unsafe original limits become errors: 1024-byte line buffer,
/// 128-byte field buffer, 64 columns, and 512 controller rows. Missing headers
/// likewise return an error instead of the original out-of-bounds column lookup.
pub fn parse(data:&[u8])->Result<Vec<Binding>,String>{
    if data.contains(&0){return Err("controls CSV contains a NUL byte".into())}
    let mut lines=Vec::new();
    for raw in data.split(|&c|c==b'\n'){
        if raw.len()>=1024{return Err("controls CSV line exceeds original buffer".into())}
        let line=right(raw);
        if line.is_empty()||(line.len()>2&&line.starts_with(b"//")){continue}
        let fields:Vec<&[u8]>=line.split(|&c|c==b',').collect();
        if fields.len()>64||fields.iter().any(|s|s.len()>=128){return Err("controls CSV field/column limit exceeded".into())}
        lines.push(fields.into_iter().map(left).collect::<Vec<_>>());
    }
    let header=lines.first().ok_or("empty controls CSV")?;
    let mut columns=[0;7];
    for (i,name) in COLUMNS.iter().enumerate(){columns[i]=header.iter().position(|v|*v==name.as_bytes()).ok_or_else(||format!("missing controls CSV column {name}"))?;}
    if lines.len()>513{return Err("controls CSV has more than 512 rows".into())}
    let mut bindings=Vec::new();
    for row in lines.iter().skip(1){
        let get=|i:usize|row.get(columns[i]).copied().unwrap_or(b"");
        let mut b=Binding{action:convert(get(0),ACTIONS,190),state:convert(get(1),STATES,31),transition:convert(get(2),STATES,31),kind:convert(get(3),KINDS,9),required:[0;2],forbidden:[0;2],button:convert(get(6),BUTTONS,0)};
        for i in 0..2{let v=get(4+i);let value=convert(v,BUTTONS,0);if v.starts_with(b"~"){b.forbidden[i]=value}else{b.required[i]=value}}
        bindings.push(b);
    }
    Ok(bindings)
}

#[cfg(test)]
mod tests{
    use super::*;
    use serde_json::{Value,json};
    fn numbers(rows:&[Binding])->Value{json!(rows.iter().map(|r|json!({"action":r.action,"state":r.state,"transition":r.transition,"kind":r.kind,"required":r.required,"forbidden":r.forbidden,"button":r.button})).collect::<Vec<_>>())}
    #[test]
    fn original_loader_corpus(){
        let gold:Value=serde_json::from_str(include_str!("../tests/data/control_bindings_golden.json")).unwrap();
        for f in gold["synthetic"].as_array().unwrap(){assert_eq!(numbers(&parse(f["input"].as_str().unwrap().as_bytes()).unwrap()),f["rows"],"{}",f["name"]);}
        let archive=crate::bridge::data_root().join("files/data/csvs.viv");
        if !archive.exists(){eprintln!("DATA absent; skipped original controls corpus");return}
        for f in gold["files"].as_array().unwrap(){
            let source=format!("{}::{}",archive.display(),f["name"].as_str().unwrap());
            let (data,_)=crate::archive::read_virtual(&source).unwrap();
            assert_eq!(crate::sha256::hex(&data),f["sha256"].as_str().unwrap(),"{source}");
            assert_eq!(numbers(&parse(&data).unwrap()),f["rows"],"{source}");
        }
    }
    #[test]
    fn unsafe_original_inputs_return_errors(){
        assert!(parse(b"ACTION_EVENT\nEVENT_PLAYER_MOVE\n").is_err());
        assert!(parse(b"\0").is_err());
        assert!(parse(&vec![b'A';1024]).is_err());
    }
}
