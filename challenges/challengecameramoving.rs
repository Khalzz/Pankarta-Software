use std::env;

use cgmath::SquareMatrix;
use image::GenericImageView;
use sdl2::render::TextureCreator;
use sdl2::video::{DisplayMode, WindowContext};
use sdl2::{video::Window, Sdl, render::Canvas};
use wgpu::util::DeviceExt;
use wgpu::{BindGroupLayoutDescriptor, Device, DeviceDescriptor, Extent3d, Features, Instance, InstanceDescriptor, Limits, Queue, Surface, SurfaceConfiguration, TextureUsages};
use crate::gameplay::play;
use crate::rendering::textures::Texture;

/* 
const VERTICES: &[Vertex] = &[
    Vertex { position: [-0.5, -0.5, 0.0], color: [0.0, 0.0, 1.0] },
    Vertex { position: [0.5, -0.5, 0.0], color: [0.0, 1.0, 0.0] },
    Vertex { position: [0.5, 0.5, 0.0], color: [1.0, 0.0, 0.0] },

    Vertex { position: [-0.5, 0.5, 0.0], color: [1.0, 0.0, 0.0] },
];

const INDICES: &[u16] = &[
    0,1,2,
    3,0,2
];
*/
/*
// pentagon downwards
const VERTICES: &[Vertex] = &[
    Vertex { position: [-0.2, -0.7, 0.0], tex_coords: [0.3, 0.0] },
    Vertex { position: [0.2, -0.7, 0.0], tex_coords: [0.7, 0.0] },
    Vertex { position: [0.0, 0.7, 0.0], tex_coords: [0.5, 1.0] },

    Vertex { position: [-0.5, 0.0, 0.0], tex_coords: [0.0, 0.5] },

    Vertex { position: [0.5, 0.0, 0.0], tex_coords: [1.0, 0.5] },
];

const INDICES: &[u16] = &[
    0,1,2,
    2,3,0,
    1,4,2,
]; 
*/


const VERTICES: &[Vertex] = &[
    Vertex { position: [-0.5, -0.5, 0.5], tex_coords: [0.0, 1.0] },
    Vertex { position: [0.5, -0.5, 0.5], tex_coords: [1.0, 1.0] },
    Vertex { position: [0.0, 0.5, 0.0], tex_coords: [0.5, 0.0] },

    Vertex { position: [0.5, -0.5, -0.5], tex_coords: [0.0, 1.0] },

    Vertex { position: [-0.5, -0.5, -0.5], tex_coords: [1.0, 1.0] },
];

const INDICES: &[u16] = &[
    0,1,2,
    1,3,2,
    3,4,2,
    2,4,0,
    0,4,3,
    0,3,1
];


/* 
const VERTICES: &[Vertex] = &[
    Vertex { position: [-0.5, -0.5, 0.5], tex_coords: [0.0, 1.0] },
    Vertex { position: [0.5, -0.5, 0.5], tex_coords: [1.0, 1.0] },
    Vertex { position: [0.5, 0.5, 0.5], tex_coords: [1.0, 0.0] },

    Vertex { position: [-0.5, 0.5, 0.5], tex_coords: [0.0, 0.0] },

    Vertex { position: [0.5, -0.5, -0.5], tex_coords: [0.0, 1.0] },
    Vertex { position: [0.5, 0.5, -0.5], tex_coords: [0.0, 0.0] },

    Vertex { position: [-0.5, -0.5, -0.5], tex_coords: [0.0, 0.0] },
    Vertex { position: [-0.5, 0.5, -0.5], tex_coords: [0.0, 0.0] },


];

const INDICES: &[u16] = &[
    0,1,2,
    1,4,2,
    1,4,2,
    4,5,2,
    2,3,0,
    3,6,0,
    3,7,6,
    5,3,2,
    3,5,7,
    7,5,4,
    4,6,7
];
*/

/* 
const VERTICES: &[Vertex] = &[
    Vertex { position: [-0.5, -0.5, 0.0], tex_coords: [0.0, 1.0] },
    Vertex { position: [0.5, -0.5, 0.0], tex_coords: [1.0, 1.0] },
    Vertex { position: [0.5, 0.5, 0.0], tex_coords: [1.0, 0.0] },

    Vertex { position: [-0.5, 0.5, 0.0], tex_coords: [0.0, 0.0] },
];

const INDICES: &[u16] = &[
    0,1,2,
    2,3,0,
];
*/

pub enum GameState {
    Playing,
}

pub struct AppState {
    pub is_running: bool,
    pub state: GameState,
}

// the bytemuck elements let us have better control on our lists of T elements and without this we can't create our vertex buffer
#[repr(C)]
#[derive(Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
struct Vertex {
    position: [f32; 3],
    tex_coords: [f32; 2],
}

impl Vertex {
    fn desc() -> wgpu::VertexBufferLayout<'static> {
        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<Vertex>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &[
                wgpu::VertexAttribute { // position of the vertex
                    offset: 0,
                    shader_location: 0,
                    format: wgpu::VertexFormat::Float32x3,
                },
                wgpu::VertexAttribute { // tex_coords
                    offset: std::mem::size_of::<[f32; 3]>() as wgpu::BufferAddress,
                    shader_location: 1,
                    format: wgpu::VertexFormat::Float32x2,
                }
            ]
        }
    }
}

#[rustfmt::skip]
pub const OPENGL_TO_WGPU_MATRIX: cgmath::Matrix4<f32> = cgmath::Matrix4::new(
    1.0, 0.0, 0.0, 0.0,
    0.0, 1.0, 0.0, 0.0,
    0.0, 0.0, 0.5, 0.5,
    0.0, 0.0, 0.0, 1.0,
);

// we create the values that make our camera position and view angle
#[derive(Copy, Clone, Debug)]
pub struct Camera {
    pub eye: cgmath::Point3<f32>, // position of the camera
    pub target: cgmath::Point3<f32>, // where is looking
    pub up: cgmath::Vector3<f32>, // y axis
    aspect: f32,
    fovy: f32,
    znear: f32,
    zfar: f32,
}

impl Camera {
    fn build_view_projection_matrix(&self) -> cgmath::Matrix4<f32> {
        let view = cgmath::Matrix4::look_at_rh(self.eye, self.target, self.up);
        let proj = cgmath::perspective(cgmath::Deg(self.fovy), self.aspect, self.znear, self.zfar); 
        return OPENGL_TO_WGPU_MATRIX * proj * view;
    }
}

// the cameraUniform will get us the positional matrix of the camera
#[repr(C)]
#[derive(Debug, Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
pub struct CameraUniform {
    view_proj: [[f32; 4]; 4],
}

impl CameraUniform {
    fn new() -> Self {
        Self {
            view_proj: cgmath::Matrix4::identity().into(),
        }
    }

    fn update_view_proj(&mut self, camera: &Camera) {
        self.view_proj = camera.build_view_projection_matrix().into();
    }
}

// For the challenge we make a camera staging, this will define a rotation of the object we are looking, while keeping the position on the x and z axis
pub struct CameraStaging {
    pub camera: Camera,
    model_rotation: cgmath::Deg<f32>
}

impl CameraStaging {
    fn new(camera: Camera) -> Self {
        Self {
            camera,
            model_rotation: cgmath::Deg(0.0)
        }
    }

    fn update_camera(&self, camera_uniform: &mut CameraUniform) {
        camera_uniform.view_proj = (OPENGL_TO_WGPU_MATRIX * self.camera.build_view_projection_matrix() * cgmath::Matrix4::from_angle_z(self.model_rotation)).into();
    }
}
// For the challenge we make a camera staging, this will define a rotation of the object we are looking, while keeping the position on the x and z axis

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
    pub vertex_buffer: wgpu::Buffer,
    pub index_buffer: wgpu::Buffer,
    pub diffuse_bind_group: wgpu::BindGroup,
    pub diffuse_texture: Texture,
    pub camera: Camera,
    pub camera_uniform: CameraUniform,
    pub camera_buffer: wgpu::Buffer,
    pub camera_bind_group: wgpu::BindGroup,
    pub camera_staging: CameraStaging
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

        // Textures
        let diffuse_bytes = include_bytes!("../assets/textures/sad_hamster.png"); // search the image
        let diffuse_texture = Texture::from_bytes(diffuse_bytes, &device, &queue, "sad-hamster.png").unwrap();

        // The bindgroup describes resources and how the shader will access to them
        let texture_bind_group_layout = device.create_bind_group_layout(&BindGroupLayoutDescriptor {
            label: Some("texture_bind_group_layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        multisampled: false,
                        view_dimension: wgpu::TextureViewDimension::D2,
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    // This should match the filterable field of the
                    // corresponding Texture entry above.
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });

        // we have to create a bind group for each texture since the fact that the layout and the group are separated is because we can swap the bind group on runtime
        let diffuse_bind_group = device.create_bind_group(
            &wgpu::BindGroupDescriptor {
                label: Some("diffuse_bind_group"),
                layout: &texture_bind_group_layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(&diffuse_texture.view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::Sampler(&diffuse_texture.sampler),
                    }
                ],
            }
        );
        // Textures

        // Camera
        // we set up the camera
        let camera = Camera {
            eye: (0.0, 1.0, 2.0).into(), // the position of the camera
            target: (0.0, 0.0, 0.0).into(), // we are looking at (0,0,0)
            up: cgmath::Vector3::unit_y(),
            aspect: config.width as f32 / config.height as f32,
            fovy: 45.0,
            znear: 0.1,
            zfar: 100.0,
        };

        // we create the 4x4 matrix of the camera
        let mut camera_uniform = CameraUniform::new();

        // we create the staging
        let camera_staging = CameraStaging::new(camera);

        // we create a buffer and a bind group
        let camera_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Camera Buffer"),
            contents: bytemuck::cast_slice(&[camera_uniform]),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });

        let camera_bind_group_layout = device.create_bind_group_layout(&BindGroupLayoutDescriptor {
            label: Some("camera_bind_group_layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform, // its a uniform buffer, duh
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
            ],
        });

        let camera_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("camera_bind_group"),
                layout: &camera_bind_group_layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: camera_buffer.as_entire_binding(),
                    },
                ],
            }
        );
        // Camera

        // SHADERING PROCESS 
        // we get access to our shader file
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shaders/camera_shader.wgsl").into()),
        });

        let render_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("Render Pipeline Layout"),
            bind_group_layouts: &[
                &texture_bind_group_layout,
                &camera_bind_group_layout
            ],
            push_constant_ranges: &[],
        });

        // here we define elements like, the main entry for our vertexes and our fragments
        let render_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("Render Pipeline"),
            layout: Some(&render_pipeline_layout),
            vertex: wgpu::VertexState { // vertexes are the lines that define the shape of a element
                module: &shader,
                entry_point: "vs_main", // we set our entry point of the vertex shader
                buffers: &[Vertex::desc()], // we set the vertexes to draw (we are doing this in the shader but we can pass data here and will be accesable from the vertex shader)
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

        let vertex_buffer = device.create_buffer_init(
            &wgpu::util::BufferInitDescriptor {
                label: Some("Vertex Buffer"),
                contents: bytemuck::cast_slice(VERTICES),
                usage: wgpu::BufferUsages::VERTEX,
            }
        );

        let index_buffer = device.create_buffer_init(
            &wgpu::util::BufferInitDescriptor {
                label: Some("Index Buffer"),
                contents: bytemuck::cast_slice(INDICES),
                usage: wgpu::BufferUsages::INDEX,
            }
        );

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
            vertex_buffer,
            index_buffer,
            diffuse_bind_group,
            diffuse_texture,
            camera,
            camera_uniform,
            camera_buffer,
            camera_bind_group,
            camera_staging
        }
    }

    pub fn resize(&mut self) {
        self.config.width = self.current_display.w as u32;
        self.config.height = self.current_display.h as u32;
        self.surface.configure(&self.device, &self.config);

        // we update the aspect ratio on resize
        self.camera_staging.camera.aspect = self.config.width as f32 / self.config.height as f32;
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
            render_pass.set_pipeline(&self.render_pipeline);
            render_pass.set_bind_group(0, &self.diffuse_bind_group, &[]); // texture deffinition
            render_pass.set_bind_group(1, &self.camera_bind_group, &[]); // Camera definition
            render_pass.set_vertex_buffer(0, self.vertex_buffer.slice(..));
            render_pass.set_index_buffer(self.index_buffer.slice(..), wgpu::IndexFormat::Uint16); // 1.
            render_pass.draw_indexed(0..INDICES.len() as u32, 0, 0..1); // we tell to the renderer "draw something with 3 vertices and 1 instance" this is the @builtin(vertex_index) value
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
        let mut play = play::GameLogic::new(&mut self, 5.0);

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
                    // we update the model rotation (so it rotates without need of input) and then update the camera position
                    self.camera_staging.model_rotation += cgmath::Deg(2.0);
                    self.camera_staging.update_camera(&mut self.camera_uniform);
                    self.queue.write_buffer(&self.camera_buffer, 0, bytemuck::cast_slice(&[self.camera_uniform]));
                }
            }
        }
    }
}