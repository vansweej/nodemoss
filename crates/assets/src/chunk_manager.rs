//! Camera-driven terrain chunk activation bookkeeping.

use std::collections::HashSet;

/// Integer chunk coordinate in the XZ plane.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ChunkCoord {
    pub x: i32,
    pub z: i32,
}

/// Result of a chunk manager update.
#[derive(Clone, Debug, Default)]
pub struct ChunkUpdate {
    /// Chunks that entered the active radius — make visible.
    pub to_create: Vec<ChunkCoord>,
    /// Chunks that exited the unload radius — hide.
    pub to_destroy: Vec<ChunkCoord>,
}

pub struct ChunkManager {
    pub chunk_size: f32,
    pub load_radius: u32,
    /// Must be >= load_radius. Creates a hysteresis band.
    pub unload_radius: u32,
    active_chunks: HashSet<ChunkCoord>,
    last_camera_chunk: Option<ChunkCoord>,
}

impl ChunkManager {
    /// Panics if unload_radius < load_radius or chunk_size <= 0.0.
    pub fn new(chunk_size: f32, load_radius: u32, unload_radius: u32) -> Self {
        assert!(chunk_size > 0.0, "chunk_size must be positive");
        assert!(
            unload_radius >= load_radius,
            "unload_radius must be >= load_radius"
        );
        Self {
            chunk_size,
            load_radius,
            unload_radius,
            active_chunks: HashSet::new(),
            last_camera_chunk: None,
        }
    }

    /// floor(world / chunk_size) per axis.
    pub fn world_to_chunk(&self, world_x: f32, world_z: f32) -> ChunkCoord {
        ChunkCoord {
            x: (world_x / self.chunk_size).floor() as i32,
            z: (world_z / self.chunk_size).floor() as i32,
        }
    }

    /// Center of chunk in world space (Y = 0).
    pub fn chunk_center(&self, coord: ChunkCoord) -> (f32, f32) {
        (
            coord.x as f32 * self.chunk_size + self.chunk_size * 0.5,
            coord.z as f32 * self.chunk_size + self.chunk_size * 0.5,
        )
    }

    /// Update active set. Returns empty ChunkUpdate if camera hasn't moved chunks.
    pub fn update(&mut self, camera_x: f32, camera_z: f32) -> ChunkUpdate {
        let current = self.world_to_chunk(camera_x, camera_z);
        if self.last_camera_chunk == Some(current) {
            return ChunkUpdate::default();
        }
        self.update_for_chunk(current)
    }

    pub fn active_chunks(&self) -> impl Iterator<Item = &ChunkCoord> {
        self.active_chunks.iter()
    }

    pub fn active_count(&self) -> usize {
        self.active_chunks.len()
    }

    /// Force initial population regardless of last position.
    pub fn initialize(&mut self, camera_x: f32, camera_z: f32) -> ChunkUpdate {
        let current = self.world_to_chunk(camera_x, camera_z);
        self.last_camera_chunk = None;
        self.update_for_chunk(current)
    }

    fn update_for_chunk(&mut self, current: ChunkCoord) -> ChunkUpdate {
        self.last_camera_chunk = Some(current);
        let desired = coords_within_radius(current, self.load_radius);
        let to_create: Vec<_> = desired
            .iter()
            .copied()
            .filter(|coord| !self.active_chunks.contains(coord))
            .collect();
        let to_destroy: Vec<_> = self
            .active_chunks
            .iter()
            .copied()
            .filter(|coord| chebyshev_distance(*coord, current) > self.unload_radius)
            .collect();

        for coord in &to_create {
            self.active_chunks.insert(*coord);
        }
        for coord in &to_destroy {
            self.active_chunks.remove(coord);
        }

        ChunkUpdate {
            to_create,
            to_destroy,
        }
    }
}

fn coords_within_radius(center: ChunkCoord, radius: u32) -> Vec<ChunkCoord> {
    let radius = radius as i32;
    let side = radius * 2 + 1;
    let mut coords = Vec::with_capacity((side * side) as usize);
    for z in center.z - radius..=center.z + radius {
        for x in center.x - radius..=center.x + radius {
            coords.push(ChunkCoord { x, z });
        }
    }
    coords
}

fn chebyshev_distance(a: ChunkCoord, b: ChunkCoord) -> u32 {
    (a.x - b.x).abs().max((a.z - b.z).abs()) as u32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn world_to_chunk_maps_correctly() {
        let manager = ChunkManager::new(64.0, 2, 2);

        assert_eq!(
            manager.world_to_chunk(31.0, 31.0),
            ChunkCoord { x: 0, z: 0 }
        );
        assert_eq!(manager.world_to_chunk(64.0, 0.0), ChunkCoord { x: 1, z: 0 });
        assert_eq!(
            manager.world_to_chunk(-1.0, 0.0),
            ChunkCoord { x: -1, z: 0 }
        );
    }

    #[test]
    fn initialize_populates_correct_radius() {
        let mut manager = ChunkManager::new(64.0, 2, 2);

        let update = manager.initialize(0.0, 0.0);

        assert_eq!(update.to_create.len(), 25);
        assert_eq!(manager.active_count(), 25);
        assert!(update.to_destroy.is_empty());
    }

    #[test]
    fn update_no_movement_returns_empty() {
        let mut manager = ChunkManager::new(64.0, 1, 1);
        manager.initialize(0.0, 0.0);

        let update = manager.update(12.0, 24.0);

        assert!(update.to_create.is_empty());
        assert!(update.to_destroy.is_empty());
    }

    #[test]
    fn update_creates_and_destroys_on_movement() {
        let mut manager = ChunkManager::new(64.0, 1, 1);
        manager.initialize(0.0, 0.0);

        let update = manager.update(128.0, 0.0);

        assert!(update.to_create.contains(&ChunkCoord { x: 2, z: 0 }));
        assert!(update.to_create.contains(&ChunkCoord { x: 3, z: 0 }));
        assert!(update.to_destroy.contains(&ChunkCoord { x: -1, z: 0 }));
        assert!(update.to_destroy.contains(&ChunkCoord { x: 0, z: 0 }));
    }

    #[test]
    fn hysteresis_prevents_thrashing() {
        let mut manager = ChunkManager::new(64.0, 1, 2);
        manager.initialize(0.0, 0.0);

        let update = manager.update(64.0, 0.0);

        assert!(update.to_destroy.is_empty());
        assert!(
            manager
                .active_chunks()
                .any(|coord| *coord == ChunkCoord { x: -1, z: 0 })
        );
    }

    #[test]
    fn chunk_center_round_trip() {
        let manager = ChunkManager::new(64.0, 2, 2);
        let coord = ChunkCoord { x: -3, z: 4 };

        let (x, z) = manager.chunk_center(coord);

        assert_eq!(manager.world_to_chunk(x, z), coord);
    }
}
