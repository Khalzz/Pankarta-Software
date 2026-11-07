use wgpu::{BindGroupLayout, Device, Queue, SurfaceConfiguration};

use crate::asset_manager::load_asset_binary;
use crate::rendering::enviroment::skybox_renderer::SkyboxRender;
use crate::rendering::models::textures::Texture;
use crate::ui::color::UiColor;

/// The clear color of `Environment::default()`.
pub const DEFAULT_CLEAR_COLOR: UiColor = UiColor::Rgb(89, 124, 149);

/// `color` as the GPU clears to it: made linear first when the screen
/// stores sRGB (`srgb_target`), so it comes out as the color it names - the
/// same way UI and 2D colors are.
pub(crate) fn clear_color(color: UiColor, srgb_target: bool) -> wgpu::Color {
    let [r, g, b, a] = color.to_rgba_f32();
    let channel = |value: f32| f64::from(if srgb_target { crate::rendering::sdf_text::srgb_to_linear(value) } else { value });
    wgpu::Color { r: channel(r), g: channel(g), b: channel(b), a: f64::from(a) }
}

/// Each face of a skybox cubemap, in +X, -X, +Y, -Y, +Z, -Z order - paths
/// relative to the assets folder (see `AssetFolder::set_root`).
#[derive(Debug, Clone, PartialEq)]
pub struct SkyboxFaces {
    pub px: String,
    pub nx: String,
    pub py: String,
    pub ny: String,
    pub pz: String,
    pub nz: String,
}

/// What's drawn behind everything - `app.renderer.set_environment`. One for
/// every scene, until it's set again.
#[derive(Debug, Clone, PartialEq)]
pub enum Environment {
    /// A plain color - nothing drawn, the screen just cleared to it.
    Color(UiColor),
    Skybox(SkyboxFaces),
    /// Procedural clear-day "sea sky" - a computed atmospheric gradient with a
    /// sun disc and glow, no cubemap needed. See `sky.wgsl` /
    /// `SkyboxRender::new_procedural`. `sun`: whether the sun's disc and glow
    /// are drawn in it - pointing the way the lighting comes from - or it's
    /// just the sky.
    ProceduralSky { sun: bool },
}

impl Default for Environment {
    fn default() -> Self {
        Environment::Color(DEFAULT_CLEAR_COLOR)
    }
}

/// An `Environment` built and ready to draw - its skybox's textures loaded
/// (see `prepare_environment`).
pub struct PreparedEnvironment {
    pub skybox: Option<SkyboxRender>,
    pub clear_color: wgpu::Color,
}

/// Loads a skybox cubemap from 6 face image files inside the assets folder,
/// in +X, -X, +Y, -Y, +Z, -Z order.
pub fn load_texture_cube(file_names: [&str; 6], device: &wgpu::Device, queue: &wgpu::Queue) -> anyhow::Result<Texture> {
    let mut face_bytes: Vec<Vec<u8>> = Vec::with_capacity(6);
    for file_name in file_names {
        face_bytes.push(load_asset_binary(file_name)?);
    }
    let face_slices: [&[u8]; 6] = std::array::from_fn(|i| face_bytes[i].as_slice());
    Texture::from_cube_bytes(face_slices, device, queue, "skybox_cube_texture")
}

/// Builds `environment` so it can be drawn (a skybox's textures loaded) -
/// no `&mut App`, so it can run on a scene's loading thread (Device/Queue are
/// cheap, Arc-backed clones). `camera_bind_group_layout` is the one piece
/// `SkyboxRender::new` needs out of the camera's resources.
pub fn prepare_environment(device: &Device, queue: &Queue, camera_bind_group_layout: &BindGroupLayout, config: &SurfaceConfiguration, environment: Environment) -> PreparedEnvironment {
    let srgb = config.format.is_srgb();
    let default_clear = clear_color(DEFAULT_CLEAR_COLOR, srgb);
    match environment {
        Environment::Color(color) => PreparedEnvironment { skybox: None, clear_color: clear_color(color, srgb) },
        Environment::ProceduralSky { sun } => PreparedEnvironment {
            skybox: Some(SkyboxRender::new_procedural(device, config, camera_bind_group_layout, sun)),
            clear_color: default_clear,
        },
        Environment::Skybox(faces) => {
            let texture = load_texture_cube(
                [&faces.px, &faces.nx, &faces.py, &faces.ny, &faces.pz, &faces.nz],
                device,
                queue,
            );

            match texture {
                Ok(texture) => PreparedEnvironment {
                    skybox: Some(SkyboxRender::new(device, config, camera_bind_group_layout, texture)),
                    clear_color: default_clear,
                },
                Err(err) => {
                    eprintln!("Skybox faces couldn't be loaded, falling back to clear color: {err}");
                    PreparedEnvironment { skybox: None, clear_color: default_clear }
                }
            }
        }
    }
}
