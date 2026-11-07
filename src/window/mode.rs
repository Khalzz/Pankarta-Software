//! The window's size and how it sits on its screen - windowed or fullscreen.

use sdl2::video::{DisplayMode, FullscreenType};

use super::window::Window;

/// How the window sits on its screen - see `Window::set_window_mode`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WindowMode {
    /// A normal window with a title bar, at `Window::size`.
    Windowed,
    /// No border, covering the whole screen at the screen's own resolution.
    /// Switching in and out is instant and safe - the usual choice.
    BorderlessFullscreen,
    /// Takes the screen over and changes its resolution to the window's size
    /// (the closest one the screen supports). Switching is slower and can
    /// make the screen go black for a moment.
    /// On macOS this is `BorderlessFullscreen` instead - exclusive fullscreen
    /// there stops Cmd+Tab from leaving the game.
    Fullscreen,
}

impl Window {
    /// The window's size (its current one is the `size` field):
    /// - `Windowed`: resized now, and centered on its screen.
    /// - `Fullscreen`: the screen changes to the closest resolution it supports
    ///   (without a `set_size`, it stays at its own).
    /// - `BorderlessFullscreen`: remembered - the window keeps the screen's
    ///   size, and gets this one when it goes back to `Windowed`.
    pub fn set_size(&mut self, width: u32, height: u32) -> Result<(), String> {
        self.requested_size = (width, height);
        self.size_chosen = true;
        match self.mode {
            WindowMode::Windowed => self.apply_windowed_size()?,
            WindowMode::Fullscreen => {
                // Changing the mode of a window that's already fullscreen
                // changes the screen's resolution but leaves the window at its
                // old size - the game then draws into the old size while laying
                // its UI out for the new one. Out of fullscreen and back in at
                // the new mode, the same way set_window_mode goes in.
                self.window.set_fullscreen(FullscreenType::Off)?;
                self.apply_display_mode()?;
                self.window.set_fullscreen(FullscreenType::True)?;
            }
            WindowMode::BorderlessFullscreen => {}
        }
        self.refresh_size();
        Ok(())
    }

    pub fn window_mode(&self) -> WindowMode {
        self.mode
    }

    pub fn set_window_mode(&mut self, mode: WindowMode) -> Result<(), String> {
        // macOS: SDL's exclusive fullscreen puts the window above everything
        // (and captures the screen when it changes resolution), so Cmd+Tab
        // and Mission Control can't get out of the game. Borderless covers
        // the screen just the same and lets the system switch apps.
        #[cfg(target_os = "macos")]
        let mode = if mode == WindowMode::Fullscreen { WindowMode::BorderlessFullscreen } else { mode };
        match mode {
            WindowMode::Windowed => {
                self.window.set_fullscreen(FullscreenType::Off)?;
                self.apply_windowed_size()?;
            }
            WindowMode::BorderlessFullscreen => {
                self.window.set_fullscreen(FullscreenType::Desktop)?;
            }
            WindowMode::Fullscreen => {
                // The resolution has to be chosen before going fullscreen.
                self.apply_display_mode()?;
                self.window.set_fullscreen(FullscreenType::True)?;
            }
        }
        self.mode = mode;
        self.refresh_size();
        Ok(())
    }

    /// The size a fullscreen mode makes the window - the screen's own for
    /// `BorderlessFullscreen`, the screen mode chosen for `Fullscreen`.
    /// None while `Windowed`.
    pub(super) fn fullscreen_size(&self) -> Option<(u32, u32)> {
        let mode = match self.mode {
            WindowMode::Windowed => return None,
            WindowMode::BorderlessFullscreen => self.video.desktop_display_mode(self.display()).ok()?,
            WindowMode::Fullscreen => self.window.display_mode().ok()?,
        };
        Some((mode.w as u32, mode.h as u32))
    }

    /// The window at `requested_size`, centered on the screen it's on.
    fn apply_windowed_size(&mut self) -> Result<(), String> {
        let (width, height) = self.requested_size;
        self.window.set_size(width, height).map_err(|error| error.to_string())?;
        let bounds = self.video.display_bounds(self.display())?;
        self.center_on(bounds);
        Ok(())
    }

    /// The screen mode `Fullscreen` uses - the one closest to
    /// `requested_size`, or the screen's own (its desktop resolution) when
    /// the game never chose a size.
    fn apply_display_mode(&mut self) -> Result<(), String> {
        let display = self.display();
        let desktop = self.video.desktop_display_mode(display)?;
        if !self.size_chosen {
            return self.set_fullscreen_mode(desktop);
        }
        let (width, height) = self.requested_size;
        let wanted = DisplayMode::new(desktop.format, width as i32, height as i32, desktop.refresh_rate);
        let mode = self.video.closest_display_mode(display, &wanted)?;
        self.set_fullscreen_mode(mode)
    }

    /// SDL_SetWindowDisplayMode, called directly: sdl2's own
    /// `Window::set_display_mode` (0.37) hands SDL a pointer to a copy of the
    /// mode that's already gone by the time SDL reads it - SDL gets garbage,
    /// finds no screen mode like it, and `Fullscreen` silently stays a window.
    fn set_fullscreen_mode(&mut self, mode: DisplayMode) -> Result<(), String> {
        let mode = mode.to_ll();
        // SAFETY: the window is alive (we own it), and `mode` outlives the
        // call - SDL copies it.
        let result = unsafe { sdl2::sys::SDL_SetWindowDisplayMode(self.window.raw(), &mode) };
        if result < 0 { Err(sdl2::get_error()) } else { Ok(()) }
    }
}
