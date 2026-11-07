use sdl2::rect::Rect;
use sdl2::video::{Window as SdlWindow, WindowPos};
use sdl2::VideoSubsystem;

use super::cursor::CursorIcon;
use super::mode::WindowMode;
use crate::app::Size;

/*
  # Window

  The engine's one window. `App::new` creates it hidden - 800x600, windowed,
  resizable, titled "Pankarta Software" - and `App::run` shows it, so it can
  be set up first without the user seeing it jump around:

      let mut app = App::new().await?;
      app.window.set_title("My Game");
      app.window.set_size(1280, 720);
      app.window.set_window_mode(WindowMode::Windowed);
      app.run();

  Every setter also works while the game runs - the renderer and UI follow
  the window's new size at the start of the next frame (see App::run).

  Split by topic: size and window mode in mode.rs, screens in display.rs,
  the mouse cursor in cursor.rs.
*/
pub struct Window {
    pub(super) window: SdlWindow,
    pub(super) video: VideoSubsystem,
    pub(super) mode: WindowMode,
    /// The size asked for with `set_size` - what the window has in
    /// `Windowed`, the screen resolution in `Fullscreen`. Kept while in
    /// `BorderlessFullscreen`, so going back to a window restores it.
    pub(super) requested_size: (u32, u32),
    /// Whether the game chose `requested_size` (`set_size`) - if not,
    /// `Fullscreen` uses the screen's own resolution, not the window's
    /// default 800x600.
    pub(super) size_chosen: bool,
    // Resolved window size in points/logical units. This is the coordinate
    // space every Mouse{Motion, Button} event and every UiTransform/hit-test
    // is in - use this (not pixel_size) for anything UI/logic-facing.
    pub size: Size,
    // The window's real backing-pixel size - on a HiDPI/Retina display this is
    // size scaled up by the display's backing scale factor (2x, 3x, ...), same
    // as SDL's own drawable_size(). Only the GPU surface/render targets should
    // be sized off this - it's not the coordinate space anything else (mouse,
    // UI layout) is in.
    pub pixel_size: Size,
    /// The cursor shown now, and the SDL cursor showing it - kept alive
    /// while it's set. None: SDL's own, the arrow.
    pub(super) cursor: Option<(CursorIcon, sdl2::mouse::Cursor)>,
}

impl Window {
    pub const DEFAULT_TITLE: &'static str = "Pankarta Software";
    pub const DEFAULT_SIZE: (u32, u32) = (800, 600);

    pub(crate) fn new(video: VideoSubsystem) -> Result<Window, String> {
        let (width, height) = Self::DEFAULT_SIZE;
        let window = video.window(Self::DEFAULT_TITLE, width, height)
            .vulkan()
            .allow_highdpi()
            .resizable()
            .hidden()
            .position_centered()
            .build()
            .map_err(|error| format!("the window couldn't be created: {error}"))?;

        let mut window = Window {
            window,
            video,
            mode: WindowMode::Windowed,
            requested_size: (width, height),
            size_chosen: false,
            size: Size { width, height },
            pixel_size: Size { width, height },
            cursor: None,
        };
        window.refresh_size();
        Ok(window)
    }

    /// The SDL window itself - for the renderer's surface.
    pub(crate) fn sdl_window(&self) -> &SdlWindow {
        &self.window
    }

    /// Shows the window - `App::run` does this once everything's set up.
    pub(crate) fn show(&mut self) {
        self.window.show();
    }

    /// Re-reads `size` and `pixel_size` from the window - see App::resize.
    pub(crate) fn refresh_size(&mut self) {
        let (mut width, mut height) = self.window.size();
        let (mut pixel_width, mut pixel_height) = self.window.drawable_size();
        // Still hidden (before `App::run`), a window set fullscreen keeps its
        // old size - SDL makes it fullscreen when it's shown. The size it'll
        // have then, so code before `run` reads the right one. (Its real
        // pixels - a HiDPI screen's are more - are read once it's shown.)
        if !self.is_shown() {
            if let Some((full_width, full_height)) = self.fullscreen_size() {
                (width, height) = (full_width, full_height);
                (pixel_width, pixel_height) = (full_width, full_height);
            }
        }
        self.size = Size { width, height };
        self.pixel_size = Size { width: pixel_width, height: pixel_height };
    }

    fn is_shown(&self) -> bool {
        self.window.window_flags() & sdl2::sys::SDL_WindowFlags::SDL_WINDOW_SHOWN as u32 != 0
    }

    pub fn title(&self) -> &str {
        self.window.title()
    }

    pub fn set_title(&mut self, title: &str) {
        if let Err(error) = self.window.set_title(title) {
            eprintln!("Couldn't set the window title: {error}");
        }
    }

    /// Whether the user can resize the window by dragging its edges (in
    /// `Windowed`). On by default.
    pub fn set_resizable(&mut self, resizable: bool) {
        self.window.set_resizable(resizable);
    }

    pub(super) fn center_on(&mut self, bounds: Rect) {
        let (width, height) = self.window.size();
        let x = bounds.x() + (bounds.width() as i32 - width as i32) / 2;
        let y = bounds.y() + (bounds.height() as i32 - height as i32) / 2;
        self.window.set_position(WindowPos::Positioned(x), WindowPos::Positioned(y));
    }
}
