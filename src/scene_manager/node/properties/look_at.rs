/// Keeps a node turned to face another node (by id) - its -Z (forward) at
/// it, +Y up, like Godot's `look_at` - every frame, after the hierarchy's
/// placed both (see `SceneNodes::resolve_hierarchy`). Most useful on a
/// camera.
///
/// A model's front is +Z (glTF's, and Blender's export), so a model with
/// this on turns its back to the target - as in Godot. Give its mesh a half
/// turn about Y in the model to have it face the target instead, or turn it
/// yourself with `scene_nodes::model_facing`.
#[derive(Debug, Clone)]
pub struct LookAt {
    pub target: String,
}
