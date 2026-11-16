//! Collision geometry read from model files - `app.assets.collision_meshes`
//! - and `Meshes`, which picks meshes of a model by name (for
//! `Collider::from_model`, `Model::hide_meshes`).

use std::collections::HashMap;
use std::sync::Arc;

use nalgebra::{Point3, Vector3};

use crate::scene_manager::node::properties::Transform3D;

/// Some of a model file's meshes, by their names in the file (as
/// `ModelMesh::name` has them - the object's name in Blender):
///
///     Meshes::all()                       // every mesh
///     Meshes::named(["rock_col"])         // just these
///     Meshes::prefix("col_")              // col_base, col_top, ...
///
/// For a collider (see `Collider::from_model`) the meshes picked are merged
/// into one shape - or, with `.each()`, made into one collider apiece:
///
///     Meshes::prefix("col_").each()
#[derive(Debug, Clone, PartialEq)]
pub struct Meshes {
    pick: Pick,
    each: bool,
}

#[derive(Debug, Clone, PartialEq)]
enum Pick {
    All,
    Named(Vec<String>),
    Prefix(String),
}

impl Meshes {
    /// Every mesh of the file.
    pub fn all() -> Self {
        Self { pick: Pick::All, each: false }
    }

    /// The meshes called one of `names`.
    pub fn named<S: Into<String>>(names: impl IntoIterator<Item = S>) -> Self {
        Self { pick: Pick::Named(names.into_iter().map(Into::into).collect()), each: false }
    }

    /// The meshes whose names start with `prefix`.
    pub fn prefix(prefix: impl Into<String>) -> Self {
        Self { pick: Pick::Prefix(prefix.into()), each: false }
    }

    /// One collider per mesh picked, instead of one for them all merged - a
    /// shape made of the convex pieces modeled for it (see
    /// `Collider::from_model`).
    pub fn each(mut self) -> Self {
        self.each = true;
        self
    }

    /// Whether it makes one collider per mesh (see `each`).
    pub fn is_each(&self) -> bool {
        self.each
    }

    /// Whether the mesh called `name` is one of these.
    pub fn matches(&self, name: &str) -> bool {
        match &self.pick {
            Pick::All => true,
            Pick::Named(names) => names.iter().any(|known| known == name),
            Pick::Prefix(prefix) => name.starts_with(prefix.as_str()),
        }
    }

    /// A short text naming the meshes picked - for messages, and the key of
    /// a model's copy with them hidden.
    pub(crate) fn describe(&self) -> String {
        match &self.pick {
            Pick::All => "all".to_owned(),
            Pick::Named(names) => format!("named {}", names.join(",")),
            Pick::Prefix(prefix) => format!("prefix {prefix}"),
        }
    }
}

/// Collision geometry: what a `Collider::from_model`'s `ModelShape` is
/// made from - a model's meshes, already placed and scaled as its node
/// draws them.
#[derive(Debug, Clone, Default)]
pub struct CollisionMesh {
    /// Its mesh's name in the file - the names merged, joined by commas,
    /// for several.
    pub name: String,
    /// Its points, in its body's frame.
    pub points: Vec<Point3<f32>>,
    /// Its triangles: three indices into `points` each.
    pub triangles: Vec<[u32; 3]>,
}

/// One model file's meshes as collision geometry, unscaled - its own space.
pub(crate) type FileMeshes = Arc<Vec<(String, Vec<Vector3<f32>>, Vec<[u32; 3]>)>>;

/// Collision geometry read from model files, by path - `app.assets.
/// collision_meshes`. Each file is read once - by a scene's loading step
/// along with its models (see `Scene::will_use_collision_meshes` for one a
/// node spawned later uses), or on the spot the first time a node needs it
/// - and kept while the game runs.
#[derive(Default)]
pub struct CollisionMeshCache {
    files: HashMap<String, Result<FileMeshes, String>>,
}

impl CollisionMeshCache {
    /// Whether the file at `path` was read already (or failed to be).
    pub fn is_loaded(&self, path: &str) -> bool {
        self.files.contains_key(path)
    }

    /// The names of the meshes in the file at `path` - None until it's read.
    pub fn mesh_names(&self, path: &str) -> Option<Vec<&str>> {
        let meshes = self.files.get(path)?.as_ref().ok()?;
        Some(meshes.iter().map(|(name, _, _)| name.as_str()).collect())
    }

    /// The file at `path`'s meshes - read now if they weren't yet.
    pub(crate) fn get_or_load(&mut self, path: &str) -> Result<FileMeshes, String> {
        self.files.entry(path.to_owned()).or_insert_with(|| read_file(path)).clone()
    }

    /// Takes what a scene's loading thread read.
    pub(crate) fn adopt(&mut self, loaded: Vec<(String, Result<FileMeshes, String>)>) {
        for (path, meshes) in loaded {
            if let Err(error) = &meshes {
                eprintln!("collision meshes of '{path}' couldn't be read: {error}");
            }
            self.files.insert(path, meshes);
        }
    }
}

/// Reads the file at `path`'s meshes (see `CollisionMeshCache`).
pub(crate) fn read_file(path: &str) -> Result<FileMeshes, String> {
    super::gltf::load_collision_meshes(path).map(Arc::new).map_err(|error| error.to_string())
}

/// `meshes`' meshes of a file, as its node has them: `model` (the model's
/// own transform, see `Model::transform`) then `node_scale` applied - as
/// `render_bridge::place` puts them on screen, without the node's position
/// and rotation (its body's). One `CollisionMesh` for all of them, or one
/// each with `Meshes::each`; none when nothing matches.
pub(crate) fn pick(file: &FileMeshes, meshes: &Meshes, model: &Transform3D, node_scale: Vector3<f32>) -> Vec<CollisionMesh> {
    let scale = node_scale.component_mul(&model.scale);
    let offset = model.position.component_mul(&node_scale);
    let place = |point: &Vector3<f32>| Point3::from(offset + model.rotation * point.component_mul(&scale));
    let picked = file.iter().filter(|(name, _, _)| meshes.matches(name));
    if meshes.each {
        return picked.map(|(name, points, triangles)| CollisionMesh {
            name: name.clone(),
            points: points.iter().map(place).collect(),
            triangles: triangles.clone(),
        }).collect();
    }
    let mut merged = CollisionMesh::default();
    let mut names = Vec::new();
    for (name, points, triangles) in picked {
        let base = merged.points.len() as u32;
        merged.points.extend(points.iter().map(place));
        merged.triangles.extend(triangles.iter().map(|[a, b, c]| [a + base, b + base, c + base]));
        names.push(name.as_str());
    }
    if names.is_empty() {
        return Vec::new();
    }
    merged.name = names.join(",");
    vec![merged]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file() -> FileMeshes {
        let triangle = |x: f32| (vec![Vector3::new(x, 0.0, 0.0), Vector3::new(x + 1.0, 0.0, 0.0), Vector3::new(x, 1.0, 0.0)], vec![[0, 1, 2]]);
        let (a, at) = triangle(0.0);
        let (b, bt) = triangle(5.0);
        let (c, ct) = triangle(10.0);
        Arc::new(vec![("body".to_owned(), a, at), ("col_a".to_owned(), b, bt), ("col_b".to_owned(), c, ct)])
    }

    #[test]
    fn picks_by_name_and_prefix() {
        assert!(Meshes::all().matches("anything"));
        assert!(Meshes::named(["body"]).matches("body"));
        assert!(!Meshes::named(["body"]).matches("col_a"));
        assert!(Meshes::prefix("col_").matches("col_b"));
        assert!(!Meshes::prefix("col_").matches("body"));
    }

    #[test]
    fn merges_unless_each() {
        let merged = pick(&file(), &Meshes::prefix("col_"), &Transform3D::default(), Vector3::repeat(1.0));
        assert_eq!(merged.len(), 1);
        assert_eq!(merged[0].name, "col_a,col_b");
        assert_eq!(merged[0].points.len(), 6);
        // The second mesh's triangle points at its own points.
        assert_eq!(merged[0].triangles[1], [3, 4, 5]);

        let each = pick(&file(), &Meshes::prefix("col_").each(), &Transform3D::default(), Vector3::repeat(1.0));
        assert_eq!(each.len(), 2);
        assert_eq!(each[1].name, "col_b");
        assert_eq!(each[1].triangles[0], [0, 1, 2]);

        assert!(pick(&file(), &Meshes::named(["nothing"]), &Transform3D::default(), Vector3::repeat(1.0)).is_empty());
    }

    #[test]
    fn placed_as_drawn() {
        let model = Transform3D { position: Vector3::new(0.0, 1.0, 0.0), scale: Vector3::repeat(2.0), ..Default::default() };
        let picked = pick(&file(), &Meshes::named(["body"]), &model, Vector3::repeat(3.0));
        // (1, 0, 0) scaled by 2 * 3, the model's offset scaled by the node's 3.
        assert_eq!(picked[0].points[1], Point3::new(6.0, 3.0, 0.0));
    }
}

#[cfg(test)]
mod file_tests {
    use super::*;

    #[test]
    fn reads_a_files_meshes_by_name() {
        let file = read_file("models/f16.glb").expect("f16.glb reads");
        let names: Vec<&str> = file.iter().map(|(name, _, _)| name.as_str()).collect();
        assert!(names.len() > 1, "one entry per mesh, not one merged: {names:?}");
        assert!(file.iter().all(|(_, points, triangles)| !points.is_empty() && !triangles.is_empty()));
    }
}
