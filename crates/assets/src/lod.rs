//! Distance-based terrain level-of-detail utilities.

/// A single LOD level definition.
#[derive(Clone, Copy, Debug)]
pub struct LodLevel {
    /// Maximum distance (world units) at which this LOD is used.
    pub max_distance: f32,
    /// Grid resolution (cols and rows) for terrain mesh generation.
    pub resolution: u32,
}

/// Select terrain resolution based on camera distance.
///
/// Levels must be ordered nearest→farthest. Returns the first level's resolution
/// where `distance <= level.max_distance`. Falls back to the last level's resolution
/// if distance exceeds all levels. Returns 16 if `levels` is empty.
pub fn select_lod(distance: f32, levels: &[LodLevel]) -> u32 {
    for level in levels {
        if distance <= level.max_distance {
            return level.resolution;
        }
    }
    levels.last().map(|level| level.resolution).unwrap_or(16)
}

/// Returns `Some(new_resolution)` if the chunk should be regenerated at a different
/// LOD, `None` if `current_resolution` already matches the desired level.
pub fn needs_lod_update(
    current_resolution: u32,
    camera_distance: f32,
    levels: &[LodLevel],
) -> Option<u32> {
    let desired = select_lod(camera_distance, levels);
    (desired != current_resolution).then_some(desired)
}

#[cfg(test)]
mod tests {
    use super::*;

    const LEVELS: [LodLevel; 3] = [
        LodLevel {
            max_distance: 128.0,
            resolution: 64,
        },
        LodLevel {
            max_distance: 256.0,
            resolution: 32,
        },
        LodLevel {
            max_distance: 512.0,
            resolution: 16,
        },
    ];

    #[test]
    fn select_lod_returns_highest_for_near_distance() {
        assert_eq!(select_lod(50.0, &LEVELS), 64);
    }

    #[test]
    fn select_lod_returns_lowest_for_far_distance() {
        assert_eq!(select_lod(999.0, &LEVELS), 16);
    }

    #[test]
    fn select_lod_at_exact_boundary() {
        assert_eq!(select_lod(128.0, &LEVELS), 64);
    }

    #[test]
    fn select_lod_empty_levels_returns_16() {
        assert_eq!(select_lod(50.0, &[]), 16);
    }

    #[test]
    fn needs_lod_update_returns_none_when_same() {
        assert_eq!(needs_lod_update(64, 50.0, &LEVELS), None);
    }

    #[test]
    fn needs_lod_update_returns_some_when_different() {
        assert_eq!(needs_lod_update(64, 200.0, &LEVELS), Some(32));
    }
}
