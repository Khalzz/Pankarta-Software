use std::borrow::Borrow;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

use nalgebra::Vector2;
use sdl2::controller::GameController;
use sdl2::event::Event;
use sdl2::joystick::Joystick;
use sdl2::{GameControllerSubsystem, JoystickSubsystem};
use serde::{Deserialize, Serialize};

use crate::input::action::{Action, ActionConfig, Binding, RawInputState};
use crate::input::source::{InputDevice, InputSource};
use crate::input::mouse::Mouse;

/// The player's mouse sensitivity - what `mouse_look` multiplies the mouse's
/// movement by. 1: as the mouse reports it.
#[derive(Debug, Clone, Copy, PartialEq, Deserialize, Serialize)]
struct MouseSettings {
    x_sensitivity: f32,
    y_sensitivity: f32,
}

impl Default for MouseSettings {
    fn default() -> Self {
        Self { x_sensitivity: 1.0, y_sensitivity: 1.0 }
    }
}

/// A bindings file - see `load_bindings`.
#[derive(Debug, Deserialize)]
struct InputSettings {
    actions: Vec<ActionConfig>,
    #[serde(default)]
    mouse: MouseSettings,
}

/// A mouse button going down or up this frame, and where (window pixels) -
/// see `mouse_button_events`.
#[derive(Clone, Copy, Debug)]
pub struct MouseButtonEvent {
    pub button: sdl2::mouse::MouseButton,
    pub pressed: bool,
    pub x: i32,
    pub y: i32,
}

/// Raw keyboard typing this frame, in order - for text fields (see
/// `typed_input`), which need the characters typed and the editing keys
/// (backspace, arrows...) rather than game actions.
#[derive(Clone, Debug)]
pub enum TypedInput {
    /// Text typed - already through the keyboard layout (SDL's TextInput).
    Text(String),
    /// A key going down (`repeat`: held, auto-repeating) or up.
    Key { key: sdl2::keyboard::Keycode, pressed: bool, repeat: bool, ctrl: bool, shift: bool },
}

const USER_BINDINGS_HEADER: &str = "// Your controls - written by the game whenever a binding is changed,\n// read at start over the game's default bindings.\n// Delete this file to go back to the defaults.\n";

/// A joystick axis moved this far (of its -1..1 travel) from where it was
/// when capturing started is the one being bound.
const CAPTURE_AXIS_TRAVEL: f32 = 0.5;
/// A captured joystick axis's dead zone - flight sticks center precisely.
pub(crate) const JOYSTICK_DEADZONE: f32 = 0.05;

/// A joystick axis's raw value as -1..1.
fn joystick_axis_value(value: i16) -> f32 {
    (value as f32 / i16::MAX as f32).clamp(-1.0, 1.0)
}

/// The player's own controls file (see `set_user_bindings_path`): only what
/// they changed from the game's defaults - rebound actions, and their mouse
/// sensitivity if they set one.
#[derive(Debug, Default, Deserialize, Serialize)]
struct UserBindings {
    #[serde(default)]
    actions: Vec<ActionConfig>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    mouse: Option<MouseSettings>,
}

/// The player's saved controls at `path` - none if there's no file yet; a
/// broken one is reported and ignored. A file from before the mouse settings
/// (just the list of actions) is still read.
fn load_user_bindings(path: &Path) -> UserBindings {
    let Ok(text) = std::fs::read_to_string(path) else { return UserBindings::default() };
    ron::from_str::<UserBindings>(&text)
        .or_else(|error| ron::from_str::<Vec<ActionConfig>>(&text).map(|actions| UserBindings { actions, mouse: None }).map_err(|_| error))
        .unwrap_or_else(|error| {
            eprintln!("{} isn't valid ({error}) - using the default controls", path.display());
            UserBindings::default()
        })
}

/// How far an action's aggregated strength has to go before it counts as "pressed" -
/// buttons/keys report a clean 0.0 or 1.0 so this only really matters for sticks/triggers.
const PRESS_THRESHOLD: f32 = 0.5;

///    # Input - `app.input`

///    Action-based input: instead of checking a single key, you check a named action
///    ("jump") that can be bound to any mix of keyboard keys, mouse buttons, controller
///    buttons, and controller axes at once - whichever one is active wins. Bindings are
///    device-agnostic ("any connected controller's A button"), not tied to a specific
///    controller index, since nothing in this project needs per-player device
///    assignment yet; extending Binding::ControllerButton/Axis with an optional device
///    id later is additive if that ever changes.
///
///    Connected controllers are tracked here too (opened on ControllerDeviceAdded,
///    dropped on ControllerDeviceRemoved) so a controller plugged in mid-game works
///    immediately, without a separate one-shot "find a controller at startup" step.
///
///    The engine starts with no actions - the game gives it its default
///    bindings, a file shipped with the game, outside the assets folder (they
///    last the whole run, not a scene's):
///    `app.input.load_bindings("settings/input.ron")?` - or built in with
///    `load_bindings_from_str(include_str!(..))`. The player's own rebinds go
///    in their own folder, writable wherever the game is installed:
///    `app.input.set_user_bindings_path(Some(user_dir("Studio", "Game")?.join("bindings.ron")))`.
///
///    App::run polls SDL's events into it once per frame. Methods:
///    - is_action_pressed(action) / is_action_just_pressed(action) / is_action_just_released(action)
///    - action_strength(action) -> the raw 0.0..=1.0 value, for analog reads (throttle, stick deflection)
///    - get_axis(negative, positive) -> action_strength(positive) - action_strength(negative)
///    - begin_capture() / captured_binding() / cancel_capture() -> for a rebinding
///      menu: after begin_capture(), the next
///      key/button pressed or axis pushed past its deadzone is captured instead of
///      dispatched normally, and handed back via captured_binding() (polled once
///      per frame until it returns Some); cancel_capture() aborts without one.
///    - action_bindings(action) / rebind(action, binding) / rebind_at(action,
///      index, binding) / unbind(action, index) -> read/append/replace-in-place/
///      remove an action's bindings at runtime (saved to the user bindings
///      file, if there is one - never back to the game's own bindings file).
///    - rumble(low, high, duration_ms) / stop_rumble() -> every connected
///      controller and joystick that can.
///    - is_pressed(source) / is_just_pressed / is_just_released -> any key,
///      button or axis direction (see InputSource), whatever the bindings are.
///    - bind(action, source) -> adds a binding from code
///      (`bind("jump", InputSource::key(Key::Space))`), making the action if
///      it doesn't exist yet.
///    - read_input() / read_inputs() -> what was pressed this frame, as
///      InputSources (device + name) - without swallowing it the way
///      begin_capture() does.
///
///    A bindings file's shape (`mouse` is optional):
///    ```ron
///    (
///        actions: [
///            (action: "jump", bindings: [Key("space"), ControllerButton("A")]),
///        ],
///        mouse: (x_sensitivity: 0.5, y_sensitivity: 0.5),
///    )
///    ```
///    `mouse` is the sensitivity the game starts players at; their own (see
///    `set_mouse_sensitivity`) is saved with their rebinds.
pub struct Input {
    actions: HashMap<String, Action>,
    raw: RawInputState,
    // `raw` as it was last frame - what "just pressed/released" compares to.
    previous: RawInputState,
    // While a physics step's fixed_update runs (see begin_fixed_step) - then
    // "just pressed/released" compares to `fixed_baseline` instead: `raw` as
    // it was at the last step, so a press is seen by exactly one step.
    in_fixed_step: bool,
    fixed_baseline: RawInputState,
    // What went down this frame, in order - see `read_inputs`.
    pressed_sources: Vec<InputSource>,
    pub mouse: Mouse,
    controller_subsystem: GameControllerSubsystem,
    controllers: HashMap<u32, GameController>,
    joystick_subsystem: JoystickSubsystem,
    /// Every connected joystick that isn't a gamepad (those are
    /// `controllers`) - HOTAS sticks, throttles, pedals... - by instance id.
    /// Its name is what bindings know it by.
    joysticks: HashMap<u32, (Joystick, String)>,
    capturing: bool,
    captured_binding: Option<Binding>,
    /// Every action's bindings as the game's bindings file has them - what
    /// save_bindings compares against, so only the player's changes are
    /// saved.
    default_bindings: HashMap<String, Vec<Binding>>,
    /// The mouse sensitivity the game's bindings file gives - the same, for
    /// the player's mouse setting.
    default_mouse: MouseSettings,
    /// Where the player's own rebinds are saved and read back from - None:
    /// rebinds last until the game closes.
    user_bindings_path: Option<PathBuf>,
    /// Where every joystick axis was when capturing started - an axis only
    /// counts as "moved" once it's gone well away from there (a throttle
    /// lever can rest at either end).
    capture_baseline: HashMap<(String, u32), f32>,
    // This frame's typing - see TypedInput.
    typed: Vec<TypedInput>,
    // This frame's mouse button presses/releases, in order - see
    // `mouse_button_events`.
    mouse_button_events: Vec<MouseButtonEvent>,
}

impl Input {
    pub(crate) fn new(controller_subsystem: GameControllerSubsystem, joystick_subsystem: JoystickSubsystem) -> Self {
        let mouse_settings = MouseSettings::default();
        Self {
            actions: HashMap::new(),
            raw: RawInputState::default(),
            previous: RawInputState::default(),
            in_fixed_step: false,
            fixed_baseline: RawInputState::default(),
            pressed_sources: Vec::new(),
            mouse: Mouse::new(mouse_settings.x_sensitivity, mouse_settings.y_sensitivity),
            controller_subsystem,
            controllers: HashMap::new(),
            joystick_subsystem,
            joysticks: HashMap::new(),
            capturing: false,
            captured_binding: None,
            capture_baseline: HashMap::new(),
            default_bindings: HashMap::new(),
            default_mouse: mouse_settings,
            user_bindings_path: None,
            typed: Vec::new(),
            mouse_button_events: Vec::new(),
        }
    }

    /// Replaces every action with the ones in `text` (a bindings file's
    /// contents), then the player's own rebinds over them.
    fn load_bindings_text(&mut self, text: &str) -> Result<(), String> {
        let settings: InputSettings = ron::from_str(text).map_err(|error| error.to_string())?;
        self.actions.clear();
        self.default_bindings.clear();
        for action_config in settings.actions {
            self.default_bindings.insert(action_config.action.clone(), action_config.bindings.clone());
            self.actions.insert(action_config.action, Action::new(action_config.bindings));
        }
        self.mouse = Mouse::new(settings.mouse.x_sensitivity, settings.mouse.y_sensitivity);
        self.default_mouse = settings.mouse;
        self.apply_user_bindings();
        Ok(())
    }

    /// The player's own rebinds (see `user_bindings_path`), over the game's.
    fn apply_user_bindings(&mut self) {
        let Some(path) = &self.user_bindings_path else { return };
        let user = load_user_bindings(path);
        if let Some(mouse) = user.mouse {
            self.mouse.set_sensitivity(mouse.x_sensitivity, mouse.y_sensitivity);
        }
        for action_config in user.actions {
            if let Some(action) = self.actions.get_mut(&action_config.action) {
                action.bindings = action_config.bindings;
            }
        }
    }

    /// Every connected controller and joystick that can rumble, at `low`
    /// and `high` (0..1 - the heavy and the light motor) for `duration_ms`.
    pub fn rumble(&mut self, low: f32, high: f32, duration_ms: u32) {
        let low = (low.clamp(0.0, 1.0) * u16::MAX as f32) as u16;
        let high = (high.clamp(0.0, 1.0) * u16::MAX as f32) as u16;
        for controller in self.controllers.values_mut() {
            let _ = controller.set_rumble(low, high, duration_ms);
        }
        for (joystick, _) in self.joysticks.values_mut() {
            let _ = joystick.set_rumble(low, high, duration_ms);
        }
    }

    // Returns true if an Event::Quit (window close button, Cmd+Q, etc.) was
    // seen this poll - App::run's own loop is what actually decides what to
    // do with that (break its loop gracefully, sending a physics shutdown
    // command first - see that loop's own comment on why this used to be a
    // raw std::process::exit(0) here instead, and why that was wrong).
    pub(crate) fn update(&mut self, event_pump: &mut sdl2::EventPump, _delta_time: f32, debug: bool) -> bool {
        self.mouse.reset_rel_x();
        self.mouse.reset_rel_y();
        self.mouse.reset_scroll_y();
        self.typed.clear();
        self.mouse_button_events.clear();
        self.previous = self.raw.clone();
        self.pressed_sources.clear();

        let mut quit_requested = false;

        for event in event_pump.poll_iter() {
            if self.capturing {
                if let Some(binding) = self.event_to_binding(&event) {
                    self.captured_binding = Some(binding);
                    self.capturing = false;
                    continue;
                }
            }

            match event {
                Event::KeyDown { keycode: Some(key), keymod, repeat, .. } => {
                    self.typed.push(Self::typed_key(key, keymod, true, repeat));
                    self.raw.keys_down.insert(key.to_string().to_uppercase());
                    if !repeat {
                        self.pressed_sources.push(InputSource::key(key));
                    }
                    if debug { println!("Key {} pressed", key); }
                }
                Event::KeyUp { keycode: Some(key), keymod, .. } => {
                    self.typed.push(Self::typed_key(key, keymod, false, false));
                    self.raw.keys_down.remove(&key.to_string().to_uppercase());
                    if debug { println!("Key {} released", key); }
                }
                Event::TextInput { text, .. } => {
                    self.typed.push(TypedInput::Text(text));
                }
                Event::MouseButtonDown { mouse_btn, x, y, .. } => {
                    self.raw.mouse_buttons_down.insert(mouse_btn);
                    self.pressed_sources.push(InputSource::mouse(mouse_btn));
                    self.mouse_button_events.push(MouseButtonEvent { button: mouse_btn, pressed: true, x, y });
                }
                Event::MouseButtonUp { mouse_btn, x, y, .. } => {
                    self.raw.mouse_buttons_down.remove(&mouse_btn);
                    self.mouse_button_events.push(MouseButtonEvent { button: mouse_btn, pressed: false, x, y });
                }
                Event::MouseMotion { xrel, yrel, x, y, .. } => {
                    self.mouse.add_rel_x(xrel);
                    self.mouse.add_rel_y(yrel);

                    // For camera control
                    self.mouse.set_x(self.mouse.get_x() + xrel);
                    self.mouse.set_y((self.mouse.get_y() + yrel).clamp(-170, 170));

                    // For buttons
                    self.mouse.set_raw_x(x);
                    self.mouse.set_raw_y(y);
                }
                Event::MouseWheel { y, .. } => {
                    self.mouse.add_scroll_y(y as f32);
                }
                Event::ControllerButtonDown { button, .. } => {
                    self.raw.controller_buttons_down.insert(button);
                    self.pressed_sources.push(InputSource::controller(button));
                }
                Event::ControllerButtonUp { button, .. } => {
                    self.raw.controller_buttons_down.remove(&button);
                }
                Event::ControllerAxisMotion { axis, value, .. } => {
                    self.raw.controller_axes.insert(axis, value as f32 / i16::MAX as f32);
                }
                Event::ControllerDeviceAdded { which, .. } => {
                    // `which` is a device index here (not an instance id).
                    if let Ok(controller) = self.controller_subsystem.open(which) {
                        let instance_id = controller.instance_id();
                        println!("Controller connected: {}", controller.name());
                        self.controllers.insert(instance_id, controller);
                    }
                }
                Event::ControllerDeviceRemoved { which, .. } => {
                    // `which` is the instance id here (not a device index).
                    self.controllers.remove(&which);
                }
                // Joysticks that aren't gamepads - a gamepad is a joystick
                // too, but it's handled as a controller above (and its joystick
                // events are ignored below, not in `joysticks`).
                Event::JoyDeviceAdded { which, .. } => {
                    // `which` is a device index here.
                    if !self.controller_subsystem.is_game_controller(which) {
                        self.open_joystick(which);
                    }
                }
                Event::JoyDeviceRemoved { which, .. } => {
                    // `which` is the instance id here.
                    if let Some((_, name)) = self.joysticks.remove(&which) {
                        println!("Joystick disconnected: {name}");
                        self.raw.joystick_buttons_down.retain(|(device, _)| *device != name);
                        self.raw.joystick_axes.retain(|(device, _), _| *device != name);
                        self.raw.joystick_hats.retain(|(device, _), _| *device != name);
                    }
                }
                Event::JoyButtonDown { which, button_idx, .. } => {
                    if let Some(name) = self.joystick_name(which) {
                        self.pressed_sources.push(InputSource::named(InputDevice::Joystick(name.clone()), format!("Button {}", button_idx as u32 + 1)));
                        self.raw.joystick_buttons_down.insert((name, button_idx as u32));
                    }
                }
                Event::JoyButtonUp { which, button_idx, .. } => {
                    if let Some(name) = self.joystick_name(which) {
                        self.raw.joystick_buttons_down.remove(&(name, button_idx as u32));
                    }
                }
                Event::JoyAxisMotion { which, axis_idx, value, .. } => {
                    if let Some(name) = self.joystick_name(which) {
                        self.raw.joystick_axes.insert((name, axis_idx as u32), joystick_axis_value(value));
                    }
                }
                Event::JoyHatMotion { which, hat_idx, state, .. } => {
                    if let Some(name) = self.joystick_name(which) {
                        self.raw.joystick_hats.insert((name, hat_idx as u32), state);
                    }
                }
                Event::Quit { .. } => {
                    quit_requested = true;
                }
                _ => {}
            }
        }

        self.push_moved_axes();
        for action in self.actions.values_mut() {
            action.refresh(&self.raw, PRESS_THRESHOLD);
        }

        quit_requested
    }

    /// The axes pushed past halfway this frame, and the hats pushed a new
    /// way, onto `pressed_sources` - from how they were last frame, so one
    /// held counts once.
    fn push_moved_axes(&mut self) {
        // Which way `now` went past halfway since `before`, if it did.
        let pushed = |before: f32, now: f32| {
            if now > PRESS_THRESHOLD && before <= PRESS_THRESHOLD {
                Some("+")
            } else if now < -PRESS_THRESHOLD && before >= -PRESS_THRESHOLD {
                Some("-")
            } else {
                None
            }
        };
        for (axis, value) in &self.raw.controller_axes {
            let before = self.previous.controller_axes.get(axis).copied().unwrap_or(0.0);
            if let Some(sign) = pushed(before, *value) {
                self.pressed_sources.push(InputSource::named(InputDevice::Controller, format!("{axis:?}{sign}")));
            }
        }
        for ((device, axis), value) in &self.raw.joystick_axes {
            let before = self.previous.joystick_axes.get(&(device.clone(), *axis)).copied().unwrap_or(*value);
            if let Some(sign) = pushed(before, *value) {
                self.pressed_sources.push(InputSource::named(InputDevice::Joystick(device.clone()), format!("Axis {}{sign}", axis + 1)));
            }
        }
        for ((device, hat), state) in &self.raw.joystick_hats {
            let before = self.previous.joystick_hats.get(&(device.clone(), *hat)).copied().unwrap_or(sdl2::joystick::HatState::Centered);
            for direction in ["Up", "Down", "Left", "Right"] {
                if Binding::hat_points(*state, direction) && !Binding::hat_points(before, direction) {
                    self.pressed_sources.push(InputSource::named(InputDevice::Joystick(device.clone()), format!("Hat {} {direction}", hat + 1)));
                }
            }
        }
    }

    fn typed_key(key: sdl2::keyboard::Keycode, keymod: sdl2::keyboard::Mod, pressed: bool, repeat: bool) -> TypedInput {
        use sdl2::keyboard::Mod;
        TypedInput::Key {
            key,
            pressed,
            repeat,
            ctrl: keymod.intersects(Mod::LCTRLMOD | Mod::RCTRLMOD),
            shift: keymod.intersects(Mod::LSHIFTMOD | Mod::RSHIFTMOD),
        }
    }

    /// Opens the joystick at device index `index` and reads where its axes
    /// and hats are right now (SDL only reports them again once they move).
    fn open_joystick(&mut self, index: u32) {
        let Ok(joystick) = self.joystick_subsystem.open(index) else { return };
        let name = joystick.name();
        println!("Joystick connected: {name} ({} axes, {} buttons, {} hats)", joystick.num_axes(), joystick.num_buttons(), joystick.num_hats());
        for axis in 0..joystick.num_axes() {
            if let Ok(value) = joystick.axis(axis) {
                self.raw.joystick_axes.insert((name.clone(), axis), joystick_axis_value(value));
            }
        }
        for hat in 0..joystick.num_hats() {
            if let Ok(state) = joystick.hat(hat) {
                self.raw.joystick_hats.insert((name.clone(), hat), state);
            }
        }
        self.joysticks.insert(joystick.instance_id(), (joystick, name));
    }

    /// The name of the joystick with instance id `which` - `None` for one
    /// that isn't open (a gamepad's own joystick events).
    fn joystick_name(&self, which: u32) -> Option<String> {
        self.joysticks.get(&which).map(|(_, name)| name.clone())
    }

    fn event_to_binding(&self, event: &Event) -> Option<Binding> {
        match event {
            Event::JoyButtonDown { which, button_idx, .. } => {
                Some(Binding::JoystickButton { device: self.joystick_name(*which)?, button: *button_idx as u32 })
            }
            Event::JoyHatMotion { which, hat_idx, state, .. } => {
                let direction = ["Up", "Down", "Left", "Right"].into_iter().find(|direction| Binding::hat_points(*state, direction))?;
                Some(Binding::JoystickHat { device: self.joystick_name(*which)?, hat: *hat_idx as u32, direction: direction.to_owned() })
            }
            Event::JoyAxisMotion { which, axis_idx, value, .. } => {
                // Moved well away from where it was when capturing started -
                // that way is "positive".
                let device = self.joystick_name(*which)?;
                let axis = *axis_idx as u32;
                let start = self.capture_baseline.get(&(device.clone(), axis)).copied().unwrap_or(0.0);
                let moved = joystick_axis_value(*value) - start;
                (moved.abs() > CAPTURE_AXIS_TRAVEL).then(|| Binding::JoystickAxis {
                    device,
                    axis,
                    positive: moved > 0.0,
                    deadzone: JOYSTICK_DEADZONE,
                    full_range: false,
                })
            }
            Event::KeyDown { keycode: Some(key), .. } => Some(Binding::Key(key.to_string())),
            Event::MouseButtonDown { mouse_btn, .. } => Some(Binding::MouseButton(format!("{:?}", mouse_btn))),
            Event::ControllerButtonDown { button, .. } => Some(Binding::ControllerButton(format!("{:?}", button))),
            Event::ControllerAxisMotion { axis, value, .. } => {
                let normalized = *value as f32 / i16::MAX as f32;
                if normalized.abs() > 0.5 {
                    Some(Binding::ControllerAxis { axis: format!("{:?}", axis), positive: normalized > 0.0, deadzone: 0.25 })
                } else {
                    None
                }
            }
            _ => None,
        }
    }

    pub fn is_action_pressed(&self, action: &str) -> bool {
        match self.actions.get(action) {
            Some(action) => action.is_pressed(),
            None => { println!("Action {} not found", action); false }
        }
    }

    pub fn is_action_just_pressed(&self, action: &str) -> bool {
        match self.actions.get(action) {
            Some(action) => self.action_went(action, true),
            None => { println!("Action {} not found", action); false }
        }
    }

    pub fn is_action_just_released(&self, action: &str) -> bool {
        match self.actions.get(action) {
            Some(action) => self.action_went(action, false),
            None => { println!("Action {} not found", action); false }
        }
    }

    /// What "just" is measured from: last frame - or, in a physics step's
    /// fixed_update, the last step (see `begin_fixed_step`).
    fn baseline(&self) -> &RawInputState {
        if self.in_fixed_step { &self.fixed_baseline } else { &self.previous }
    }

    /// Whether `action` went down (`down`) or came up since the baseline.
    fn action_went(&self, action: &Action, down: bool) -> bool {
        if !self.in_fixed_step {
            return if down { action.is_just_pressed() } else { action.is_just_released() };
        }
        let was_pressed = action.bindings.iter().map(|binding| binding.strength(&self.fixed_baseline)).fold(0.0, f32::max) > PRESS_THRESHOLD;
        if down { action.is_pressed() && !was_pressed } else { !action.is_pressed() && was_pressed }
    }

    /// From here until `end_fixed_step`, "just pressed/released" means since
    /// the last physics step, not since last frame - see
    /// `Behavior::fixed_update`.
    pub(crate) fn begin_fixed_step(&mut self) {
        self.in_fixed_step = true;
    }

    /// Back to frames - and what's pressed now is what the next step
    /// compares to, so this step's presses aren't seen again.
    pub(crate) fn end_fixed_step(&mut self) {
        self.in_fixed_step = false;
        self.fixed_baseline = self.raw.clone();
    }

    pub fn action_strength(&self, action: &str) -> f32 {
        self.actions.get(action).map_or(0.0, Action::strength)
    }

    /// How far `source` is pushed, 0..1 - an action's strongest binding, a
    /// key or button 0 or 1, an axis how far along. 0 for one its device
    /// doesn't have, or an action that doesn't exist.
    pub fn strength(&self, source: impl Into<InputSource>) -> f32 {
        let source = source.into();
        match source.device {
            InputDevice::Action => self.action_strength(&source.name),
            _ => source.to_binding().map_or(0.0, |binding| binding.strength(&self.raw)),
        }
    }

    /// Whether `source` is held now - false for one its device doesn't have.
    pub fn is_pressed(&self, source: impl Into<InputSource>) -> bool {
        let source = source.into();
        match source.device {
            InputDevice::Action => self.actions.get(&source.name).is_some_and(Action::is_pressed),
            _ => source.to_binding().is_some_and(|binding| binding.strength(&self.raw) > PRESS_THRESHOLD),
        }
    }

    /// Whether `source` went down this frame (in a physics step's
    /// fixed_update: since the last step).
    pub fn is_just_pressed(&self, source: impl Into<InputSource>) -> bool {
        let source = source.into();
        match source.device {
            InputDevice::Action => self.actions.get(&source.name).is_some_and(|action| self.action_went(action, true)),
            _ => source.to_binding().is_some_and(|binding| binding.strength(&self.raw) > PRESS_THRESHOLD && binding.strength(self.baseline()) <= PRESS_THRESHOLD),
        }
    }

    /// Whether `source` came up this frame (in a physics step's
    /// fixed_update: since the last step).
    pub fn is_just_released(&self, source: impl Into<InputSource>) -> bool {
        let source = source.into();
        match source.device {
            InputDevice::Action => self.actions.get(&source.name).is_some_and(|action| self.action_went(action, false)),
            _ => source.to_binding().is_some_and(|binding| binding.strength(&self.raw) <= PRESS_THRESHOLD && binding.strength(self.baseline()) > PRESS_THRESHOLD),
        }
    }

    /// Adds `binding` to `action` - making the action if there isn't one -
    /// as one of the game's own (not saved as a player's rebind). One the
    /// action has already is left as it is.
    fn bind_binding(&mut self, action: &str, binding: Binding) {
        let defaults = self.default_bindings.entry(action.to_owned()).or_default();
        if !defaults.contains(&binding) {
            defaults.push(binding.clone());
        }
        let action = self.actions.entry(action.to_owned()).or_insert_with(|| Action::new(Vec::new()));
        if !action.bindings.contains(&binding) {
            action.bindings.push(binding);
        }
    }

    /// Starts listening for the next physical input, for a rebinding menu - see
    /// captured_binding().
    pub fn begin_capture(&mut self) {
        self.capturing = true;
        self.captured_binding = None;
        self.capture_baseline = self.raw.joystick_axes.clone();
    }

    /// The value of a lever bound to `action` (see `Binding::JoystickAxis::
    /// full_range`) - 0..1 end to end - while one's connected.
    pub fn absolute_value(&self, action: &str) -> Option<f32> {
        self.actions.get(action)?.absolute_value(&self.raw)
    }

    /// The bindings of every action the player has changed from the defaults,
    /// to the user bindings file - only those, so later changes to the
    /// defaults still reach every other action. Nothing without one.
    fn save_bindings(&self) {
        let Some(path) = &self.user_bindings_path else { return };
        let mut actions: Vec<ActionConfig> = self.actions.iter()
            .filter(|(name, action)| self.default_bindings.get(*name) != Some(&action.bindings))
            .map(|(name, action)| ActionConfig { action: name.clone(), bindings: action.bindings.clone() })
            .collect();
        actions.sort_by(|a, b| a.action.cmp(&b.action));
        let (x_sensitivity, y_sensitivity) = self.mouse.get_sensitivity();
        let mouse = MouseSettings { x_sensitivity, y_sensitivity };
        let user = UserBindings { actions, mouse: (mouse != self.default_mouse).then_some(mouse) };
        let text = match ron::ser::to_string_pretty(&user, ron::ser::PrettyConfig::new().indentor("    ")) {
            Ok(text) => text,
            Err(error) => {
                eprintln!("Couldn't write the bindings out: {error}");
                return;
            }
        };
        if let Err(error) = std::fs::write(path, format!("{USER_BINDINGS_HEADER}{text}\n")) {
            eprintln!("Couldn't save the bindings to {}: {error}", path.display());
        }
    }

    /// Takes whatever binding was captured since begin_capture(), if any yet.
    pub fn captured_binding(&mut self) -> Option<Binding> {
        self.captured_binding.take()
    }

    pub fn rebind(&mut self, action: &str, binding: Binding) {
        if let Some(action) = self.actions.get_mut(action) {
            action.bindings.push(binding);
        }
        self.save_bindings();
    }

    /// Like rebind, but replaces one binding in place by its index (see
    /// action_bindings) instead of appending - a no-op if the index is out of
    /// range, so a stale index from a UI that hasn't refreshed yet can't panic.
    pub fn rebind_at(&mut self, action: &str, index: usize, binding: Binding) {
        if let Some(action) = self.actions.get_mut(action) {
            if index < action.bindings.len() {
                action.bindings[index] = binding;
            }
        }
        self.save_bindings();
    }

    /// Cloned out (not a reference) since a rebinding menu needs to render this
    /// list into UI nodes it owns independently of the input subsystem's own
    /// lifetime/borrow rules.
    pub fn action_bindings(&self, action: &str) -> Vec<Binding> {
        self.actions.get(action).map(|a| a.bindings.clone()).unwrap_or_default()
    }

    /// Removes one binding by its index in that action's list (see
    /// action_bindings) - a no-op if the index is out of range, so a stale index
    /// from a UI that hasn't refreshed yet can't panic.
    pub fn unbind(&mut self, action: &str, index: usize) {
        if let Some(action) = self.actions.get_mut(action) {
            if index < action.bindings.len() {
                action.bindings.remove(index);
            }
        }
        self.save_bindings();
    }

    /// Drops every binding on `action`. One save, whether it had one key or several.
    pub fn clear_bindings(&mut self, action: &str) {
        let Some(action) = self.actions.get_mut(action) else { return };
        if action.bindings.is_empty() {
            return;
        }
        action.bindings.clear();
        self.save_bindings();
    }

    /// Aborts an in-progress begin_capture() without producing a binding - e.g. a
    /// rebinding menu's own "cancel" button, or closing the menu while still
    /// waiting on input.
    pub fn cancel_capture(&mut self) {
        self.capturing = false;
        self.captured_binding = None;
    }
}

// The game-facing half - what `app.input` is used through.
impl Input {
    /// Replaces every action with the ones in the bindings file at `path` (see
    /// Input's own doc comment for its shape) - then the player's own
    /// rebinds over them, if there's a user bindings file.
    pub fn load_bindings(&mut self, path: impl AsRef<Path>) -> Result<(), String> {
        let path = path.as_ref();
        let text = std::fs::read_to_string(path).map_err(|error| format!("couldn't read {}: {error}", path.display()))?;
        self.load_bindings_from_str(&text).map_err(|error| format!("{} isn't valid: {error}", path.display()))
    }

    /// `load_bindings` from the file's contents - for bindings built into the
    /// game (`include_str!`) instead of read at runtime.
    pub fn load_bindings_from_str(&mut self, text: &str) -> Result<(), String> {
        self.load_bindings_text(text)
    }

    /// Where the player's rebinds are saved (every change) and read back from,
    /// over the game's bindings - e.g. `user_dir("Studio", "Game")?.join("bindings.ron")`
    /// (see `user_dir`). None (the default): rebinds last until the game closes.
    pub fn set_user_bindings_path(&mut self, path: Option<impl Into<PathBuf>>) {
        self.user_bindings_path = path.map(Into::into);
        self.apply_user_bindings();
    }

    pub fn stop_rumble(&mut self) {
        self.rumble(0.0, 0.0, 0);
    }

    /// `positive`'s strength minus `negative`'s, -1..1 - each an action's name
    /// (`get_axis("Left", "Right")`) or any source
    /// (`get_axis(InputSource::key(Key::A), InputSource::key(Key::D))`), mixed
    /// as you like.
    pub fn get_axis(&self, negative: impl Into<InputSource>, positive: impl Into<InputSource>) -> f32 {
        self.strength(positive) - self.strength(negative)
    }

    /// Two axes as a direction - x from `left`/`right`, y from `up`/`down` (y
    /// down, like the screen) - each an action's name or any source (see
    /// `get_axis`) - as they are: moving diagonally gives a longer
    /// vector than straight. Chain `.normalize()` for one always 1 long (or
    /// zero, with nothing pressed):
    ///
    ///     let direction = app.input.get_vector("Left", "Right", "Up", "Down").normalize();
    ///     transform.position += direction * speed * app.time.delta_time;
    pub fn get_vector(&self, left: impl Into<InputSource>, right: impl Into<InputSource>, up: impl Into<InputSource>, down: impl Into<InputSource>) -> InputVector {
        InputVector(Vector2::new(self.get_axis(left, right), self.get_axis(up, down)))
    }

    /// Binds `action` to `source` - `InputSource::key(Key::Space)`,
    /// `InputSource::controller(Button::A)`, one from `read_input`... - making
    /// the action if it doesn't exist yet. The game's own binding, like its
    /// bindings file's: call it after `load_bindings` (which replaces every
    /// action) and before `set_user_bindings_path`, so the player's rebinds
    /// still win. A source built by hand with a name its device doesn't have is
    /// reported and left out.
    pub fn bind(&mut self, action: &str, source: impl Borrow<InputSource>) {
        let source = source.borrow();
        match source.to_binding() {
            Some(binding) => self.bind_binding(action, binding),
            None => eprintln!("input.bind(\"{action}\"): '{source}' isn't a key or a button - left out"),
        }
    }

    /// The first key, button or axis direction pressed this frame (see
    /// InputSource) - None when nothing was. It still counts as pressed for
    /// everything else, unlike `begin_capture`.
    pub fn read_input(&self) -> Option<InputSource> {
        self.pressed_sources.first().cloned()
    }

    /// Everything pressed this frame - keys and buttons in the order they went
    /// down, then axes and hats.
    pub fn read_inputs(&self) -> &[InputSource] {
        &self.pressed_sources
    }

    pub fn mouse_rel_x(&self) -> i32 {
        self.mouse.get_rel_x()
    }

    pub fn mouse_rel_y(&self) -> i32 {
        self.mouse.get_rel_y()
    }

    // Absolute, window-relative, top-left-origin pixels - the same coordinate space
    // UiTransform.rect is already in, so this is directly usable for UI hit-testing
    // (see UiNode::on_hover) with no conversion.
    pub fn mouse_x(&self) -> i32 {
        self.mouse.get_raw_x()
    }

    pub fn mouse_y(&self) -> i32 {
        self.mouse.get_raw_y()
    }

    pub fn mouse_scroll_y(&self) -> f32 {
        self.mouse.get_scroll_y()
    }

    // Raw left-button-down state, distinct from any bound action - for feeding
    // egui::RawInput (see engine::rendering::egui_overlay), which needs the
    // literal button state rather than a game action.
    pub fn mouse_left_button_down(&self) -> bool {
        self.raw.mouse_buttons_down.contains(&sdl2::mouse::MouseButton::Left)
    }

    /// Whether the left mouse button went down this frame - what the engine's UI
    /// counts as a click (see UiNode::on_click), whatever the game's bindings are.
    pub fn mouse_left_button_just_pressed(&self) -> bool {
        self.mouse_button_events.iter().any(|event| event.pressed && event.button == sdl2::mouse::MouseButton::Left)
    }

    // Raw right-button-down state - for feeding egui (see mouse_left_button_down).
    pub fn mouse_right_button_down(&self) -> bool {
        self.raw.mouse_buttons_down.contains(&sdl2::mouse::MouseButton::Right)
    }

    // Raw middle-button-down state - for feeding egui (see mouse_left_button_down).
    pub fn mouse_middle_button_down(&self) -> bool {
        self.raw.mouse_buttons_down.contains(&sdl2::mouse::MouseButton::Middle)
    }

    // Whether either Shift / Ctrl key is held right now - for feeding egui its
    // modifiers (see mouse_left_button_down).
    pub fn shift_down(&self) -> bool {
        self.raw.keys_down.contains("LEFT SHIFT") || self.raw.keys_down.contains("RIGHT SHIFT")
    }

    pub fn ctrl_down(&self) -> bool {
        self.raw.keys_down.contains("LEFT CTRL") || self.raw.keys_down.contains("RIGHT CTRL")
    }

    /// This frame's raw typing, in order - see `TypedInput`.
    pub fn typed_input(&self) -> &[TypedInput] {
        &self.typed
    }

    /// This frame's mouse button presses and releases, in order, with where
    /// each happened - every one of them, even a click quick enough to go down
    /// and up between two frames (which `mouse_left_button_down` never sees).
    /// For feeding egui (see egui_overlay.rs).
    pub fn mouse_button_events(&self) -> &[MouseButtonEvent] {
        &self.mouse_button_events
    }

    /// The player's mouse sensitivity, x and y - see `mouse_look`.
    pub fn mouse_sensitivity(&self) -> (f32, f32) {
        self.mouse.get_sensitivity()
    }

    /// Sets the player's mouse sensitivity (1: as the mouse reports it) - what
    /// `mouse_look` multiplies by. Saved with their controls (see
    /// `set_user_bindings_path`), so it's there next time.
    pub fn set_mouse_sensitivity(&mut self, x: f32, y: f32) {
        self.mouse.set_sensitivity(x, y);
        self.save_bindings();
    }

    /// How far the mouse moved this frame, times the player's sensitivity - for
    /// turning a camera, aiming: the game multiplies it by what one count means
    /// to it (an angle, a distance). Right and down are positive. Not rounded:
    /// a sensitivity of 0.8 gives 0.8 of a count. `mouse_rel_x/y` are the raw
    /// counts, for exact pixels.
    pub fn mouse_look(&self) -> Vector2<f32> {
        let (x_sensitivity, y_sensitivity) = self.mouse.get_sensitivity();
        Vector2::new(self.mouse.get_rel_x() as f32 * x_sensitivity, self.mouse.get_rel_y() as f32 * y_sensitivity)
    }
}

/// The folder this player's files for this game belong in - their rebinds
/// (see `Input::set_user_bindings_path`), saves... - made if it isn't there
/// yet. Per user and writable wherever the game is installed:
/// `%APPDATA%\org\game\` on Windows, `~/.local/share/game/` on Linux,
/// `~/Library/Application Support/game/` on macOS.
pub fn user_dir(org: &str, game: &str) -> Result<PathBuf, String> {
    sdl2::filesystem::pref_path(org, game).map(PathBuf::from).map_err(|error| error.to_string())
}

/// What `get_vector` returns - used like the `Vector2` it holds (`.x`,
/// `.norm()`, `* speed`), plus `normalize`.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct InputVector(pub Vector2<f32>);

impl InputVector {
    /// The same direction, 1 long - (0, 0) stays (0, 0) instead of turning
    /// into NaN the way `Vector2::normalize` would.
    pub fn normalize(self) -> InputVector {
        InputVector(self.0.try_normalize(0.0).unwrap_or_default())
    }
}

impl std::ops::Deref for InputVector {
    type Target = Vector2<f32>;

    fn deref(&self) -> &Vector2<f32> {
        &self.0
    }
}

impl std::ops::Mul<f32> for InputVector {
    type Output = Vector2<f32>;

    fn mul(self, scale: f32) -> Vector2<f32> {
        self.0 * scale
    }
}

impl From<InputVector> for Vector2<f32> {
    fn from(vector: InputVector) -> Self {
        vector.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_makes_a_diagonal_1_long_and_leaves_nothing_as_nothing() {
        let diagonal = InputVector(Vector2::new(1.0, -1.0)).normalize();
        assert!((diagonal.norm() - 1.0).abs() < 1e-6);
        assert_eq!(InputVector(Vector2::zeros()).normalize().0, Vector2::zeros());
        assert_eq!(InputVector(Vector2::new(0.0, 1.0)).normalize().0, Vector2::new(0.0, 1.0));
    }
}
