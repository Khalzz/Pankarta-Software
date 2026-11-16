use crate::asset_manager::collision::{self, Source};
use crate::asset_manager::primitives::collision_geometry;
use crate::asset_manager::AssetManager;
use crate::physics::model_shape::{ModelPlaceholder, ModelRef, ProfileOutline};
use rapier3d::prelude::RigidBodyBuilder;

use crate::physics::body::{self, Colliders};
use crate::physics::{Collider, Layers};
use crate::scene_manager::node::Node;
use crate::scene_manager::node::properties::{Mesh, MeshShape, Model, Transform3D};
use crate::scene_manager::render_bridge::place;
use crate::physics::physics_resources::PhysicsObjectDef;

use super::scene::Scene;

/// Bridges a spawned `Node`'s `Transform3D`, `RigidBodyBuilder` and
/// `Colliders` properties into a
/// `PhysicsObjectDef` - what its body is made from (see that type) - on
/// `SceneContent::physics_bodies`, until the scene's physics starts or, once
/// it has, the node's spawn hands it on (see `Scene::spawn_one`). Mirrors
/// `render_bridge::register_static_model`'s role for rendering - same
/// "extract once at spawn time" shape, just building a `PhysicsObjectDef`
/// instead of an `InstanceData`.
///
/// Also where colliders read from a model or a mesh (`Collider::from_model`,
/// `from_mesh`) are built: a file's meshes taken from
/// `assets.collision_meshes` (the scene's loading step read them already; a
/// file it didn't is read now, on the spot), a mesh's generated as it's
/// drawn, all placed and scaled as this node draws them, and made into the
/// Rapier shape they asked for.
///
/// Unlike `register_static_model`, this never needs to run again for the
/// same id later - a body's starting position/mass/colliders are only read
/// once, when it's made; from then on the body is where it is.
///
/// A node with colliders and no `RigidBodyBuilder` gets the body they imply
/// (see `body::implied_body`); one with a body and no colliders, just the
/// body.
pub fn register_physics_body(scene: &mut Scene, id: &str, assets: &mut AssetManager) -> Result<(), String> {
    let node = scene.content.nodes.get(id).ok_or_else(|| format!("no node named '{id}' to register"))?;
    let transform = *node.transform_3d().ok_or_else(|| format!("node '{id}' has no Transform3D property"))?;
    if !has_body(node) {
        return Err(format!("node '{id}' has no RigidBodyBuilder or Colliders property"));
    }
    let entries = node.get_property::<Colliders>().cloned().unwrap_or_default().into_entries();
    let body = node.get_property::<RigidBodyBuilder>().cloned().unwrap_or_else(|| body::implied_body(&entries));
    let follows_node = body::follows_node(&body);
    let layers = node.get_property::<Layers>().map(|layers| layers.0.clone()).unwrap_or_default();

    let own = Own { model: node.get_property::<Model>(), mesh: node.get_property::<Mesh>() };
    let colliders = entries.into_iter()
        .flat_map(|collider| resolve_collider(collider, transform.scale, id, &own, assets))
        .collect();

    scene.content.physics_bodies.push(PhysicsObjectDef { id: id.to_owned(), position: transform.position, rotation: transform.rotation, body, colliders, layers, follows_node });

    Ok(())
}

/// Whether `node` gets a body - it has a `RigidBodyBuilder` or `Colliders`.
pub(crate) fn has_body(node: &Node) -> bool {
    node.get_property::<RigidBodyBuilder>().is_some() || node.get_property::<Colliders>().is_some()
}

/// Whether physics moves `node` - its own `RigidBodyBuilder` is dynamic,
/// fixed or velocity-based - so its parent doesn't. A kinematic
/// position-based one, or colliders with no body, follow the node instead.
pub(crate) fn moved_by_physics(node: &Node) -> bool {
    node.get_property::<RigidBodyBuilder>().is_some_and(|body| !body::follows_node(body))
}

/// Every model file `node`'s colliders read meshes from - for the scene's
/// loading step to read ahead.
pub(crate) fn collision_mesh_paths(node: &Node) -> Vec<&str> {
    let colliders = node.get_property::<Colliders>().into_iter().flat_map(Colliders::iter);
    colliders.filter_map(|collider| match &ModelPlaceholder::of(&collider.builder)?.model {
        ModelRef::Model { file, .. } => file.as_deref(),
        ModelRef::OwnModel => node.get_property::<Model>().and_then(Model::file_path),
        ModelRef::OwnMesh => None,
    }).collect()
}

/// The node's own `Model` and `Mesh`, for `from_own_model`/`from_own_mesh`.
struct Own<'a> {
    model: Option<&'a Model>,
    mesh: Option<&'a Mesh>,
}

/// A model's meshes - its file's and its parts' - each with where it's
/// placed in its body's frame.
struct Gathered {
    /// The file's meshes, all placed by the model's transform.
    file: Option<(collision::FileMeshes, Transform3D)>,
    /// Each part's name, geometry, and where it's placed.
    parts: Vec<(String, Vec<nalgebra::Vector3<f32>>, Vec<[u32; 3]>, Transform3D)>,
    /// Each part's rings, placed, if it's a `Mesh::profile_shape` - by
    /// name, for its collider's debug drawing.
    outlines: std::collections::HashMap<String, ProfileOutline>,
}

/// A collider read from a model or a mesh, made: its meshes read (a file's
/// now if they weren't), placed as the node draws them, and made into its
/// Rapier shape - one collider, or one per mesh with `Meshes::each`, each
/// with the collider's settings. One that can't be read or made is left
/// out, saying why. Any other collider as it is.
fn resolve_collider(collider: Collider, node_scale: nalgebra::Vector3<f32>, id: &str, own: &Own, assets: &mut AssetManager) -> Vec<Collider> {
    let Some(placeholder) = ModelPlaceholder::of(&collider.builder).cloned() else { return vec![collider] };
    // The node's scale - its body has its position and rotation.
    let node = Transform3D { scale: node_scale, ..Default::default() };
    let (file, parts, model_transform): (Option<&str>, Vec<(String, Mesh)>, Transform3D) = match &placeholder.model {
        ModelRef::Model { file, parts, transform } => (file.as_deref(), parts.clone(), *transform),
        ModelRef::OwnModel => match own.model {
            Some(model) => (model.file_path(), model.parts().map(|(name, mesh)| (name.to_owned(), mesh.clone())).collect(), model.transform),
            None => {
                eprintln!("node '{id}': a collider from its own model, but it has no Model - left out");
                return Vec::new();
            }
        },
        ModelRef::OwnMesh => match own.mesh {
            Some(mesh) => (None, vec![("mesh".to_owned(), mesh.clone())], Transform3D::default()),
            None => {
                eprintln!("node '{id}': a collider from its own mesh, but it has no Mesh - left out");
                return Vec::new();
            }
        },
    };
    let what = file.unwrap_or("its meshes").to_owned();

    let mut gathered = Gathered { file: None, parts: Vec::new(), outlines: Default::default() };
    if let Some(path) = file {
        match assets.collision_meshes.get_or_load(path) {
            Ok(meshes) => gathered.file = Some((meshes, place(&node, &model_transform))),
            Err(error) => {
                eprintln!("node '{id}': collider from '{path}' couldn't be read - left out: {error}");
                return Vec::new();
            }
        }
    }
    for (name, mesh) in &parts {
        let (source, geometry_scale) = mesh.geometry();
        let Some((points, triangles)) = collision_geometry(&source, &assets.models.meshes) else {
            eprintln!("node '{id}': collider from mesh '{name}' - its geometry isn't there (a custom mesh never added?) - left out");
            continue;
        };
        // As render_bridge::mesh_draw places a mesh in its model.
        let own_transform = Transform3D { scale: mesh.transform.scale.component_mul(&geometry_scale), ..mesh.transform };
        let at = place(&node, &place(&model_transform, &own_transform));
        if let MeshShape::ProfileShape { rings, sides } = &mesh.shape {
            // Placed as collision::pick places the mesh's points.
            let outline = ProfileOutline::new(rings, *sides, |point| nalgebra::Point3::from(at.position + at.rotation * point.component_mul(&at.scale)));
            gathered.outlines.insert(name.clone(), outline);
        }
        gathered.parts.push((name.clone(), points, triangles, at));
    }

    let file_sources = gathered.file.iter().flat_map(|(meshes, at)| {
        meshes.iter().map(move |(name, points, triangles)| Source { name, points, triangles, at: *at })
    });
    let part_sources = gathered.parts.iter().map(|(name, points, triangles, at)| Source { name, points, triangles, at: *at });
    let picked = collision::pick(file_sources.chain(part_sources), &placeholder.meshes);
    if picked.is_empty() {
        eprintln!("node '{id}': no mesh of {what} is {} - collider left out", placeholder.meshes.describe());
    }
    // The meshes in a picked one: itself with `.each()`, every match without.
    let file_names: Vec<&str> = gathered.file.iter().flat_map(|(meshes, _)| meshes.iter().map(|(name, _, _)| name.as_str())).collect();
    let part_names: Vec<&str> = gathered.parts.iter().map(|(name, ..)| name.as_str()).collect();
    let each = placeholder.meshes.is_each();
    let in_picked = |picked: &str, name: &str| placeholder.meshes.matches(name) && (!each || name == picked);
    picked.into_iter().filter_map(|mesh| {
        let name = mesh.name.clone();
        // Drawn by its rings if it's all profiles - nothing from the file.
        let outline = if file_names.iter().any(|file| in_picked(&name, file)) {
            None
        } else {
            part_names.iter().filter(|part| in_picked(&name, part)).try_fold(None::<ProfileOutline>, |merged, part| {
                let outline = gathered.outlines.get(*part)?.clone();
                Some(Some(match merged {
                    Some(merged) => merged.merge(outline),
                    None => outline,
                }))
            }).flatten()
        };
        match placeholder.shape.build(mesh) {
            Ok(shape) => {
                let outline = outline.map(|outline| outline.trace(shape.as_ref()).into());
                let mut builder = collider.builder.clone();
                builder.shape = shape;
                Some(Collider { builder, outline, ..collider.clone() })
            }
            Err(error) => {
                eprintln!("node '{id}': collider from mesh '{name}' of {what} couldn't be made ({:?}) - left out: {error}", placeholder.shape);
                None
            }
        }
    }).collect()
}
