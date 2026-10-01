// Frontend (APT) 2D material: Wii TEV-style `texture * multiplier + additive` in gamma space,
// converted to linear for the sRGB swapchain.
#import bevy_pbr::forward_io::VertexOutput

@group(#{MATERIAL_BIND_GROUP}) @binding(0) var<uniform> mul_color: vec4<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(1) var<uniform> add_color: vec4<f32>;
struct MaskBuf { n: vec4<u32>, tris: array<vec4<f32>, 96> };
@group(#{MATERIAL_BIND_GROUP}) @binding(4) var<uniform> mask: MaskBuf;
@group(#{MATERIAL_BIND_GROUP}) @binding(2) var tex: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(3) var tex_sampler: sampler;

fn srgb_to_linear(c: vec3<f32>) -> vec3<f32> {
    let lo = c / 12.92;
    let hi = pow((c + vec3<f32>(0.055)) / 1.055, vec3<f32>(2.4));
    return select(hi, lo, c <= vec3<f32>(0.04045));
}

fn in_tri(p: vec2<f32>, a: vec2<f32>, b: vec2<f32>, c: vec2<f32>) -> bool {
    let d1 = (p.x - b.x) * (a.y - b.y) - (a.x - b.x) * (p.y - b.y);
    let d2 = (p.x - c.x) * (b.y - c.y) - (b.x - c.x) * (p.y - c.y);
    let d3 = (p.x - a.x) * (c.y - a.y) - (c.x - a.x) * (p.y - a.y);
    let neg = (d1 < 0.0) || (d2 < 0.0) || (d3 < 0.0);
    let pos = (d1 > 0.0) || (d2 > 0.0) || (d3 > 0.0);
    return !(neg && pos);
}

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    if (mask.n.x > 0u) {
        let p = vec2<f32>(in.world_position.x, -in.world_position.y);
        var inside = false;
        for (var i = 0u; i < mask.n.x; i = i + 1u) {
            let t0 = mask.tris[2u * i];
            let t1 = mask.tris[2u * i + 1u];
            if (in_tri(p, t0.xy, t0.zw, t1.xy)) { inside = true; break; }
        }
        if (!inside) { discard; }
    }
    let t = textureSample(tex, tex_sampler, in.uv);
    let c = clamp(t * mul_color + add_color, vec4<f32>(0.0), vec4<f32>(1.0));
    return vec4<f32>(srgb_to_linear(c.rgb), c.a);
}
