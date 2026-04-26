//! Frame timing and FPS tracking.

use std::time::Instant;

pub struct FrameTimer {
    pub(crate) last_instant: Instant,
    pub(crate) frame_count: u64,
    pub(crate) current_fps: f32,
    pub(crate) fps_accumulator: f32,
    pub(crate) fps_frames: u32,
}

impl FrameTimer {
    pub fn new() -> Self {
        Self {
            last_instant: Instant::now(),
            frame_count: 0,
            current_fps: 0.0,
            fps_accumulator: 0.0,
            fps_frames: 0,
        }
    }

    pub fn tick(&mut self) -> f32 {
        let now = Instant::now();
        let dt = (now - self.last_instant).as_secs_f32();
        self.last_instant = now;
        self.apply_delta(dt);
        dt
    }

    pub(crate) fn apply_delta(&mut self, dt: f32) {
        self.frame_count += 1;
        self.fps_accumulator += dt;
        self.fps_frames += 1;
        if self.fps_accumulator >= 1.0 {
            self.current_fps = self.fps_frames as f32 / self.fps_accumulator;
            self.fps_accumulator = 0.0;
            self.fps_frames = 0;
        }
    }

    pub fn fps(&self) -> f32 {
        self.current_fps
    }

    pub fn frame_count(&self) -> u64 {
        self.frame_count
    }
}

impl Default for FrameTimer {
    fn default() -> Self {
        Self::new()
    }
}
