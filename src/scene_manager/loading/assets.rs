//! What a built scene needs loaded, and what its loading thread hands back.

use std::collections::{HashMap, HashSet};

use crate::asset_manager::models::ModelLoad;
use crate::rendering::enviroment::environment::{Environment, PreparedEnvironment};
use crate::rendering::models::model::Model as LoadedModel;
use crate::scene_manager::physics_bridge::TrimeshCache;

/// What a built scene needs loaded before it starts - see `Scene::assets_to_load`.
pub(crate) struct SceneAssets {
    /// Models not loaded yet.
    pub models: Vec<ModelLoad>,
    /// Every model key the scene uses, loaded already or not.
    pub keys: HashSet<String>,
    /// Model files mesh colliders read.
    pub trimeshes: Vec<String>,
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
    pub trimeshes: TrimeshCache,
    pub images: Vec<(String, Result<crate::asset_manager::Image, String>)>,
    pub textures: Vec<(String, Result<crate::asset_manager::ModelTexture, String>)>,
    pub environment: Option<(Environment, PreparedEnvironment)>,
}

impl LoadedAssets {
    pub(crate) fn nothing() -> Self {
        Self { models: Vec::new(), trimeshes: HashMap::new(), images: Vec::new(), textures: Vec::new(), environment: None }
    }
}
