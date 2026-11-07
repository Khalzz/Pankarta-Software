//! A node's screen UI - a tree of `UiNode`s it owns, laid out over the
//! window and drawn above the scene:
//!
//!     Node::new("hud")
//!         .with_property(UiLayer::new(
//!             UiNode::container()
//!                 .set_padding(10.0)
//!                 .set_child("fps", text("-", &TextStyle::default())),
//!         ))
//!         .with_behavior(Hud)
//!
//! Its behaviors change it through the property - paths are from the layer's
//! root, "panel/title" being the root's child "panel"'s child "title":
//!
//!     if let Some(label) = node.get_property_mut::<UiLayer>().and_then(|ui| ui.label_mut("fps")) {
//!         label.set_text(format!("{fps} fps"));
//!     }
//!
//! It lives and dies with its node: despawning the node takes its UI away,
//! and a scene switch takes every layer with the scene. Layers stack by
//! `z_index` (higher on top); one that `blocks_input` (a modal) keeps clicks
//! and hover from reaching the layers under it.

use crate::ui::components::container::Container;
use crate::ui::components::image::ImageNode;
use crate::ui::components::label::Label;
use crate::ui::ui_node::UiNode;

/// See the module doc.
pub struct UiLayer {
    pub(crate) root: UiNode,
    pub(crate) z_index: i32,
    pub(crate) blocks_input: bool,
    pub(crate) visible: bool,
    /// The room (UI units) `root` was last laid out in - None until it's
    /// first drawn, and cleared by `relayout`. Laid out again when the room
    /// changes (a resize, a new UI scale).
    pub(crate) laid_out_for: Option<(f32, f32)>,
}

impl UiLayer {
    /// A layer showing `root`, laid out against the whole window.
    pub fn new(root: UiNode) -> Self {
        Self { root, z_index: 0, blocks_input: false, visible: true, laid_out_for: None }
    }

    /// Where it stacks among the scene's layers - higher is drawn over lower
    /// (a pause menu over the HUD). 0 by default; equal ones stack by node id.
    pub fn z_index(mut self, z_index: i32) -> Self {
        self.z_index = z_index;
        self
    }

    /// Keeps hover and clicks from reaching the layers under it while it's
    /// visible - a modal. Off by default.
    pub fn blocks_input(mut self, blocks: bool) -> Self {
        self.blocks_input = blocks;
        self
    }

    /// Shown or not, to start with - see `set_visible`.
    pub fn visible(mut self, visible: bool) -> Self {
        self.visible = visible;
        self
    }

    /// Shows or hides the whole layer - hidden, it's not drawn and takes no
    /// input (it doesn't block any either).
    pub fn set_visible(&mut self, visible: bool) {
        self.visible = visible;
    }

    pub fn is_visible(&self) -> bool {
        self.visible
    }

    pub fn set_z_index(&mut self, z_index: i32) {
        self.z_index = z_index;
    }

    pub fn set_blocks_input(&mut self, blocks: bool) {
        self.blocks_input = blocks;
    }

    /// Lays the layer out again next frame - after changing sizes or
    /// positions in it (`set_size`/`set_position` values), or adding nodes
    /// to it. A resize does it on its own.
    pub fn relayout(&mut self) {
        self.laid_out_for = None;
    }

    /// The node at `path` - "" is the root.
    pub fn get(&self, path: &str) -> Option<&UiNode> {
        self.root.find(path)
    }

    /// The node at `path`, to change - for what every node has (`set_active`,
    /// `set_enabled`, `set_alpha`, `update_style`), and widgets' values
    /// (`input_value`, `toggle_value`, `dropdown_selected`...).
    pub fn get_mut(&mut self, path: &str) -> Option<&mut UiNode> {
        self.root.find_mut(path)
    }

    /// The text node at `path` (see `widgets::text`) - None if there's none,
    /// or it isn't one.
    pub fn label_mut(&mut self, path: &str) -> Option<&mut Label> {
        self.root.find_mut(path)?.as_label_mut()
    }

    /// The image node at `path` - None if there's none, or it isn't one.
    pub fn image_mut(&mut self, path: &str) -> Option<&mut ImageNode> {
        self.root.find_mut(path)?.as_image_mut()
    }

    /// The container at `path` - None if there's none, or it isn't one.
    pub fn container_mut(&mut self, path: &str) -> Option<&mut Container> {
        self.root.find_mut(path)?.as_container_mut()
    }

    /// Lays `root` out in a `width` x `height` room, if it wasn't already.
    pub(crate) fn lay_out(&mut self, width: f32, height: f32) {
        if self.laid_out_for != Some((width, height)) {
            self.root.resolve(width, height);
            self.laid_out_for = Some((width, height));
        }
    }
}
