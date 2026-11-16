//! What a built scene needs loaded, and what its loading thread hands back.

use std::collections::HashSet;

use crate::asset_manager::models::ModelLoad;
use crate::rendering::enviroment::environment::{Environment, PreparedEnvironment};
use crate::rendering::models::model::Model as LoadedModel;
use crate::asset_manager::collision::FileMeshes;

/// What a built scene needs loaded before it starts - see `Scene::assets_to_load`.
pub(crate) struct SceneAssets {
    /// Models not loaded yet.
    pub models: Vec<ModelLoad>,
    /// Every model key the scene uses, loaded already or not.
    pub keys: HashSet<String>,
    /// Model files colliders read meshes from, not read yet.
    pub collision_meshes: Vec<String>,
    /// Every texture its nodes' materials use.
    pub(crate) textures: Vec<String>,
}

/// Everything a waiting scene uses - kept loaded when another scene opens
/// before it does.
#[derive(Default, Clone)]
pub(crate) struct UsedAssets {
    pub models: HashSet<String>,
    pub images: Vec<String>,
    pub textures: Vec<String>,
}

impl UsedAssets {
    pub(crate) fn extend(&mut self, other: &UsedAssets) {
        self.models.extend(other.models.iter().cloned());
        self.images.extend(other.images.iter().cloned());
        self.textures.extend(other.textures.iter().cloned());
    }
}

/// What a scene's loading thread hands back.
pub(crate) struct LoadedAssets {
    pub models: Vec<(String, Result<LoadedModel, String>)>,
    pub collision_meshes: Vec<(String, Result<FileMeshes, String>)>,
    pub images: Vec<(String, Result<crate::asset_manager::Image, String>)>,
    pub textures: Vec<(String, Result<crate::asset_manager::ModelTexture, String>)>,
    pub environment: Option<(Environment, PreparedEnvironment)>,
}

impl LoadedAssets {
    pub(crate) fn nothing() -> Self {
        Self { models: Vec::new(), collision_meshes: Vec::new(), images: Vec::new(), textures: Vec::new(), environment: None }
    }
}
