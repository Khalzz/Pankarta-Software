//! A plain piece of text.
//!
//!     text("Settings", &TextStyle::default().size(22.0))
//!
//! As wide and tall as its text. For a fixed width (so changing the text
//! later doesn't need a bigger box), chain
//! `.set_size(SizeValue::Pixels(400.0), SizeValue::Fit)`.

use crate::ui::text_style::TextStyle;
use crate::ui::ui_node::UiNode;

/// `content`, looking like `style`.
pub fn text(content: &str, style: &TextStyle) -> UiNode {
    UiNode::label(content, None, None).set_text_style(style)
}
