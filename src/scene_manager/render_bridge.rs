//! From a node's `Mesh` or `Model` to the renderer's instances. One node can
//! draw several: a model's file is one instance (under the node's id), each
//! of its parts another (under "node#part"). See docs/meshes_and_models.md.

use std::borrow::Cow;

use nalgebra::Vector3;

use crate::app::App;
use crate::asset_manager::models::{self, DoubleSidedMeshes, ModelSource};
use crate::rendering::instance_management::{create_instance_buffer, InstanceData, ModelDataInstance};
use crate::rendering::shader_library::STANDARD;

use super::node::properties::{Material, Mesh, Model, Transform3D};
use super::node::Node;
use super::scene::Scene;

/// One instance a node draws.
pub(crate) struct Draw<'a> {
    /// Its key in `SceneContent::renderizable_instances`.
    pub(crate) id: String,
    pub(crate) source: ModelSource,
    pub(crate) double_sided: Cow<'a, DoubleSidedMeshes>,
    pub(crate) variant: Option<&'a str>,
    /// Where it sits relative to its node.
    pub(crate) local: Transform3D,
    pub(crate) material: Option<Cow<'a, Material>>,
    pub(crate) casts_shadows: bool,
    pub(crate) receives_shadows: bool,
}

impl Draw<'_> {
    /// What its loaded copy is kept under.
    pub(crate) fn model_ref(&self) -> String {
        models::variant_key(&self.source.key(), self.variant)
    }

    pub(crate) fn material_key(&self) -> String {
        self.material.as_ref().map_or_else(|| STANDARD.to_owned(), |material| material.key(&self.id))
    }
}

pub(crate) fn has_drawable(node: &Node) -> bool {
    node.get_property::<Mesh>().is_some() || node.get_property::<Model>().is_some()
}

/// Everything `node` draws - nothing for a hidden part.
pub(crate) fn draws(node: &Node) -> Vec<Draw<'_>> {
    let mut draws = Vec::new();
    if let Some(mesh) = node.get_property::<Mesh>() {
        if let Some(draw) = mesh_draw(node.id.clone(), mesh, &Transform3D::default(), None, true, true) {
            draws.push(draw);
        }
    }
    if let Some(model) = node.get_property::<Model>() {
        draws.extend(model_draws(&node.id, model));
    }
    draws
}

/// Everything `model` draws, on a node `id`.
pub(crate) fn model_draws<'a>(id: &str, model: &'a Model) -> Vec<Draw<'a>> {
    let mut draws = Vec::new();
    if let Some((source, double_sided, variant, material)) = model.file_draw() {
        draws.push(Draw {
            id: id.to_owned(),
            source,
            double_sided: Cow::Borrowed(double_sided),
            variant,
            local: model.transform,
            material,
            casts_shadows: model.casts_shadows,
            receives_shadows: model.receives_shadows,
        });
    }
    for (name, mesh) in model.parts() {
        let part_id = format!("{id}#{name}");
        if let Some(draw) = mesh_draw(part_id, mesh, &model.transform, model.material.as_ref(), model.casts_shadows, model.receives_shadows) {
            draws.push(draw);
        }
    }
    draws
}

fn mesh_draw<'a>(id: String, mesh: &'a Mesh, within: &Transform3D, fallback: Option<&'a Material>, casts: bool, receives: bool) -> Option<Draw<'a>> {
    if !mesh.visible {
        return None;
    }
    let (source, geometry_scale) = mesh.geometry();
    let own = Transform3D { scale: mesh.transform.scale.component_mul(&geometry_scale), ..mesh.transform };
    Some(Draw {
        id,
        source,
        double_sided: Cow::Owned(DoubleSidedMeshes::None),
        variant: None,
        local: place(within, &own),
        material: mesh.material.as_ref().or(fallback).map(Cow::Borrowed),
        casts_shadows: casts && mesh.casts_shadows,
        receives_shadows: receives && mesh.receives_shadows,
    })
}

/// `local` placed inside `outer` - moved, turned and scaled by it.
pub(crate) fn place(outer: &Transform3D, local: &Transform3D) -> Transform3D {
    Transform3D {
        position: outer.position + outer.rotation * local.position.component_mul(&outer.scale),
        rotation: outer.rotation * local.rotation,
        scale: outer.scale.component_mul(&local.scale),
    }
}

/// How much bigger than its node a node's geometry is drawn - what a
/// collider read from its model file is scaled by.
pub(crate) fn geometry_scale(node: &Node) -> Vector3<f32> {
    if let Some(model) = node.get_property::<Model>() {
        return model.scale();
    }
    match node.get_property::<Mesh>() {
        Some(mesh) => mesh.transform.scale.component_mul(&mesh.geometry().1),
        None => Vector3::new(1.0, 1.0, 1.0),
    }
}

/// Takes all of node `id`'s instances off the renderer. Their models stay
/// loaded; the shared instance buffers keep their size, only the counts drop.
pub fn unregister_model(scene: &mut Scene, app: &mut App, id: &str) {
    let ids: Vec<String> = scene.content.renderizable_instances.iter()
        .filter(|(_, instance)| instance.node == id)
        .map(|(instance_id, _)| instance_id.clone())
        .collect();
    for instance_id in ids {
        unregister_instance(scene, app, &instance_id);
    }
}

fn unregister_instance(scene: &mut Scene, app: &mut App, instance_id: &str) {
    let Some(removed) = scene.content.renderizable_instances.remove(instance_id) else { return };
    let remaining = scene.content.renderizable_instances.values()
        .filter(|instance| instance.model_ref == removed.model_ref)
        .count() as u32;
    if let Some(model) = app.assets.models.drawn.get_mut(&removed.model_ref) {
        model.instance_count = remaining;
    }
}

/// Puts every instance node `id` draws on the renderer.
pub fn register_static_model(scene: &mut Scene, app: &mut App, id: &str) -> Result<(), String> {
    let node = scene.content.nodes.get(id).ok_or_else(|| format!("no node named '{id}' to register"))?;
    let transform = *node.transform_3d().ok_or_else(|| format!("node '{id}' has no Transform3D property"))?;
    let pending: Vec<Pending> = draws(node).iter().map(|draw| Pending::of(draw, &transform, id, app)).collect();
    let mut errors = Vec::new();
    for pending in pending {
        if let Err(error) = register_instance(scene, app, pending) {
            errors.push(error);
        }
    }
    if errors.is_empty() { Ok(()) } else { Err(errors.join("; ")) }
}

/// Every frame: each drawn instance onto its node's current transform,
/// material and shadows - and a part added, removed, hidden or shown since
/// the last frame put on or taken off the renderer.
pub(crate) fn sync_instances(scene: &mut Scene, app: &mut App) {
    let mut added = Vec::new();
    let mut gone = Vec::new();
    {
        let content = &mut scene.content;
        for node in content.nodes.iter() {
            let Some(transform) = node.transform_3d() else { continue };
            if !has_drawable(node) {
                continue;
            }
            let draws = draws(node);
            for draw in &draws {
                match content.renderizable_instances.get_mut(&draw.id) {
                    Some(instance) => {
                        instance.transform = place(transform, &draw.local);
                        instance.casts_shadows = draw.casts_shadows;
                        instance.receives_shadows = draw.receives_shadows;
                        let key = draw.material_key();
                        if instance.material != key {
                            instance.material = key;
                        }
                        let model_ref = draw.model_ref();
                        if instance.model_ref != model_ref {
                            gone.push(draw.id.clone());
                            added.push(Pending::of(draw, transform, &node.id, app));
                        }
                    }
                    None => added.push(Pending::of(draw, transform, &node.id, app)),
                }
            }
            for (instance_id, instance) in content.renderizable_instances.iter() {
                if instance.node == node.id && !draws.iter().any(|draw| draw.id == *instance_id) {
                    gone.push(instance_id.clone());
                }
            }
        }
    }
    for instance_id in gone {
        unregister_instance(scene, app, &instance_id);
    }
    for pending in added {
        if let Err(error) = register_instance(scene, app, pending) {
            eprintln!("{error}");
        }
    }
}

/// An instance to put on the renderer, with what loading its model takes.
struct Pending {
    node: String,
    instance: String,
    data: InstanceData,
    variant: Option<String>,
    load: Result<models::ModelLoad, String>,
}

impl Pending {
    fn of(draw: &Draw, node_transform: &Transform3D, node: &str, app: &App) -> Self {
        Self {
            node: node.to_owned(),
            instance: draw.id.clone(),
            data: InstanceData {
                transform: place(node_transform, &draw.local),
                model_ref: draw.model_ref(),
                node: node.to_owned(),
                material: draw.material_key(),
                casts_shadows: draw.casts_shadows,
                receives_shadows: draw.receives_shadows,
            },
            variant: draw.variant.map(str::to_owned),
            load: app.assets.model_load(&draw.source, &draw.double_sided),
        }
    }
}

/// Puts one instance on the renderer. Its model is normally loaded already -
/// by the scene's loading step, or by another node using it; one nothing
/// loaded yet is loaded here, on the spot (`Scene::will_use` has it loaded
/// with the scene instead).
///
/// The model's instance buffer is one fixed-size GPU buffer for all its
/// instances, so it's rebuilt from all of them each time one is added.
fn register_instance(scene: &mut Scene, app: &mut App, pending: Pending) -> Result<(), String> {
    let Pending { node, instance, data, variant, load } = pending;
    let model_ref = data.model_ref.clone();
    let loaded_model = match app.assets.models.drawn.remove(&model_ref) {
        Some(existing) => existing.model,
        None => models::load_scene_model(app, &load.map_err(|error| format!("node '{node}': {error}"))?, variant.as_deref())?,
    };

    scene.content.renderizable_instances.insert(instance, data);

    let instances: Vec<&Transform3D> = scene.content.renderizable_instances.values()
        .filter(|instance| instance.model_ref == model_ref)
        .map(|instance| &instance.transform)
        .collect();
    let instance_count = instances.len() as u32;
    let camera_position = scene.cameras.active().camera.position().coords;
    let instance_buffer = create_instance_buffer(&instances, &app.renderer.device, camera_position);

    app.assets.models.drawn.insert(model_ref, ModelDataInstance {
        model: loaded_model,
        instance_buffer,
        instance_count,
        material_ranges: Vec::new(),
        shadow_runs: Vec::new(),
        mesh_materials: Default::default(),
        stamp: ModelDataInstance::new_stamp(),
    });
    Ok(())
}

#[cfg(test)]
mod tests {
    use nalgebra::UnitQuaternion;

    use super::*;

    #[test]
    fn a_part_is_placed_inside_its_model() {
        let model = Transform3D {
            position: Vector3::new(1.0, 0.0, 0.0),
            rotation: UnitQuaternion::from_axis_angle(&Vector3::y_axis(), std::f32::consts::FRAC_PI_2),
            scale: Vector3::new(2.0, 2.0, 2.0),
        };
        let part = Transform3D { position: Vector3::z(), ..Default::default() };
        let placed = place(&model, &part);
        assert!((placed.position - Vector3::new(3.0, 0.0, 0.0)).magnitude() < 1e-4);
        assert_eq!(placed.scale, Vector3::new(2.0, 2.0, 2.0));
    }

    #[test]
    fn a_model_draws_its_file_and_each_visible_part() {
        let mut model = Model::file("models/f16.glb").part("pod", Mesh::sphere(1.0)).part("gone", Mesh::cube());
        model.part_mut("gone").unwrap().visible = false;
        let ids: Vec<String> = model_draws("jet", &model).into_iter().map(|draw| draw.id).collect();
        assert_eq!(ids, vec!["jet".to_owned(), "jet#pod".to_owned()]);
    }

    #[test]
    fn a_part_without_a_material_uses_the_models() {
        let model = Model::new()
            .material(Material::shader("metal"))
            .part("body", Mesh::cube())
            .part("glass", Mesh::cube().material(Material::shader("glass")));
        let shaders: Vec<String> = model_draws("car", &model).iter().map(|draw| draw.material.as_ref().unwrap().shader.clone()).collect();
        assert_eq!(shaders, vec!["metal".to_owned(), "glass".to_owned()]);
    }

    #[test]
    fn a_sphere_is_the_unit_one_scaled() {
        let model = Model::from(Mesh::sphere(3.0));
        let draws = model_draws("ball", &model);
        assert_eq!(draws[0].local.scale, Vector3::new(6.0, 6.0, 6.0));
        assert_eq!(draws[0].source, ModelSource::Sphere);
    }
}
