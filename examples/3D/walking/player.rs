use nalgebra::{UnitQuaternion, Vector2, Vector3};
use pankarta_software::physics::PhysicsWorld;
use pankarta_software::prelude::*;
use pankarta_software::rapier3d::prelude::SharedShape;

const RADIANS_PER_COUNT: f32 = 0.0004;

const PLAYER_RADIUS: f32 = 0.35;
const PLAYER_HALF_HEIGHT: f32 = 0.9;
const EYE_HEIGHT: f32 = 0.7;
/// Half as tall as it stands while crouching (1.1 m in all), its feet kept
/// where they are - its top comes down.
const CROUCH_HALF_HEIGHT: f32 = 0.55;
/// The eye, over the body's middle, while crouching - as far under its
/// (lower) top as `EYE_HEIGHT` is standing.
const CROUCH_EYE_HEIGHT: f32 = EYE_HEIGHT - 2.0 * (PLAYER_HALF_HEIGHT - CROUCH_HALF_HEIGHT);

const PLAYER_MASS: f32 = 80.0;

/// Free camera (F toggles): how fast it flies (m/s) - slow, to get right up
/// to things - and how many times faster with "run" held.
const FREE_SPEED: f32 = 2.0;
const FREE_RUN_FACTOR: f32 = 8.0;

struct Camera {
    pitch: f32,
    yaw: f32,
    /// The field of view the scroll wheel asks for (degrees) - the lens eases
    /// toward it. None until the first frame, which takes the lens's own.
    target_fov: Option<f32>,
    /// Seconds the zoom takes to get most of the way to `target_fov`.
    zoom_smoothing: f32,
}

struct Movement {
    spawn: Vector3<f32>,
    walk_speed: f32,
    run_speed: f32,
    crouch_speed: f32,
    jump_speed: f32,
    fall_gravity: f32,
    ground_deceleration: f32,
    ground_acceleration: f32,
    air_acceleration: f32,
}

pub struct Player {
    camera: Camera,
    movement: Movement,
    crouch: Crouch,
    /// Where the free camera is, while it's on (F) - the body stays where it
    /// was left, and the controls fly the camera instead.
    free: Option<Vector3<f32>>,
}

/// Crouching (held "crouch"): a shorter capsule, a lower eye, slower.
struct Crouch {
    crouched: bool,
    /// The eye's height over the body's middle now - easing between
    /// `EYE_HEIGHT` and `CROUCH_EYE_HEIGHT`.
    eye_height: f32,
    /// Seconds the eye takes to get most of the way down or up.
    smoothing: f32,
}

impl Player {
    pub fn new(spawn: Vector3<f32>) -> Player {
        Player {
            camera: Camera {
                pitch: 0.0,
                // Turned around from -Z, to face the course (laid out along +Z).
                yaw: std::f32::consts::PI,
                target_fov: None,
                zoom_smoothing: 0.08,
            },
            movement: Movement {
                spawn,
                walk_speed: 10.0,
                run_speed: 15.0,
                crouch_speed: 4.0,
                jump_speed: 7.0,
                fall_gravity: 1.5,
                ground_deceleration: 8.0,
                ground_acceleration: 8.0,
                air_acceleration: 3.0,
            },
            crouch: Crouch { crouched: false, eye_height: EYE_HEIGHT, smoothing: 0.08 },
            free: None,
        }
    }
}

impl Behavior for Player {
    fn on_spawn(&mut self, node: &mut Node, scene: &mut SceneAccess, _app: &mut App) {
        node.set_property(Transform3D {
            position: self.movement.spawn,
            ..Default::default()
        });
        // What sensors see it as (see sensor_gallery.rs).
        node.set_property(Layers::new(["player"]));
        node.set_property(RigidBodyBuilder::dynamic().lock_rotations().ccd_enabled(true));
        node.set_property(Colliders::new()
            // No grip at all, whatever it touches (the lower friction of
            // two wins): it slides along walls instead of sticking to them.
            .add(ColliderBuilder::capsule_y(PLAYER_HALF_HEIGHT - PLAYER_RADIUS, PLAYER_RADIUS)
                .mass(PLAYER_MASS)
                .friction(0.0)
                .friction_combine_rule(CoefficientCombineRule::Min)));

        let camera = Node::new("camera")
            .with_property(Transform3D::default())
            .with_property(CameraLens::new(75.0));

        scene.spawn(camera);
    }

    fn update(&mut self, node: &mut Node, scene: &mut SceneAccess, app: &mut App) {
        // For enabling the cursor (not gameplay related)
        if app.input.is_just_pressed(InputSource::key(Key::Escape)) {
            app.window.set_cursor_locked(false);
        } else if app.input.mouse_left_button_just_pressed() {
            app.window.set_cursor_locked(true);
        }

        if app.window.is_cursor_locked() {
            let look = app.input.mouse_look() * RADIANS_PER_COUNT;
            self.camera.yaw -= look.x;
            // Looking along -Z: looking down is a negative turn about X.
            self.camera.pitch = (self.camera.pitch - look.y).clamp(-1.5, 1.5);
        }

        let heading = UnitQuaternion::from_axis_angle(&Vector3::y_axis(), self.camera.yaw);

        let Some(position) = node
            .get_property::<Transform3D>()
            .map(|transform| transform.position)
        else {
            return;
        };
        let Some(camera) = scene.get_node_mut("camera") else {
            return;
        };
        let rotation = heading * UnitQuaternion::from_axis_angle(&Vector3::x_axis(), self.camera.pitch);

        // F: free camera on (from where the eye is now) or off (back to the body).
        if app.input.is_just_pressed(InputSource::key(Key::F)) {
            self.free = match self.free {
                Some(_) => None,
                None => camera.get_property::<Transform3D>().map(|transform| transform.position),
            };
        }
        if let Some(free) = &mut self.free {
            // Flies where it looks; jump up, crouch down (world up/down);
            // run faster. Unscaled time: it flies the same in slow motion.
            let input = app.input.get_vector("left", "right", "forward", "back");
            let vertical = app.input.get_axis("crouch", "jump");
            let direction = rotation * Vector3::new(input.x, 0.0, input.y) + Vector3::y() * vertical;
            if direction.norm_squared() > 1e-6 {
                let speed = if app.input.is_pressed("run") { FREE_SPEED * FREE_RUN_FACTOR } else { FREE_SPEED };
                *free += direction.normalize() * speed * app.time.unscaled_delta_time;
            }
        }
        // The eye eases down when it crouches and back up when it stands -
        // the same at any frame rate.
        let eye_target = if self.crouch.crouched { CROUCH_EYE_HEIGHT } else { EYE_HEIGHT };
        let ease = 1.0 - (-app.time.delta_time / self.crouch.smoothing.max(0.0001)).exp();
        self.crouch.eye_height += (eye_target - self.crouch.eye_height) * ease;
        if let Some(transform) = camera.get_property_mut::<Transform3D>() {
            transform.position = self.free.unwrap_or(position + Vector3::y() * self.crouch.eye_height);
            transform.rotation = rotation;
        }

        if let Some(lens) = camera.get_property_mut::<CameraLens>() {
            let target = self.camera.target_fov.get_or_insert(lens.fovy);
            *target = (*target - app.input.mouse_scroll_y() * 3.0).clamp(15.0, 110.0);
            // Eased the same at any frame rate.
            let ease = 1.0
                - (-app.time.unscaled_delta_time / self.camera.zoom_smoothing.max(0.0001)).exp();
            lens.fovy += (*target - lens.fovy) * ease;
        }
    }

    fn fixed_update(&mut self, node: &mut Node, _scene: &mut SceneAccess, app: &mut App) {
        // With the free camera on, the controls fly it - the body just stops.
        let free = self.free.is_some();
        let walk: Vector2<f32> = if free {
            Vector2::zeros()
        } else {
            app.input.get_vector("left", "right", "forward", "back").normalize().into()
        };
        let grounded = self::on_ground(&app.physics, &node.id);

        // Crouches while "crouch" is held - and stays down after it's let go
        // until there's room over its head to stand.
        let crouched = (!free && app.input.is_pressed("crouch")) || (self.crouch.crouched && !room_to_stand(&app.physics, &node.id));
        if crouched != self.crouch.crouched {
            self.crouch.crouched = crouched;
            set_crouched(&mut app.physics, &node.id, crouched);
        }

        let speed = if crouched {
            self.movement.crouch_speed
        } else if !free && app.input.is_pressed("run") {
            self.movement.run_speed
        } else {
            self.movement.walk_speed
        };

        let heading = UnitQuaternion::from_axis_angle(&Vector3::y_axis(), self.camera.yaw);
        let forward = heading * -Vector3::z();
        let right = heading * Vector3::x();

        if let Some(body) = app.physics.body_mut(&node.id) {
            body.reset_forces(false);

            let mut velocity = *body.linvel();

            // Movement logic
            let movement = (forward * -walk.y + right * walk.x) * speed;

            let target = Vector3::new(movement.x, 0.0, movement.z);
            let current = Vector3::new(velocity.x, 0.0, velocity.z);

            // Braking: no input, or input pushing against how it's moving now.
            let braking = target.norm_squared() < 1e-4 || target.dot(&current) < 0.0;
            let acceleration = match (grounded, braking) {
                (true, true) => self.movement.ground_deceleration, // e.g. 20-30: stops fast
                (true, false) => self.movement.ground_acceleration, // e.g. 10-15
                (false, _) => self.movement.air_acceleration,      // e.g. 2-4
            };

            body.add_force((target - current) * acceleration * body.mass(), true);
            // velocity.x = movement.x;
            // velocity.z = movement.z;

            // Jump logic
            if !free && app.input.is_just_pressed("jump") && grounded && !crouched {
                velocity.y = self.movement.jump_speed;
            }

            let falling = velocity.y < 0.0;
            body.set_gravity_scale(
                if falling {
                    self.movement.fall_gravity
                } else {
                    1.0
                },
                true,
            );

            body.set_linvel(velocity, true); // true wakes the body if it was asleep
        }
    }
}

const GROUND_MARGIN: f32 = 0.08;

/// Makes node `id`'s capsule (its first collider) crouching-short or
/// standing-tall - Rapier's own collider, changed while it runs. Moved down
/// as it shrinks, so its feet stay on the ground and its top comes down.
fn set_crouched(physics: &mut PhysicsWorld, id: &str, crouched: bool) {
    let half_height = if crouched { CROUCH_HALF_HEIGHT } else { PLAYER_HALF_HEIGHT };
    if let Some(collider) = physics.collider_mut(id, 0) {
        collider.set_shape(SharedShape::capsule_y(half_height - PLAYER_RADIUS, PLAYER_RADIUS));
        collider.set_translation_wrt_parent(Vector3::y() * (half_height - PLAYER_HALF_HEIGHT));
    }
}

/// Whether node `id` can stand up: nothing over its head, up to where its
/// standing top would be.
fn room_to_stand(physics: &PhysicsWorld, id: &str) -> bool {
    let Some(body) = physics.body(id) else {
        return true;
    };
    let center = *body.translation();
    let edge = PLAYER_RADIUS * 0.7;
    [Vector3::zeros(), Vector3::x() * edge, -Vector3::x() * edge, Vector3::z() * edge, -Vector3::z() * edge]
        .iter()
        .all(|offset| physics.cast_ray(center + offset, Vector3::y(), PLAYER_HALF_HEIGHT + GROUND_MARGIN, Some(id)).is_none())
}

pub fn on_ground(physics: &PhysicsWorld, id: &str) -> bool {
    let Some(body) = physics.body(id) else {
        return false;
    };
    let center = *body.translation();
    let reach = PLAYER_HALF_HEIGHT + GROUND_MARGIN;
    let edge = PLAYER_RADIUS * 0.7;
    [
        Vector3::zeros(),
        Vector3::x() * edge,
        -Vector3::x() * edge,
        Vector3::z() * edge,
        -Vector3::z() * edge,
    ]
    .iter()
    .any(|offset| {
        physics
            .cast_ray(center + offset, -Vector3::y(), reach, Some(id))
            .is_some()
    })
}
