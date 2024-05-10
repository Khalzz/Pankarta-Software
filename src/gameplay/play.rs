use std::time::{Duration, Instant};

use cgmath::InnerSpace;
use sdl2::{event::Event, keyboard::Keycode, pixels::Color, ttf::Font};
use wgpu::BindGroupLayoutDescriptor;
use crate::{app::{App, AppState}, game_object::GameObject, input::button_module::{Button, TextAlign}, rendering::textures::Texture};

pub struct Controller {
    forward: bool,
    backwards: bool,
    left: bool,
    right: bool
}

pub struct GameLogic { // here we define the data we use on our script
    fps: u32,
    fps_text: Button,
    last_frame: Instant,
    pub start_time: Instant,
    frame_count: u32,
    frame_timer: Duration,
    controller: Controller,
    speed: f32
} 

impl GameLogic {
    // this is called once
    pub fn new(_app: &mut App, speed: f32) -> Self {
        // UI ELEMENTS AND LIST
        let framerate = Button::new(GameObject {active: true, x:10 as f32, y: 10.0, width: 0.0, height: 0.0},Some(String::from("Framerate")),Color::RGBA(100, 100, 100, 0),Color::WHITE,Color::RGB(0, 200, 0),Color::RGB(0, 0, 0),None, TextAlign::Left);

        Self {
            fps: 0,
            fps_text: framerate,
            last_frame: Instant::now(),
            start_time: Instant::now(),
            frame_count: 0,
            frame_timer: Duration::new(0, 0),
            controller: Controller { forward: false, backwards: false, left: false, right: false },
            speed
        }
    }

    // this is called every frame
    pub fn update(&mut self, _font: &Font, mut app_state: &mut AppState, mut event_pump: &mut sdl2::EventPump, app: &mut App) {
        let delta_time = self.delta_time();
        self.display_framerate(delta_time);

        let forward = app.camera.camera.target - app.camera.camera.eye;
        let forward_norm = forward.normalize();
        let forward_mag = forward.magnitude();

        // Prevents glitching when the camera gets too close to the
        // center of the scene.
        if self.controller.forward && forward_mag > self.speed {
            app.camera.camera.eye += forward_norm * self.speed * delta_time.as_secs_f32();
        }
        if self.controller.backwards {
            app.camera.camera.eye -= forward_norm * self.speed * delta_time.as_secs_f32();
        }

        let right = forward_norm.cross(app.camera.camera.up);

        // Redo radius calc in case the forward/backward is pressed.
        let forward = app.camera.camera.target - app.camera.camera.eye;
        let forward_mag = forward.magnitude();

        if self.controller.right {
            // Rescale the distance between the target and the eye so 
            // that it doesn't change. The eye, therefore, still 
            // lies on the circle made by the target and eye.
            app.camera.camera.eye = app.camera.camera.target - (forward + right * self.speed * delta_time.as_secs_f32()).normalize() * forward_mag;
        }
        if self.controller.left {
            app.camera.camera.eye = app.camera.camera.target - (forward - right * self.speed * delta_time.as_secs_f32()).normalize() * forward_mag;
        }

        Self::event_handler(self, &mut app_state, &mut event_pump, app);
    }

    fn event_handler(&mut self, app_state: &mut AppState, event_pump: &mut sdl2::EventPump, app: &mut App) {
        for event in event_pump.poll_iter() {
            match event {
                Event::KeyDown { keycode: Some(Keycode::Space), .. } => {
                    
                }
                Event::KeyDown { keycode: Some(Keycode::W), .. } => {
                    self.controller.forward = true
                }
                Event::KeyUp { keycode: Some(Keycode::W), .. } => {
                    self.controller.forward = false
                }
                Event::KeyDown { keycode: Some(Keycode::A), .. } => {
                    self.controller.left = true
                }
                Event::KeyUp { keycode: Some(Keycode::A), .. } => {
                    self.controller.left = false
                }
                Event::KeyDown { keycode: Some(Keycode::S), .. } => {
                    self.controller.backwards = true
                }
                Event::KeyUp { keycode: Some(Keycode::S), .. } => {
                    self.controller.backwards = false
                }
                Event::KeyDown { keycode: Some(Keycode::D), .. } => {
                    self.controller.right = true
                }
                Event::KeyUp { keycode: Some(Keycode::D), .. } => {
                    self.controller.right = false
                }
                Event::KeyDown { keycode: Some(Keycode::Escape), .. }  => {
                    app_state.is_running = false;
                }, Event::Quit { .. } => {
                    app_state.is_running = false;
                } 
                _ => {}
            }
        }
    }

    fn delta_time(&mut self) -> Duration {
        let current_time = Instant::now();
        let delta_time = current_time.duration_since(self.last_frame); // this is our Time.deltatime
        self.last_frame = current_time;
        return delta_time
    }

    fn display_framerate(&mut self, delta_time: Duration) {
        self.frame_count += 1;
        self.frame_timer += delta_time;

        // Calculate FPS every second
        if self.frame_timer >= Duration::from_secs(1) {
            self.fps = self.frame_count;
            self.frame_count = 0;
            self.frame_timer -= Duration::from_secs(1); // Remove one second from the timer
        }

        // Render FPS text
        let fps_text = format!("FPS: {}", self.fps);
        self.fps_text.text = Some(fps_text);
    }
}