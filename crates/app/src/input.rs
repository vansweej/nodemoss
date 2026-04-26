//! Input state and key handling.

use std::collections::HashSet;

use winit::keyboard::KeyCode;

#[derive(Default)]
pub struct InputState {
    pub(crate) keys: HashSet<KeyCode>,
}

impl InputState {
    pub fn is_key_pressed(&self, key: KeyCode) -> bool {
        self.keys.contains(&key)
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
