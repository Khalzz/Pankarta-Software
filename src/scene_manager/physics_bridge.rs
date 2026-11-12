use std::collections::HashMap;

use nalgebra::Vector3;

use crate::physics::{Collider, ColliderShape, Layers, Physics, RigidBodyData, Sensor};
use crate::scene_manager::node::Node;
use crate::scene_manager::node::properties::Model;
use crate::physics::physics_resources::PhysicsObjectDef;

use super::scene::Scene;

/// Bridges a spawned `Node`'s `Transform3D`+`Physics` properties into a
/// `PhysicsObjectDef` - what its body is made from (see that type) - on
/// `SceneContent::physics_bodies`, until the scene's physics starts or, once
/// it has, the node's spawn hands it on (see `Scene::spawn_one`). Mirrors
/// `render_bridge::register_static_model`'s role for rendering - same
/// "extract once at spawn time" shape, just building a `PhysicsObjectDef`
/// instead of an `InstanceData`.
///
/// Also where `ColliderShape::TrimeshFromModel` gets resolved into a real
/// `ColliderShape::Trimesh` (see that variant's own doc comment) - reading
/// the model file and scaling its geometry by this node's own
/// `Transform3D::scale` both happen here, at spawn time, so the body is
/// built from geometry already in hand.
///
/// `trimeshes` is the mesh geometry the scene's loading step already read
/// (by model path) - a path missing from it is read now, on the spot.
///
/// Unlike `register_static_model`, this never needs to run again for the
/// same id later - a body's starting position/mass/colliders are only read
/// once, when it's made; from then on the body is where it is.
///
/// A node's `Sensor` is one more collider on its body - or, without
/// `Physics`, its whole body, one that follows the node.
pub fn register_physics_body(scene: &mut Scene, id: &str, trimeshes: &TrimeshCache) -> Result<(), String> {
    let node = scene.content.nodes.get(id).ok_or_else(|| format!("no node named '{id}' to register"))?;
    let transform = *node.transform_3d().ok_or_else(|| format!("node '{id}' has no Transform3D property"))?;
    let sensor = node.get_property::<Sensor>();
    let follows_node = node.get_property::<Physics>().is_none();
    let mut physics = match node.get_property::<Physics>() {
        Some(physics) => physics.clone(),
        None if sensor.is_some() => Physics { rigidbody: RigidBodyData::default(), colliders: Vec::new() },
        None => return Err(format!("node '{id}' has no Physics or Sensor property")),
    };
    physics.colliders.extend(sensor.map(|sensor| sensor.collider.clone()));
    let layers = node.get_property::<Layers>().map(|layers| layers.0.clone()).unwrap_or_default();

    // Geometry read from a model file is scaled the way the model is drawn.
    let geometry_scale = transform.scale.component_mul(&super::render_bridge::geometry_scale(node));
    let own_model = node.get_property::<Model>().and_then(Model::file_path).map(str::to_owned);
    physics.colliders = physics.colliders.into_iter()
        .map(|collider| resolve_collider(collider, geometry_scale, id, own_model.as_deref(), trimeshes))
        .collect();

    scene.content.physics_bodies.push(PhysicsObjectDef { id: id.to_owned(), position: transform.position, rotation: transform.rotation, physics, layers, follows_node });

    Ok(())
}

/// A model's mesh as a collider sees it - its points and triangles.
pub type Trimesh = (Vec<Vector3<f32>>, Vec<[u32; 3]>);
/// Meshes already read for colliders, by model path (see
/// `ColliderShape::TrimeshFromModel`) - or why one couldn't be.
pub type TrimeshCache = HashMap<String, Result<Trimesh, String>>;

/// Whether `node` gets a body - it has `Physics` or a `Sensor`.
pub(crate) fn has_body(node: &Node) -> bool {
    node.get_property::<Physics>().is_some() || node.get_property::<Sensor>().is_some()
}

/// Every model path `node`'s colliders read a mesh from - for the scene's
/// loading step to read ahead.
pub(crate) fn trimesh_paths(node: &Node) -> Vec<&str> {
    let physics = node.get_property::<Physics>().map(|physics| physics.colliders.as_slice()).unwrap_or_default();
    let sensor = node.get_property::<Sensor>().map(|sensor| &sensor.collider);
    physics.iter().chain(sensor).filter_map(|collider| match &collider.shape {
        ColliderShape::TrimeshFromModel { model_path } | ColliderShape::ConvexHullFromModel { model_path } => Some(model_path.as_str()),
        ColliderShape::ConvexHullFromOwnModel => node.get_property::<Model>().and_then(Model::file_path),
        _ => None,
    }).collect()
}

/// Reads the mesh at `model_path` (inside the assets folder) for a collider.
pub(crate) fn load_trimesh(model_path: &str) -> Result<Trimesh, String> {
    crate::asset_manager::gltf::load_trimesh_geometry(model_path).map_err(|error| error.to_string())
}

fn resolve_collider(mut collider: Collider, scale: Vector3<f32>, id: &str, own_model: Option<&str>, trimeshes: &TrimeshCache) -> Collider {
    let (model_path, hull) = match &collider.shape {
        ColliderShape::TrimeshFromModel { model_path } => (model_path.clone(), false),
        ColliderShape::ConvexHullFromModel { model_path } => (model_path.clone(), true),
        ColliderShape::ConvexHullFromOwnModel => match own_model {
            Some(path) => (path.to_owned(), true),
            None => {
                eprintln!("node '{id}': a collider from its own model, but it has no Model file");
                collider.shape = ColliderShape::ConvexHull { points: vec![] };
                return collider;
            }
        },
        _ => return collider,
    };
    let model_path = &model_path;

    let loaded = match trimeshes.get(model_path) {
        Some(loaded) => loaded.clone(),
        None => load_trimesh(model_path),
    };
    match loaded {
        Ok((raw_vertices, indices)) => {
            let vertices = raw_vertices.iter()
                .map(|v| Vector3::new(v.x * scale.x, v.y * scale.y, v.z * scale.z))
                .collect();
            collider.shape = if hull { ColliderShape::ConvexHull { points: vertices } } else { ColliderShape::Trimesh { vertices, indices } };
            collider
        },
        Err(error) => {
            eprintln!("node '{id}': collider from '{model_path}' couldn't be loaded: {error}");
            // Falls back to a real (if wrong) ColliderShape rather than
            // leaving the unresolved TrimeshFromModel in place - the physics
            // thread has no file access to retry this itself, so an empty
            // Trimesh (no collision at all) is the honest failure mode here,
            // not a silent do-over later.
            collider.shape = ColliderShape::Trimesh { vertices: vec![], indices: vec![] };
            collider
        }
    }
}
