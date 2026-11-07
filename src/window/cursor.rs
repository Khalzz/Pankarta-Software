//! The mouse cursor - its shape, and locking it to the window.

use sdl2::mouse::{Cursor, SystemCursor};

use super::window::Window;

/// The mouse cursor's shape - see `Window::set_cursor_icon`, and
/// `UiNode::set_cursor` for showing one while the mouse is over a node.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CursorIcon {
    /// The plain arrow.
    #[default]
    Default,
    /// A pointing hand - something clickable.
    Pointer,
    /// A text beam - something to type in.
    Text,
    /// Something that can be dragged around.
    Move,
    /// Can't be used right now.
    NotAllowed,
    /// Busy.
    Wait,
    ResizeHorizontal,
    ResizeVertical,
    /// Top-right to bottom-left.
    ResizeNeSw,
    /// Top-left to bottom-right.
    ResizeNwSe,
}

impl Window {
    /// Locks the mouse to the window and hides it - it then reports only
    /// movement (app.input.mouse_rel_x/y), for a camera looked around with the
    /// mouse. Off by default.
    pub fn set_cursor_locked(&mut self, locked: bool) {
        self.video.sdl().mouse().set_relative_mouse_mode(locked);
    }

    pub fn is_cursor_locked(&self) -> bool {
        self.video.sdl().mouse().relative_mouse_mode()
    }

    /// Shows the mouse cursor as `icon` - only re-created when it changes.
    /// The engine sets it every frame from the UI under the mouse (see
    /// `UiNode::set_cursor`), so a game sets it on its nodes instead.
    pub fn set_cursor_icon(&mut self, icon: CursorIcon) {
        if self.cursor.as_ref().is_some_and(|(current, _)| *current == icon) {
            return;
        }
        let system = match icon {
            CursorIcon::Default => SystemCursor::Arrow,
            CursorIcon::Pointer => SystemCursor::Hand,
            CursorIcon::Text => SystemCursor::IBeam,
            CursorIcon::Move => SystemCursor::SizeAll,
            CursorIcon::NotAllowed => SystemCursor::No,
            CursorIcon::Wait => SystemCursor::Wait,
            CursorIcon::ResizeHorizontal => SystemCursor::SizeWE,
            CursorIcon::ResizeVertical => SystemCursor::SizeNS,
            CursorIcon::ResizeNeSw => SystemCursor::SizeNESW,
            CursorIcon::ResizeNwSe => SystemCursor::SizeNWSE,
        };
        if let Ok(cursor) = Cursor::from_system(system) {
            cursor.set();
            self.cursor = Some((icon, cursor));
        }
    }

    pub fn cursor_icon(&self) -> CursorIcon {
        self.cursor.as_ref().map_or(CursorIcon::Default, |(icon, _)| *icon)
    }
}
