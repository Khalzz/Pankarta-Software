use std::env;

use sdl2::event::Event;
use sdl2::keyboard::Keycode;
use sdl2::render::TextureCreator;
use sdl2::video::{DisplayMode, WindowContext};
use sdl2::{video::Window, Sdl, render::Canvas};
use wgpu::{Device, DeviceDescriptor, Features, Instance, InstanceDescriptor, Limits, Queue, Surface, SurfaceConfiguration, TextureUsages};
use crate::gameplay::play;

pub enum GameState {
    Playing,
}

pub struct AppState {
    pub is_running: bool,
    pub state: GameState,
}

pub struct App {
    pub context: Sdl,
    pub width: u32,
    pub height: u32,
    pub canvas: Canvas<Window>,
    pub current_display: DisplayMode,
    pub texture_creator: TextureCreator<WindowContext>,
    pub surface: Surface,
    pub queue: Queue,
    pub device: Device,
    pub config: SurfaceConfiguration,
    pub render_pipeline: wgpu::RenderPipeline,
    pub challenge_render_pipeline: wgpu::RenderPipeline,
    pub change_pipeline: bool
}

impl App {
    pub async fn new(title: &str, ext_width: Option<u32>, ext_height: Option<u32>) -> App{
        // base sdl2
        let context = sdl2::init().expect("SDL2 wasn't initialized");
        let video_susbsystem = context.video().expect("The Video subsystem wasn't initialized");

        let current_display = video_susbsystem.current_display_mode(0).unwrap();
        
        let width = match ext_width {
            Some(w) => w,
            None => current_display.w as u32,
        };
        let height =  match ext_height {
            Some(h) => h,
            None => current_display.h as u32,
        };

        env::set_var("SDL_VIDEO_MINIMIZE_ON_FOCUS_LOSS", "0"); // this is highly needed so the sdl2 can alt tab without generating bugs

        let window: Window = video_susbsystem.window(title, width, height as u32).vulkan().build().expect("The window wasn't created");
        
        // WGPU INSTANCES AND SURFACE
        let instance = Instance::new(InstanceDescriptor::default());
        let surface = unsafe { instance.create_surface(&window).unwrap() }; // the surface is where we draw stuff created based on a raw window handle

        // The adapter will let us get information and data from our graphics card (for example the name of it)
        let adapter = instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            ..Default::default() // remember that this set every other parameter as their default values
        }).await.unwrap();

        println!("{}", adapter.get_info().name);

        let (device, queue) = adapter.request_device(
            &DeviceDescriptor { 
                label: None, 
                features: Features::empty(), 
                limits: Limits::default() }
            , None).await.unwrap();

        // Surface settings
        let surface_caps = surface.get_capabilities(&adapter);
        let surface_format = surface_caps.formats;

        let config = wgpu::SurfaceConfiguration {
            usage: TextureUsages::RENDER_ATTACHMENT,
            format: surface_format[0],
            width,
            height,
            present_mode: surface_caps.present_modes[0],
            alpha_mode: surface_caps.alpha_modes[0],
            view_formats: vec![],
        };

        surface.configure(&device, &config);
        // Surface settings

        // SHADERING PROCESS 
        // we get access to our shader file
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shaders/shader.wgsl").into()),
        });


        let render_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("Render Pipeline Layout"),
            bind_group_layouts: &[],
            push_constant_ranges: &[],
        });

        // here we define elements like, the main entry for our vertexes and our fragments
        let render_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("Render Pipeline"),
            layout: Some(&render_pipeline_layout),
            vertex: wgpu::VertexState { // vertexes are the lines that define the shape of a element
                module: &shader,
                entry_point: "vs_main", // we set our entry point of the vertex shader
                buffers: &[], // we set the vertexes to draw (we are doing this in the shader but we can pass data here and will be accesable from the vertex shader)
            },
            fragment: Some(wgpu::FragmentState { // fragments define the pixels inside the vertexes that will be painted and paint them
                module: &shader, 
                entry_point: "fs_main",
                targets: &[Some(wgpu::ColorTargetState {
                    format: config.format,
                    blend: Some(wgpu::BlendState::REPLACE),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState { // this sets how to interpret our vertices when converting them to triangles
                topology: wgpu::PrimitiveTopology::TriangleList, // this sets that every 3 vertices will create a triangle
                strip_index_format: None,
                // this tells wgpu how to define if the shape is lookin forwards
                front_face: wgpu::FrontFace::Ccw,
                cull_mode: Some(wgpu::Face::Back),
                // this tells wgpu how to define if the shape is lookin forwards
                polygon_mode: wgpu::PolygonMode::Fill,
                unclipped_depth: false,
                conservative: false,
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState {
                count: 1,
                mask: !0,
                alpha_to_coverage_enabled: false, // antialiasing
            },
            multiview: None,
        });

        // SHADERING PROCESS 
        // we get access to our shader file
        let challenge_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shaders/challenge1.wgsl").into()),
        });

        let challenge_render_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("Render Pipeline"),
            layout: Some(&render_pipeline_layout),
            vertex: wgpu::VertexState { // vertexes are the lines that define the shape of a element
                module: &challenge_shader,
                entry_point: "vs_main", // we set our entry point of the vertex shader
                buffers: &[], // we set the vertexes to draw (we are doing this in the shader but we can pass data here and will be accesable from the vertex shader)
            },
            fragment: Some(wgpu::FragmentState { // fragments define the pixels inside the vertexes that will be painted and paint them
                module: &challenge_shader, 
                entry_point: "fs_main",
                targets: &[Some(wgpu::ColorTargetState {
                    format: config.format,
                    blend: Some(wgpu::BlendState::REPLACE),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState { // this sets how to interpret our vertices when converting them to triangles
                topology: wgpu::PrimitiveTopology::TriangleList, // this sets that every 3 vertices will create a triangle
                strip_index_format: None,
                // this tells wgpu how to define if the shape is lookin forwards
                front_face: wgpu::FrontFace::Ccw,
                cull_mode: Some(wgpu::Face::Back),
                // this tells wgpu how to define if the shape is lookin forwards
                polygon_mode: wgpu::PolygonMode::Fill,
                unclipped_depth: false,
                conservative: false,
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState {
                count: 1,
                mask: !0,
                alpha_to_coverage_enabled: false, // antialiasing
            },
            multiview: None,
        });

        let mut canvas = window.into_canvas().accelerated().present_vsync().build().expect("the canvas wasn't builded");

        canvas.set_blend_mode(sdl2::render::BlendMode::Blend);
        let texture_creator = canvas.texture_creator();

        App {
            current_display,
            context,
            width,
            height,
            canvas,
            texture_creator,
            surface,
            queue,
            device,
            config,
            render_pipeline,
            challenge_render_pipeline,
            change_pipeline: false
        }
    }

    pub fn resize(&mut self) {
        self.config.width = self.current_display.w as u32;
        self.config.height = self.current_display.h as u32;
        self.surface.configure(&self.device, &self.config);
    }

    pub fn render(&self) -> Result<(), wgpu::SurfaceError> {
        // WGPU
        let output = self.surface.get_current_texture()?;
        let view = output.texture.create_view(&wgpu::TextureViewDescriptor::default()); // this let us to control how render code interacts with textures
        
        // most graphics frameworks expect commands to be stored in a buffer before sending them to the gpu, the encoder is that buffer
        let mut encoder = self.device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("Render Encoder"),
        });

        {
            // we make a render pass, this will have all the methods for drawing in the screen
            let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor { 
                label: Some("Render Pass"), 
                color_attachments: &[Some(wgpu::RenderPassColorAttachment { // here we will define the base colors of the screen
                    view: &view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: 0.1,
                            g: 0.2,
                            b: 0.3,
                            a: 1.0,
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                occlusion_query_set: None,
                timestamp_writes: None,
            });
            
            // we define the pipeline and then draw
            render_pass.set_pipeline(if self.change_pipeline {
                &self.challenge_render_pipeline
            } else {
                &self.render_pipeline
            });
            
            render_pass.draw(0..3, 0..1); // we tell to the renderer "draw something with 3 vertices and 1 instance" this is the @builtin(vertex_index) value
        }

        // we have the render pass inside the {} so we can do the submit to the queue, we can also drop the render pass if you prefeer
        self.queue.submit(std::iter::once(encoder.finish()));
        output.present();

        Ok(())
    }

    pub fn update(mut self) {
        // SDL2
        let mut app_state = AppState { is_running: true, state: GameState::Playing};
        let mut event_pump = self.context.event_pump().unwrap();

        // we define a font for our text
        let ttf_context = sdl2::ttf::init().unwrap(); // we create a "context"
        let use_font = "./assets/fonts/Inter-Thin.ttf";
        let mut _font = ttf_context.load_font(use_font, 20).unwrap();

        // here we define the initial state of our game states
        let mut play = play::GameLogic::new(&mut self);

        // main game loop
        while app_state.is_running { 
            match self.render() {
                Ok(_) => {},
                Err(wgpu::SurfaceError::Outdated) => { 
                    self.resize()
                }
                Err(e) => eprintln!("Error: {}", e),
            }
            
            match app_state.state {
                GameState::Playing => {
                    play.update(&_font, &mut app_state, &mut event_pump, &mut self);
                }
            }

            for event in event_pump.poll_iter() {
                match event {
                    Event::KeyDown { keycode: Some(Keycode::Space), .. }  => {
                        self.change_pipeline = !self.change_pipeline
                    },
                    Event::KeyDown { keycode: Some(Keycode::Escape), .. }  => {
                        app_state.is_running = false;
                    }, Event::Quit { .. } => {
                        app_state.is_running = false;
                    } 
                    _ => {}
                }
            }
        }
    }
}