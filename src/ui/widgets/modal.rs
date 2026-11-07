//! A dialog's look: a screen-wide backdrop dimming what's behind, with a card
//! in the middle. A modal is a node - its `UiLayer` over the rest
//! (`z_index`), keeping hover and clicks from what's behind it
//! (`blocks_input`). Spawning it opens it; despawning it closes it:
//!
//!     let content = UiNode::container()
//!         .set_child("message", text("Quit the game?", &TextStyle::default()))
//!         .set_child("ok", button("OK", &ButtonStyle::default())
//!             .on_click(|node, scene, _app| scene.despawn(&node.id)));
//!     scene.spawn(Node::new("confirm").with_property(
//!         UiLayer::new(modal(content, &ModalStyle::default())).z_index(100).blocks_input(true),
//!     ));
//!
//! Its nodes are under the layer's root like any other: "card/content/ok".
//! Whether it's open: `scene.get_node("confirm").is_some()`.

use crate::ui::color::{Fill, UiColor};
use crate::ui::ui_node::UiNode;
use crate::ui::ui_transform::{Anchor, PositionValue, SizeValue};
use crate::ui::widgets::card::{card, CardStyle};

#[derive(Clone)]
pub struct ModalStyle {
    /// Covers the whole screen behind the card - usually a translucent dark.
    pub backdrop: Fill,
    /// Blurs what's behind the backdrop, in pixels. 0: off.
    pub backdrop_blur: f32,
    /// The box in the middle holding the content.
    pub card: CardStyle,
    /// About how long (ms) the backdrop's dimming and blur take to fade in
    /// when it opens. 0: shown at once.
    pub fade_in_ms: f32,
}

impl Default for ModalStyle {
    fn default() -> Self {
        Self {
            backdrop: UiColor::Rgba(0, 0, 0, 200).into(),
            backdrop_blur: 15.0,
            card: CardStyle::default().padding(20.0).gap(14.0),
            fade_in_ms: 200.0,
        }
    }
}

impl ModalStyle {
    pub fn backdrop(mut self, backdrop: impl Into<Fill>) -> Self {
        self.backdrop = backdrop.into();
        self
    }

    pub fn backdrop_blur(mut self, radius_px: f32) -> Self {
        self.backdrop_blur = radius_px;
        self
    }

    pub fn card(mut self, card: CardStyle) -> Self {
        self.card = card;
        self
    }

    pub fn fade_in(mut self, ms: f32) -> Self {
        self.fade_in_ms = ms;
        self
    }
}

/// `content` in a card in the middle of a screen-wide backdrop, looking like
/// `style` - the root of a modal's `UiLayer` (see the module doc).
pub fn modal(content: UiNode, style: &ModalStyle) -> UiNode {
    let backdrop = UiNode::container()
        .set_size(SizeValue::Percent(100.0), SizeValue::Percent(100.0))
        .set_position(PositionValue::Start(0.0), PositionValue::Start(0.0))
        .set_child_anchor(Anchor::Center, Anchor::Center)
        .set_background_color(style.backdrop.clone())
        .set_background_blur(style.backdrop_blur)
        .set_child("card", card(&style.card).set_child("content", content));
    if style.fade_in_ms <= 0.0 {
        return backdrop;
    }
    // Starts clear and sharp, and eases into the dimmed, blurred look.
    backdrop
        .set_transition(style.fade_in_ms)
        .set_enter(|enter| enter.set_background_color(UiColor::TRANSPARENT).set_background_blur(0.0))
}
