/// Marks the node the floating origin follows (see `WorldOrigin`,
/// `app.origin`) - usually the player. Once it gets far enough from (0, 0, 0),
/// the whole world is moved back so it's near it again. Without one in the
/// scene, the origin follows the active camera; with more than one, the
/// first found.
#[derive(Debug, Clone, Copy, Default)]
pub struct OriginAnchor;
