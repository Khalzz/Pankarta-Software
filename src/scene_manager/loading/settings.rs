//! The engine-wide settings a scene's factory may change. A scene can be
//! built while another one runs (a preload, or while the old scene stays on
//! screen), so what its factory sets is taken back right after the build and
//! applied only when the scene actually opens.

use nalgebra::Vector3;

use crate::app::App;
use crate::rendering::enviroment::environment::Environment;
use crate::rendering::render_pipeline::shadow_renderer::Shadows;

#[derive(Clone)]
pub(crate) struct GlobalSettings {
    environment: Environment,
    ambient_light: [f32; 3],
    shadows: Shadows,
    gravity: Vector3<f32>,
    timestep: f32,
    wind: Vector3<f32>,
    sea_level: Option<f32>,
}

impl GlobalSettings {
    pub(crate) fn capture(app: &App) -> Self {
        Self {
            environment: app.renderer.environment().clone(),
            ambient_light: app.renderer.ambient_light(),
            shadows: app.renderer.shadows(),
            gravity: app.physics.gravity(),
            timestep: app.physics.timestep(),
            wind: app.particles.wind(),
            sea_level: app.particles.sea_level(),
        }
    }

    /// Puts every setting back to what it was when this was captured.
    pub(crate) fn restore(&self, app: &mut App) {
        SettingsChange::between(&GlobalSettings::capture(app), self).apply(app);
    }
}

/// Only the settings a factory actually changed - applying it leaves every
/// other setting as the running game has it by then.
#[derive(Default)]
pub(crate) struct SettingsChange {
    environment: Option<Environment>,
    ambient_light: Option<[f32; 3]>,
    shadows: Option<Shadows>,
    gravity: Option<Vector3<f32>>,
    timestep: Option<f32>,
    wind: Option<Vector3<f32>>,
    sea_level: Option<Option<f32>>,
}

fn changed<T: PartialEq + Clone>(before: &T, after: &T) -> Option<T> {
    (before != after).then(|| after.clone())
}

impl SettingsChange {
    pub(crate) fn between(before: &GlobalSettings, after: &GlobalSettings) -> Self {
        Self {
            environment: changed(&before.environment, &after.environment),
            ambient_light: changed(&before.ambient_light, &after.ambient_light),
            shadows: changed(&before.shadows, &after.shadows),
            gravity: changed(&before.gravity, &after.gravity),
            timestep: changed(&before.timestep, &after.timestep),
            wind: changed(&before.wind, &after.wind),
            sea_level: changed(&before.sea_level, &after.sea_level),
        }
    }

    pub(crate) fn environment(&self) -> Option<&Environment> {
        self.environment.as_ref()
    }

    pub(crate) fn apply(self, app: &mut App) {
        if let Some(environment) = self.environment {
            app.renderer.set_environment(environment);
        }
        if let Some(ambient_light) = self.ambient_light {
            app.renderer.set_ambient_light(ambient_light);
        }
        if let Some(shadows) = self.shadows {
            app.renderer.set_shadows(shadows);
        }
        if let Some(gravity) = self.gravity {
            app.physics.set_gravity(gravity);
        }
        if let Some(timestep) = self.timestep {
            app.physics.set_timestep(timestep);
        }
        if let Some(wind) = self.wind {
            app.particles.set_wind(wind);
        }
        if let Some(sea_level) = self.sea_level {
            app.particles.set_sea_level(sea_level);
        }
    }
}
