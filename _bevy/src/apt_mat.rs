//! Material for the frontend's 2D draws.
use bevy::render::render_resource::ShaderType;
use bevy::{asset::embedded_asset,mesh::MeshVertexBufferLayoutRef,pbr::{Material,MaterialPipeline,MaterialPipelineKey,MaterialPlugin},prelude::*,render::render_resource::{AsBindGroup,RenderPipelineDescriptor,SpecializedMeshPipelineError},shader::ShaderRef};

/// Clip region: up to 48 triangles in movie space (two `vec4` each: x0 y0 x1 y1 / x2 y2 pad pad).
#[derive(ShaderType,Clone,Debug,PartialEq)]
pub struct MaskBuf{pub n:UVec4,pub tris:[Vec4;96]}
impl Default for MaskBuf{fn default()->Self{Self{n:UVec4::ZERO,tris:[Vec4::ZERO;96]}}}
impl MaskBuf{
    pub fn from(m:Option<&[[f32;6]]>)->Self{
        let mut b=Self::default();
        if let Some(m)=m{
            for (i,t) in m.iter().take(48).enumerate(){b.tris[2*i]=Vec4::new(t[0],t[1],t[2],t[3]);b.tris[2*i+1]=Vec4::new(t[4],t[5],0.,0.);}
            b.n.x=m.len().min(48) as u32;
        }
        b
    }
}

#[derive(Asset,TypePath,AsBindGroup,Clone,Debug)]
pub struct AptMat{
    #[uniform(4)] pub mask:MaskBuf,
    #[uniform(0)] pub mul:Vec4,
    #[uniform(1)] pub add:Vec4,
    #[texture(2)] #[sampler(3)] pub tex:Option<Handle<Image>>,
}
impl Material for AptMat{
    fn fragment_shader()->ShaderRef{
        // The crate name in the embedded path follows the binary name (EAGL-Workbench), so ask the macro for it.
        let p=bevy::asset::embedded_path!("apt_ui.wgsl");
        let s=format!("embedded://{}",p.display().to_string().replace('\\',"/"));
        ShaderRef::Path(bevy::asset::AssetPath::parse(&s).into_owned())
    }
    fn alpha_mode(&self)->AlphaMode{AlphaMode::Blend}
    fn enable_prepass()->bool{false}
    fn enable_shadows()->bool{false}
    /// Shapes are y-flipped and wound either way: never cull.
    fn specialize(_p:&MaterialPipeline,d:&mut RenderPipelineDescriptor,_l:&MeshVertexBufferLayoutRef,_k:MaterialPipelineKey<Self>)->Result<(),SpecializedMeshPipelineError>{
        d.primitive.cull_mode=None;Ok(())
    }
}

pub struct AptMatPlugin;
impl Plugin for AptMatPlugin{
    fn build(&self,app:&mut App){
        embedded_asset!(app,"apt_ui.wgsl");
        app.add_plugins(MaterialPlugin::<AptMat>::default());
    }
}

#[cfg(test)]
mod tests{
    use super::*;
    #[test] fn mask_buffer_packs_two_vectors_per_triangle_and_caps_at_48(){
        let tris:Vec<[f32;6]>=(0..60).map(|i|[i as f32,1.,2.,3.,4.,5.]).collect();
        let m=MaskBuf::from(Some(&tris));
        assert_eq!(m.n.x,48);
        assert_eq!(m.tris[2],Vec4::new(1.,1.,2.,3.));assert_eq!(m.tris[3],Vec4::new(4.,5.,0.,0.));
        assert_eq!(MaskBuf::from(None).n.x,0);
    }
}
