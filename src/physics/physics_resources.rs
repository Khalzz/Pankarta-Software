use std::collections::HashMap;
use nalgebra::{UnitQuaternion, Vector3};
use rapier3d::prelude::*;

use crate::physics::body;

/// A body loaded from a `PhysicsObjectDef`, and its colliders.
pub struct PhysicsData {
    pub rigidbody_handle: RigidBodyHandle,
    pub collider_handles: Vec<ColliderHandle>,
}



/// Rotational inertia for a body of `mass` kg with its center of mass at
/// `center_of_mass`, worked out from where its `colliders` are and how big:
/// each solid one (not sensors, half spaces or triangle meshes) counts as
/// the upright box around it, its share of `mass` by volume, plus how far
/// it sits from the center of mass. For giving a body exact numbers of
/// your own - with its colliders at `density(0.0)` so they add nothing:
///
///     let inertia = compute_principal_inertia(3000.0, center_of_mass, colliders.builders());
///     RigidBodyBuilder::dynamic().additional_mass_properties(MassProperties::new(center_of_mass.into(), 3000.0, inertia))
///
/// Rapier's own (from the colliders' `mass`/`density`) is exact for every
/// shape - this is an approximation for when the numbers must be yours.
pub fn compute_principal_inertia<'a>(mass: f32, center_of_mass: Vector3<f32>, colliders: impl IntoIterator<Item = &'a ColliderBuilder>) -> Vector3<f32> {
    struct BoxPart {
        volume: f32,
        half_extents: (f32, f32, f32),
        position: (f32, f32, f32),
    }

    // Each solid collider counted as the upright box around it, where it
    // sits - not sensors, nor shapes with no inside (a half space, a
    // triangle mesh).
    let boxes: Vec<BoxPart> = colliders.into_iter().filter(|collider| !collider.is_sensor).filter_map(|collider| {
        let shape = &collider.shape;
        if shape.as_halfspace().is_some() || shape.as_trimesh().is_some() || shape.as_heightfield().is_some() {
            return None;
        }
        let aabb = shape.compute_aabb(&collider.position);
        let half = aabb.half_extents();
        let center = aabb.center();
        let volume = 8.0 * half.x * half.y * half.z;
        (volume.is_finite() && volume > 0.0).then(|| BoxPart { volume, half_extents: (half.x, half.y, half.z), position: (center.x, center.y, center.z) })
    }).collect();

    let total_volume: f32 = boxes.iter().map(|b| b.volume).sum();
    if total_volume <= 0.0 {
        // No box geometry to derive inertia from — fall back to something proportional
        // to mass rather than a fixed constant, so it's at least in the right ballpark.
        let fallback = mass * 2.0;
        return Vector3::new(fallback, fallback, fallback);
    }

    let mut inertia = Vector3::new(0.0f32, 0.0, 0.0);
    for b in &boxes {
        let part_mass = mass * (b.volume / total_volume);
        let (hx, hy, hz) = b.half_extents;

        // Inertia of a solid box of this part's mass about its own center.
        let own_ixx = (part_mass / 3.0) * (hy * hy + hz * hz);
        let own_iyy = (part_mass / 3.0) * (hx * hx + hz * hz);
        let own_izz = (part_mass / 3.0) * (hx * hx + hy * hy);

        // Parallel-axis theorem: account for this part sitting away from the body's COM.
        let offset = Vector3::new(b.position.0, b.position.1, b.position.2) - center_of_mass;
        let (ox, oy, oz) = (offset.x, offset.y, offset.z);

        inertia.x += own_ixx + part_mass * (oy * oy + oz * oz);
        inertia.y += own_iyy + part_mass * (ox * ox + oz * oz);
        inertia.z += own_izz + part_mass * (ox * ox + oy * oy);
    }

    inertia
}

/// A node's body and colliders (see `engine::scene_manager::physics_bridge`)
/// plus the position and rotation it was spawned at - what its body is made
/// from, extracted once at spawn time the same way
/// `render_bridge::register_static_model` extracts `Transform3D`+`Model` into
/// an `InstanceData` for rendering. Collected on
/// `SceneContent::physics_bodies` while the scene is built, and made into
/// bodies when its physics starts (see `PhysicsWorld::start`) - a node
/// spawned after that gets its body right away.
#[derive(Clone)]
pub struct PhysicsObjectDef {
    pub id: String,
    pub position: Vector3<f32>,
    pub rotation: UnitQuaternion<f32>,
    /// Rapier's body - the node's `RigidBodyBuilder`, or the one its
    /// colliders imply (see `body::implied_body`).
    pub body: RigidBodyBuilder,
    /// Its colliders, read from their models already.
    pub colliders: Vec<body::Collider>,
    /// Its node's `Layers` - what sensors see it as.
    pub layers: Vec<String>,
    /// A body that follows its node's `Transform3D` instead of moving on
    /// its own - a kinematic position-based one.
    pub follows_node: bool,
}

/// The bit each layer name (see `body::Layers`) got, in the order they were
/// first seen - bit 0 is every node without layers.
#[derive(Default)]
pub(crate) struct LayerBits {
    names: Vec<String>,
}

impl LayerBits {
    fn bits(&mut self, names: &[String]) -> Group {
        let mut bits = Group::empty();
        for name in names {
            let index = match self.names.iter().position(|known| known == name) {
                Some(index) => index,
                None if self.names.len() < 31 => {
                    self.names.push(name.clone());
                    self.names.len() - 1
                }
                None => {
                    eprintln!("physics: layer '{name}' left out - 31 layers at most");
                    continue;
                }
            };
            bits |= Group::from_bits_truncate(1 << (index + 1));
        }
        bits
    }

    /// A collider's groups: it's in its node's `layers` (bit 0 without
    /// any); a sensor sees only what it `detects`, anything else everything.
    fn groups(&mut self, layers: &[String], collider: &body::Collider) -> InteractionGroups {
        let memberships = if layers.is_empty() { Group::GROUP_1 } else { self.bits(layers) };
        let filter = if collider.is_sensor() && !collider.detects.is_empty() { self.bits(&collider.detects) } else { Group::ALL };
        InteractionGroups::new(memberships, filter)
    }
}

/// Seeds `collider_set`/`rigidbody_set` from the scene's `PhysicsObjectDef`s
/// (see that type's own doc comment) - one body each, with its colliders.
pub(crate) fn load_physics_from_definitions(defs: &[PhysicsObjectDef], collider_set: &mut ColliderSet, rigidbody_set: &mut RigidBodySet, physics_handlers: &mut HashMap<String, Option<PhysicsData>>, layers: &mut LayerBits, debugged: &mut HashMap<ColliderHandle, body::ColliderDebug>) {
    for def in defs {
        // Rapier's body as the game wrote it - placed where its node is.
        let rigid_body = def.body.clone()
            .position(Isometry::from_parts(def.position.into(), def.rotation))
            .build();
        let rigidbody_handle = rigidbody_set.insert(rigid_body);

        let mut collider_handles: Vec<ColliderHandle> = Vec::new();
        for collider_data in &def.colliders {
            // Read from its model when its node spawned (see
            // scene_manager::physics_bridge) - one still a stand-in never was.
            if crate::physics::model_shape::ModelPlaceholder::of(&collider_data.builder).is_some() {
                eprintln!("node '{}': collider read from a model wasn't loaded - left out", def.id);
                continue;
            }
            let mut builder = collider_data.builder.clone();
            // Its node's layers - unless the game gave it groups of its own.
            if builder.collision_groups == InteractionGroups::all() {
                builder = builder.collision_groups(layers.groups(&def.layers, collider_data));
            }
            // Also sees bodies an animation holds still - static ones it
            // doesn't, or every zone on the ground would detect the ground.
            if builder.is_sensor {
                builder.active_collision_types |= ActiveCollisionTypes::KINEMATIC_KINEMATIC;
            }
            let handle = collider_set.insert_with_parent(builder.build(), rigidbody_handle, rigidbody_set);
            if let Some(debug) = collider_data.debug {
                debugged.insert(handle, debug);
            }
            collider_handles.push(handle);
        }

        physics_handlers.insert(def.id.clone(), Some(PhysicsData { rigidbody_handle, collider_handles }));
    }
}

