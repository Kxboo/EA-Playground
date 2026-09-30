//! `Tetherball::Initialize` (0x8039d3d8), before its first Update.
use crate::tetherball::{BallMotion, Direction, Zone};
use crate::tetherball_scene::BallScene;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LoadedAsset {
    pub handle: u32,
    pub id: u32,
}

/// Retained object/asset references at Tetherball +4..+30. The scene owns
/// the shadow-presence projection used during Update; these are the real handles.
#[derive(Clone, Debug, PartialEq)]
pub struct BallResources {
    pub shadows: [u32; 2],
    pub cached: [u32; 4],
    /// Ball model, ball shadow model, ball texture, rope model, rope shadow
    /// model, rope texture, in native field order.
    pub asset_ids: [u32; 6],
    pub accelerate_modifier_158: f32,
    pub flag_15c: u8,
}

/// Engine-owned resources and database collection lifetimes. Implementations
/// must be synchronous and must not reenter ball logic during these calls.
pub trait BallInitServices {
    fn texture(&mut self, name: &str) -> LoadedAsset;
    fn model(&mut self, name: &str, flags: i32) -> LoadedAsset;
    fn set_textures(&mut self, model: u32, texture: u32);
    /// Uses the original current memory pool and allocation type zero.
    fn allocate(&mut self, bytes: u32, tag: &str) -> u32;
    fn construct_cached_model(&mut self, handle: u32);
    /// Writes CachedModel+44 then invokes SetScaleMatrix.
    fn bind_cached_model(&mut self, cached: u32, model: u32);
    fn construct_shadow(&mut self, handle: u32, model: u32);
    fn add_scene_entity(&mut self, layer: i32, entity: u32);
    fn database_key(&mut self, name: &str) -> u64;
    fn collection(&mut self, class: u64, name: u64) -> u32;
    fn float_from_array(&mut self, collection: u32, field: &str, index: u32) -> f32;
    fn destroy_collection(&mut self, collection: u32);
}

#[derive(Clone, Copy, Debug)]
pub struct BallInitInput {
    /// Native raw unsigned array index; this routine does not clamp difficulty.
    pub difficulty: u32,
    pub anchor: [f32; 3],
    pub heading: f32,
    /// Placeable +f4, read after the tuning collection is released.
    pub pole_height: f32,
}

fn cached(host: &mut impl BallInitServices) -> u32 {
    let handle = host.allocate(0x4c, "Ren::CachedModel");
    if handle != 0 {
        host.construct_cached_model(handle);
    }
    handle
}

fn shadow(host: &mut impl BallInitServices, model: u32) -> u32 {
    let handle = host.allocate(0x4c, "TetherballShadowRenderEntity");
    if handle != 0 {
        host.construct_shadow(handle, model);
    }
    handle
}

/// Preserve all fields the original initializer leaves to the constructor or
/// caller: radii, velocities, attachment, trails, position and model matrices.
/// Cached-model allocation failure is a native invalid-dereference path, so the
/// host must provide valid cached resources; shadow allocation may return null.
pub fn initialize_ball(
    motion: &mut BallMotion,
    scene: &mut BallScene,
    input: BallInitInput,
    host: &mut impl BallInitServices,
) -> BallResources {
    // These misspellings are the retail asset names.
    let texture = host.texture("teatherball.gsh");
    let model = host.model("teatherball.o", 0);
    host.set_textures(model.handle, texture.handle);
    let shadow_model = host.model("teatherball_shadow.o", 0);
    let ball_cached = cached(host);
    let ball_shadow_cached = cached(host);
    host.bind_cached_model(ball_cached, model.handle);
    host.bind_cached_model(ball_shadow_cached, shadow_model.handle);

    let rope_texture = host.texture("teatherball_rope.gsh");
    let rope_model = host.model("teatherball_rope.o", 0);
    host.set_textures(rope_model.handle, rope_texture.handle);
    let rope_shadow_model = host.model("teatherball_rope_shadow.o", 0);
    let rope_cached = cached(host);
    let rope_shadow_cached = cached(host);
    host.bind_cached_model(rope_cached, rope_model.handle);
    host.bind_cached_model(rope_shadow_cached, rope_shadow_model.handle);

    let ball_shadow = shadow(host, shadow_model.handle);
    host.add_scene_entity(0, ball_shadow);
    let rope_shadow = shadow(host, rope_shadow_model.handle);
    // Initialize registers only the ball shadow with the scene.
    scene.ball_shadow = ball_shadow != 0;
    scene.rope_shadow = rope_shadow != 0;
    motion.direction = Direction::Zero;
    scene.anchor = input.anchor;
    motion.zone = Zone::Zero;
    motion.angle = crate::tetherball_angles::wrap_angle(f32::from_bits(0x40490fdb) + input.heading);

    let class = host.database_key("mg_tetherball");
    let name = host.database_key("tunables");
    let collection = host.collection(class, name);
    motion.base_hit_speed =
        host.float_from_array(collection, "ball_basehitspeed", input.difficulty);
    let accelerate_modifier_158 =
        host.float_from_array(collection, "ball_acceleratemodifier", input.difficulty);
    motion.power_modifier =
        host.float_from_array(collection, "ball_powermodifier", input.difficulty);
    motion.mega_modifier = host.float_from_array(collection, "ball_megamodifier", input.difficulty);
    host.destroy_collection(collection);

    motion.pole_height = input.pole_height;
    motion.height = input.pole_height + 0.5;
    motion.target_height = motion.height;
    motion.spinning_down = false;
    motion.spinning_up = false;
    BallResources {
        shadows: [ball_shadow, rope_shadow],
        cached: [
            ball_cached,
            ball_shadow_cached,
            rope_cached,
            rope_shadow_cached,
        ],
        asset_ids: [
            model.id,
            shadow_model.id,
            texture.id,
            rope_model.id,
            rope_shadow_model.id,
            rope_texture.id,
        ],
        accelerate_modifier_158,
        flag_15c: 1,
    }
}

#[cfg(test)]
#[path = "tetherball_ball_init_tests.rs"]
mod tests;
