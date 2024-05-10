use app::App;

mod app;
mod game_object;
mod resources;

mod ui {
    pub mod text;
}

mod input {
    pub mod button_module;
}

mod gameplay {
    pub mod play;
}

mod rendering {
    pub mod textures;
    pub mod camera;
    pub mod model;
}


// this tokio trait means that main WILL AND CAN be asyncronous (without tokio this is not achievable)
#[tokio::main]
async fn main() -> Result<(), String> {
    let app = App::new("WGPU with SDL2", Some(1280), Some(720));
    app.await.update();
    Ok(())
}