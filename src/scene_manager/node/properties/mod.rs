//! The engine's own `Node` properties - one file each. Everything is
//! re-exported here, so `node::properties::Material` etc. work as before.

pub mod camera_lens;
pub mod light;
pub mod line;
pub mod look_at;
pub mod material;
pub mod mesh;
pub mod model;
pub mod origin_anchor;
pub mod parent;
pub mod shape_2d;
pub mod spawned_live;
pub mod text;
pub mod transform;
pub mod ui_layer;

pub use camera_lens::CameraLens;
pub use light::{Light, LightKind};
pub use line::{Line, LinePoint, LineSpace};
pub use look_at::LookAt;
pub use material::{Material, TextureSpace};
pub use mesh::{Mesh, MeshShape};
pub use model::{Model, ModelMesh};
pub use origin_anchor::OriginAnchor;
pub use parent::{Parent, Space};
pub use shape_2d::{Shape2D, ShapeKind};
pub use spawned_live::SpawnedLive;
pub use text::Text;
pub use transform::{degrees_from_rotation, rotation_from_degrees, Transform2D, Transform3D};
pub use ui_layer::UiLayer;
