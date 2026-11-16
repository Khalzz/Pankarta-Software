use crate::asset_manager::collision::{self, CollisionMeshCache};
use crate::physics::model_shape::{ModelPlaceholder, ModelRef};
use crate::physics::{Collider, Layers, Physics, RigidBodyData, Sensor};
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
/// Also where colliders read from a model (`Collider::from_model`) are
/// built: their meshes taken from `meshes` (`app.assets.collision_meshes` -
/// the scene's loading step read them already; a file it didn't is read
/// now, on the spot), placed and scaled as this node draws them, and made
/// into the Rapier shape they asked for.
///
/// Unlike `register_static_model`, this never needs to run again for the
/// same id later - a body's starting position/mass/colliders are only read
/// once, when it's made; from then on the body is where it is.
///
/// A node's `Sensor` is one more collider on its body - or, without
/// `Physics`, its whole body, one that follows the node.
pub fn register_physics_body(scene: &mut Scene, id: &str, meshes: &mut CollisionMeshCache) -> Result<(), String> {
    let node = scene.content.nodes.get(id).ok_or_else(|| format!("no node named '{id}' to register"))?;
    let transform = *node.transform_3d().ok_or_else(|| format!("node '{id}' has no Transform3D property"))?;
    let sensor = node.get_property::<Sensor>();
    let follows_node = node.get_property::<Physics>().is_none();
    let mut physics = match node.get_property::<Physics>() {
        Some(physics) => physics.clone(),
        None if sensor.is_some() => Physics::new(RigidBodyData::default()),
        None => return Err(format!("node '{id}' has no Physics or Sensor property")),
    };
    physics.colliders.extend(sensor.map(|sensor| sensor.collider.clone()));
    let layers = node.get_property::<Layers>().map(|layers| layers.0.clone()).unwrap_or_default();

    let own_model = node.get_property::<Model>().and_then(|model| Some((model.file_path()?.to_owned(), model.transform)));
    physics.colliders = physics.colliders.into_iter()
        .flat_map(|collider| resolve_collider(collider, transform.scale, id, own_model.as_ref(), meshes))
        .collect();

    scene.content.physics_bodies.push(PhysicsObjectDef { id: id.to_owned(), position: transform.position, rotation: transform.rotation, physics, layers, follows_node });

    Ok(())
}

/// Whether `node` gets a body - it has `Physics` or a `Sensor`.
pub(crate) fn has_body(node: &Node) -> bool {
    node.get_property::<Physics>().is_some() || node.get_property::<Sensor>().is_some()
}

/// Every model file `node`'s colliders read meshes from - for the scene's
/// loading step to read ahead.
pub(crate) fn collision_mesh_paths(node: &Node) -> Vec<&str> {
    let physics = node.get_property::<Physics>().map(Physics::colliders).unwrap_or_default();
    let sensor = node.get_property::<Sensor>().map(|sensor| &sensor.collider);
    physics.iter().chain(sensor).filter_map(|collider| match &ModelPlaceholder::of(&collider.builder)?.model {
        ModelRef::File { path, .. } => Some(path.as_str()),
        ModelRef::Own => node.get_property::<Model>().and_then(Model::file_path),
        ModelRef::NoFile => None,
    }).collect()
}

/// A collider read from a model, made: its meshes from `meshes` (read now
/// if they weren't), placed by its model's transform and `node_scale`, and
/// made into its Rapier shape - one collider, or one per mesh with
/// `Meshes::each`, each with the collider's settings. One that can't be read
/// or made is left out, saying why. Any other collider as it is.
fn resolve_collider(collider: Collider, node_scale: nalgebra::Vector3<f32>, id: &str, own_model: Option<&(String, crate::scene_manager::node::properties::Transform3D)>, meshes: &mut CollisionMeshCache) -> Vec<Collider> {
    let Some(placeholder) = ModelPlaceholder::of(&collider.builder).cloned() else { return vec![collider] };
    let (path, transform) = match &placeholder.model {
        ModelRef::File { path, transform } => (path.as_str(), *transform),
        ModelRef::Own => match own_model {
            Some((path, transform)) => (path.as_str(), *transform),
            None => {
                eprintln!("node '{id}': a collider from its own model, but it has no Model file - left out");
                return Vec::new();
            }
        },
        ModelRef::NoFile => {
            eprintln!("node '{id}': a collider from a Model with no file - left out");
            return Vec::new();
        }
    };
    let file = match meshes.get_or_load(path) {
        Ok(file) => file,
        Err(error) => {
            eprintln!("node '{id}': collider from '{path}' couldn't be read - left out: {error}");
            return Vec::new();
        }
    };
    let picked = collision::pick(&file, &placeholder.meshes, &transform, node_scale);
    if picked.is_empty() {
        eprintln!("node '{id}': no mesh of '{path}' is {} - collider left out", placeholder.meshes.describe());
    }
    picked.into_iter().filter_map(|mesh| {
        let name = mesh.name.clone();
        match placeholder.shape.build(mesh) {
            Ok(shape) => {
                let mut builder = collider.builder.clone();
                builder.shape = shape;
                Some(Collider { builder, ..collider.clone() })
            }
            Err(error) => {
                eprintln!("node '{id}': collider from mesh '{name}' of '{path}' couldn't be made ({:?}) - left out: {error}", placeholder.shape);
                None
            }
        }
    }).collect()
}
