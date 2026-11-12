use nalgebra::Vector3;
use pankarta_software::prelude::*;

pub mod player;
pub mod scene;
pub mod sign;
pub mod zone;

#[tokio::main]
async fn main() {
    let mut app = match App::new().await {
        Ok(app) => app,
        Err(err) => {
            eprintln!("Couldn't create the app: {err}");
            return;
        }
    };
    app.window.set_title("Walking");

    app.physics.set_gravity(Vector3::new(0.0, -20.0, 0.0));

    if let Err(error) = app
        .window
        .set_window_mode(pankarta_software::window::WindowMode::Fullscreen)
    {
        eprintln!("{error}");
    }

    app.renderer.set_vsync(false);

    app.renderer.set_ambient_light([0.8, 0.8, 0.8]);

    // Shows the sensors' areas (see zone.rs) - green while the player's in one.
    app.renderer.debug.sensors = true;

    // The game's default controls, shipped with it - then where the player's
    // own rebinds are kept, in their user folder (writable wherever the game
    // is installed).
    if let Err(error) = app.input.load_bindings("examples/3D/walking/input.ron") {
        eprintln!("{error}");
    }
    match user_dir("Pankarta", "Walking") {
        Ok(dir) => app
            .input
            .set_user_bindings_path(Some(dir.join("bindings.ron"))),
        Err(error) => eprintln!("No user folder for the rebinds: {error}"),
    }

    // The sign glass's shader, from its file - saving it while this runs
    // reloads it.
    app.renderer.shaders.add_file(
        "sign_glass",
        concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/examples/3D/walking/sign_glass.wgsl"
        ),
    );

    app.scene_manager
        .create_scene("main", scene::WalkingScene::new);
    app.scene_manager.open_scene("main");
    app.run();
}
