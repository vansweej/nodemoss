//! Input state and key handling.

use std::collections::HashSet;

use rig_math::Vec2;
use winit::event::{ElementState, MouseButton};
use winit::keyboard::KeyCode;

pub struct InputState {
    pub(crate) keys: HashSet<KeyCode>,
    /// Currently pressed mouse buttons.
    pub mouse_buttons: HashSet<MouseButton>,
    /// Current cursor position in pixels, origin top-left.
    pub mouse_position: Vec2,
    /// Per-frame accumulated cursor movement, reset each frame.
    pub mouse_delta: Vec2,
}

impl Default for InputState {
    fn default() -> Self {
        Self {
            keys: HashSet::new(),
            mouse_buttons: HashSet::new(),
            mouse_position: Vec2::ZERO,
            mouse_delta: Vec2::ZERO,
        }
    }
}

impl InputState {
    pub fn is_key_pressed(&self, key: KeyCode) -> bool {
        self.keys.contains(&key)
    }

    /// Returns true if the given mouse button is currently pressed.
    pub fn is_mouse_button_pressed(&self, button: MouseButton) -> bool {
        self.mouse_buttons.contains(&button)
    }

    /// Update cursor position and accumulate delta.
    pub fn update_mouse_position(&mut self, new_pos: Vec2) {
        self.mouse_delta += new_pos - self.mouse_position;
        self.mouse_position = new_pos;
    }

    /// Update mouse button pressed/released state.
    pub fn update_mouse_button(&mut self, button: MouseButton, state: ElementState) {
        match state {
            ElementState::Pressed => {
                self.mouse_buttons.insert(button);
            }
            ElementState::Released => {
                self.mouse_buttons.remove(&button);
            }
        }
    }

    /// Reset per-frame mouse delta. Call at the start of each frame.
    pub fn reset_mouse_delta(&mut self) {
        self.mouse_delta = Vec2::ZERO;
    }

    #[cfg(not(tarpaulin_include))]
    pub(crate) fn update(&mut self, event: &winit::event::KeyEvent) {
        if let winit::keyboard::PhysicalKey::Code(code) = event.physical_key {
            self.update_key(code, event.state);
        }
    }

    pub(crate) fn update_key(&mut self, code: KeyCode, state: winit::event::ElementState) {
        match state {
            winit::event::ElementState::Pressed => {
                self.keys.insert(code);
            }
            winit::event::ElementState::Released => {
                self.keys.remove(&code);
            }
        }
    }
}

pub(crate) fn key_axis(input: &InputState, negative: KeyCode, positive: KeyCode) -> f32 {
    let negative = input.is_key_pressed(negative) as i8;
    let positive = input.is_key_pressed(positive) as i8;
    (positive - negative) as f32
}
