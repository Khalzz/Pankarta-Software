//! The screens connected, and which one the window is on.

use sdl2::video::FullscreenType;

use super::mode::WindowMode;
use super::window::Window;

impl Window {
    /// How many screens are connected.
    pub fn display_count(&self) -> i32 {
        self.video.num_video_displays().unwrap_or(1)
    }

    /// Which screen the window is on (0 is the main one).
    pub fn display(&self) -> i32 {
        self.window.display_index().unwrap_or(0)
    }

    /// Moves the window to screen `index` (0 is the main one), centered on it
    /// - in a fullscreen mode it covers that screen instead.
    pub fn set_display(&mut self, index: i32) -> Result<(), String> {
        let count = self.display_count();
        if index < 0 || index >= count {
            return Err(format!("there's no screen {index} - {count} connected"));
        }
        // A fullscreen window can't be moved - out of it, over, and back.
        let mode = self.mode;
        if mode != WindowMode::Windowed {
            self.window.set_fullscreen(FullscreenType::Off)?;
        }
        let bounds = self.video.display_bounds(index)?;
        self.center_on(bounds);
        if mode != WindowMode::Windowed {
            self.set_window_mode(mode)?;
        }
        self.refresh_size();
        Ok(())
    }

    /// Every resolution the window's current screen supports, biggest first -
    /// for a settings menu's resolution list.
    pub fn available_resolutions(&self) -> Vec<(u32, u32)> {
        let display = self.display();
        let count = self.video.num_display_modes(display).unwrap_or(0);
        let mut resolutions: Vec<(u32, u32)> = (0..count)
            .filter_map(|index| self.video.display_mode(display, index).ok())
            .map(|mode| (mode.w as u32, mode.h as u32))
            .collect();
        resolutions.sort_by(|a, b| b.cmp(a));
        resolutions.dedup();
        resolutions
    }
}
