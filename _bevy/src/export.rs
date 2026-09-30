//! File writers for decoded data: PNG images, binary glTF (models with embedded textures, skeletons with every
//! animation clip) and Wavefront OBJ (collision).  No decoding happens here; inputs are the decoders' own outputs.
use crate::{anim,assets::{AlphaKind,BuiltModel,Wrap},skeleton::Skeleton};
use serde_json::{json,Value};

pub fn png(rgba:&[u8],w:usize,h:usize)->Result<Vec<u8>,String>{
    let mut out=vec![];
    {
        let mut e=png::Encoder::new(&mut out,w as u32,h as u32);
        e.set_color(png::ColorType::Rgba);e.set_depth(png::BitDepth::Eight);
        let mut wr=e.write_header().map_err(|e|e.to_string())?;
        wr.write_image_data(rgba).map_err(|e|e.to_string())?;
    }
    Ok(out)
}

/// Accumulates a glTF binary chunk plus accessor/bufferView JSON.
struct Glb{bin:Vec<u8>,views:Vec<Value>,accessors:Vec<Value>}
impl Glb{
    fn new()->Self{Glb{bin:vec![],views:vec![],accessors:vec![]}}
    fn align(&mut self){while self.bin.len()%4!=0{self.bin.push(0)}}
    fn view(&mut self,bytes:&[u8],target:Option<u32>)->usize{
        self.align();let off=self.bin.len();self.bin.extend_from_slice(bytes);
        let mut v=json!({"buffer":0,"byteOffset":off,"byteLength":bytes.len()});if let Some(t)=target{v["target"]=json!(t)}
        self.views.push(v);self.views.len()-1
    }
    fn f32s(&mut self,data:&[f32],comps:usize,ty:&str,minmax:bool,target:Option<u32>)->usize{
        let bytes:Vec<u8>=data.iter().flat_map(|f|f.to_le_bytes()).collect();let v=self.view(&bytes,target);
        let mut a=json!({"bufferView":v,"componentType":5126,"count":data.len()/comps,"type":ty});
        if minmax&&!data.is_empty(){
            let mut lo=vec![f32::MAX;comps];let mut hi=vec![f32::MIN;comps];
            for c in data.chunks(comps){for i in 0..comps{lo[i]=lo[i].min(c[i]);hi[i]=hi[i].max(c[i])}}
            a["min"]=json!(lo);a["max"]=json!(hi);
        }
        self.accessors.push(a);self.accessors.len()-1
    }
    fn u32s(&mut self,data:&[u32])->usize{
        let bytes:Vec<u8>=data.iter().flat_map(|f|f.to_le_bytes()).collect();let v=self.view(&bytes,Some(34963));
        self.accessors.push(json!({"bufferView":v,"componentType":5125,"count":data.len(),"type":"SCALAR"}));self.accessors.len()-1
    }
    fn u16x4(&mut self,data:&[[u16;4]])->usize{
        let bytes:Vec<u8>=data.iter().flat_map(|j|j.iter().flat_map(|x|x.to_le_bytes())).collect();let v=self.view(&bytes,Some(34962));
        self.accessors.push(json!({"bufferView":v,"componentType":5123,"count":data.len(),"type":"VEC4"}));self.accessors.len()-1
    }
    fn finish(mut self,mut doc:Value)->Vec<u8>{
        self.align();
        doc["asset"]=json!({"version":"2.0","generator":"EAGL-Workbench decode-all"});
        doc["buffers"]=json!([{"byteLength":self.bin.len()}]);doc["bufferViews"]=json!(self.views);doc["accessors"]=json!(self.accessors);
        let mut js=serde_json::to_vec(&doc).unwrap();while js.len()%4!=0{js.push(b' ')}
        let total=12+8+js.len()+8+self.bin.len();
        let mut out=Vec::with_capacity(total);
        out.extend_from_slice(b"glTF");out.extend_from_slice(&2u32.to_le_bytes());out.extend_from_slice(&(total as u32).to_le_bytes());
        out.extend_from_slice(&(js.len() as u32).to_le_bytes());out.extend_from_slice(b"JSON");out.extend_from_slice(&js);
        out.extend_from_slice(&(self.bin.len() as u32).to_le_bytes());out.extend_from_slice(b"BIN\0");out.extend_from_slice(&self.bin);
        out
    }
}

fn wrap_code(w:Wrap)->u32{match w{Wrap::Clamp=>33071,Wrap::Repeat=>10497,Wrap::Mirror=>33648}}

/// A decoded model as binary glTF: one mesh, one primitive per decoded primitive, textures embedded as PNG.
pub fn glb_model(m:&BuiltModel,name:&str)->Result<Vec<u8>,String>{
    let mut g=Glb::new();
    let mut images=vec![];let mut textures=vec![];let mut samplers:Vec<Value>=vec![];
    for t in &m.textures{let v=g.view(&png(&t.rgba,t.width,t.height)?,None);images.push(json!({"bufferView":v,"mimeType":"image/png","name":t.name}));}
    let mut tex_of:std::collections::HashMap<(usize,[Wrap;2]),usize>=Default::default();
    let mut materials=vec![];
    for mat in &m.materials{
        let mut pbr=json!({"baseColorFactor":mat.base_color,"metallicFactor":0.0,"roughnessFactor":1.0});
        if let Some(t)=mat.texture{
            let key=(t,mat.wrap);
            let ti=*tex_of.entry(key).or_insert_with(||{
                let s=json!({"wrapS":wrap_code(mat.wrap[0]),"wrapT":wrap_code(mat.wrap[1])});
                let si=samplers.iter().position(|x|*x==s).unwrap_or_else(||{samplers.push(s);samplers.len()-1});
                textures.push(json!({"source":t,"sampler":si}));textures.len()-1});
            pbr["baseColorTexture"]=json!({"index":ti});
        }
        let mut mj=json!({"pbrMetallicRoughness":pbr,"doubleSided":mat.double_sided,
            "alphaMode":match mat.alpha{AlphaKind::Opaque=>"OPAQUE",AlphaKind::Mask=>"MASK",AlphaKind::Blend=>"BLEND"}});
        if mat.unlit{mj["extensions"]=json!({"KHR_materials_unlit":{}})}
        materials.push(mj);
    }
    let mut prims=vec![];
    for p in &m.prims{
        let pos:Vec<f32>=p.positions.iter().flatten().copied().collect();
        let nrm:Vec<f32>=p.normals.iter().flatten().copied().collect();
        let uv:Vec<f32>=p.uvs.iter().flatten().copied().collect();
        let mut attrs=json!({"POSITION":g.f32s(&pos,3,"VEC3",true,Some(34962)),"NORMAL":g.f32s(&nrm,3,"VEC3",false,Some(34962)),"TEXCOORD_0":g.f32s(&uv,2,"VEC2",false,Some(34962))});
        if let Some(c)=&p.colors{let c:Vec<f32>=c.iter().flatten().copied().collect();attrs["COLOR_0"]=json!(g.f32s(&c,4,"VEC4",false,Some(34962)))}
        if let (Some(j),Some(w))=(&p.joints,&p.weights){attrs["JOINTS_0"]=json!(g.u16x4(j));let w:Vec<f32>=w.iter().flatten().copied().collect();attrs["WEIGHTS_0"]=json!(g.f32s(&w,4,"VEC4",false,Some(34962)))}
        prims.push(json!({"attributes":attrs,"indices":g.u32s(&p.indices),"material":p.material,"extras":{"family":p.family}}));
    }
    let mut doc=json!({"scene":0,"scenes":[{"nodes":[0]}],"nodes":[{"name":name}],"materials":materials,"extras":{"warnings":m.warnings}});
    if !prims.is_empty(){doc["meshes"]=json!([{"name":name,"primitives":prims}]);doc["nodes"][0]["mesh"]=json!(0)}
    if !images.is_empty(){doc["images"]=json!(images);doc["textures"]=json!(textures);doc["samplers"]=json!(samplers)}
    if materials_uses_unlit(&doc){doc["extensionsUsed"]=json!(["KHR_materials_unlit"])}
    Ok(g.finish(doc))
}
fn materials_uses_unlit(doc:&Value)->bool{doc["materials"].as_array().map(|a|a.iter().any(|m|m.get("extensions").is_some())).unwrap_or(false)}

/// Skeleton (one node per bone, rest pose) plus every decodable clip as a glTF animation (30 samples/s,
/// quaternions sign-continuous; absent samples hold rest values).
pub fn glb_rig(skel:&Skeleton,clips:&[anim::Clip],name:&str)->Vec<u8>{
    let mut g=Glb::new();
    let mut nodes=vec![];let mut roots=vec![];
    for b in &skel.bones{
        let kids:Vec<usize>=skel.bones.iter().filter(|c|c.parent==b.index as i32).map(|c|c.index).collect();
        let mut n=json!({"name":b.name,"rotation":[b.quat[0],b.quat[1],b.quat[2],b.quat[3]],"translation":b.trans,"scale":b.scale});
        if !kids.is_empty(){n["children"]=json!(kids)}
        nodes.push(n);if b.parent<0{roots.push(b.index)}
    }
    let mut anims=vec![];
    for c in clips{
        let n=c.sample_count.max(1);
        let times:Vec<f32>=(0..n).map(|i|(i as f64/anim::CLIP_FPS) as f32).collect();
        let input=g.f32s(&times,1,"SCALAR",true,None);
        let mut samplers=vec![];let mut channels=vec![];
        let mut add=|g:&mut Glb,bone:usize,path:&str,vals:Vec<f32>,comps:usize,ty:&str|{
            let out=g.f32s(&vals,comps,ty,false,None);samplers.push(json!({"input":input,"output":out,"interpolation":"LINEAR"}));
            channels.push(json!({"sampler":samplers.len()-1,"target":{"node":bone,"path":path}}));
        };
        for (b,s) in &c.rot{if let Ok(q)=anim::normalized(s){add(&mut g,*b,"rotation",q.iter().flatten().copied().collect(),4,"VEC4")}}
        for (b,s) in &c.trans{let rest=skel.bones[*b].trans;add(&mut g,*b,"translation",s.iter().flat_map(|v|v.unwrap_or(rest).map(|x|x as f32)).collect(),3,"VEC3")}
        for (b,s) in &c.scale{let rest=skel.bones[*b].scale;add(&mut g,*b,"scale",s.iter().flat_map(|v|v.unwrap_or(rest).map(|x|x as f32)).collect(),3,"VEC3")}
        if !channels.is_empty(){anims.push(json!({"name":c.name,"samplers":samplers,"channels":channels,"extras":{"codec":c.codec,"caveats":c.caveats}}))}
    }
    let mut doc=json!({"scene":0,"scenes":[{"name":name,"nodes":roots}],"nodes":nodes});
    if !anims.is_empty(){doc["animations"]=json!(anims)}
    g.finish(doc)
}

/// Triangle soup as OBJ, one group per named object.
pub fn obj(groups:&[(String,Vec<[[f32;3];3]>)])->String{
    let mut s=String::from("# EAGL-Workbench decode-all\n");let mut n=0usize;
    for (name,tris) in groups{
        s+=&format!("o {}\n",name.replace(char::is_whitespace,"_"));
        for t in tris{for v in t{s+=&format!("v {} {} {}\n",v[0],v[1],v[2])}s+=&format!("f {} {} {}\n",n+1,n+2,n+3);n+=3}
    }
    s
}
