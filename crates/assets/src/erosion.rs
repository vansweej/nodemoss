//! CPU hydraulic erosion for row-major heightmap grids.

/// Parameters for droplet-based hydraulic erosion.
#[derive(Clone, Debug)]
pub struct ErosionParams {
    /// Number of water droplets to simulate. Typical: 50_000–200_000.
    pub iterations: u32,
    /// Maximum steps per droplet before evaporation.
    pub max_lifetime: u32,
    /// Droplet inertia [0, 1]. 0 = instant turn, 1 = never turns. Typical: 0.05.
    pub inertia: f32,
    /// Sediment carrying capacity multiplier. Typical: 6.0.
    pub capacity: f32,
    /// Fraction of capacity deficit eroded per step. Typical: 0.4.
    pub erosion_rate: f32,
    /// Fraction of excess sediment deposited per step. Typical: 0.4.
    pub deposition_rate: f32,
    /// Minimum slope to allow erosion (prevents flat-area instability). Typical: 0.01.
    pub min_slope: f32,
    /// Gravity constant for velocity update. Typical: 6.0.
    pub gravity: f32,
    /// Water evaporation per step: volume *= (1 - evaporation). Typical: 0.02.
    pub evaporation: f32,
    /// Erosion brush radius in grid cells. Typical: 4.
    pub erosion_radius: u32,
    /// Initial water volume per droplet. Typical: 1.0.
    pub initial_volume: f32,
    /// Initial droplet speed. Typical: 1.0.
    pub initial_speed: f32,
}

impl Default for ErosionParams {
    fn default() -> Self {
        Self {
            iterations: 100_000,
            max_lifetime: 64,
            inertia: 0.05,
            capacity: 6.0,
            erosion_rate: 0.4,
            deposition_rate: 0.4,
            min_slope: 0.01,
            gravity: 6.0,
            evaporation: 0.02,
            erosion_radius: 4,
            initial_volume: 1.0,
            initial_speed: 1.0,
        }
    }
}

/// Run hydraulic erosion on a heightmap grid in-place.
///
/// `heights` is a row-major flat array of `cols * rows` f32 height values.
/// Uses a deterministic LCG for reproducibility without depending on `rand`.
pub fn erode(heights: &mut [f32], cols: u32, rows: u32, params: &ErosionParams) {
    let cols = cols as usize;
    let rows = rows as usize;
    let Some(required_len) = cols.checked_mul(rows) else {
        return;
    };
    if cols < 3 || rows < 3 || params.iterations == 0 || heights.len() < required_len {
        return;
    }

    let brush = ErosionBrush::new(params.erosion_radius);
    let mut rng = Lcg::new(params.iterations);
    let max_x = cols as f32 - 1.0;
    let max_z = rows as f32 - 1.0;
    let inertia = params.inertia.clamp(0.0, 1.0);
    let evaporation = params.evaporation.clamp(0.0, 1.0);

    for _ in 0..params.iterations {
        let mut pos_x = rng.next_f32() * max_x;
        let mut pos_z = rng.next_f32() * max_z;
        let mut dir_x = 0.0_f32;
        let mut dir_z = 0.0_f32;
        let mut speed = params.initial_speed.max(0.0);
        let mut volume = params.initial_volume.max(0.0);
        let mut sediment = 0.0_f32;

        for _ in 0..params.max_lifetime {
            if volume < 0.001 || !inside_bilinear_bounds(pos_x, pos_z, cols, rows) {
                break;
            }

            let sample = sample_height_and_gradient(heights, cols, pos_x, pos_z);
            dir_x = dir_x * inertia - sample.gradient_x * (1.0 - inertia);
            dir_z = dir_z * inertia - sample.gradient_z * (1.0 - inertia);

            let dir_len = (dir_x * dir_x + dir_z * dir_z).sqrt();
            if dir_len <= f32::EPSILON {
                break;
            }
            dir_x /= dir_len;
            dir_z /= dir_len;

            let next_x = pos_x + dir_x;
            let next_z = pos_z + dir_z;
            if !inside_bilinear_bounds(next_x, next_z, cols, rows) {
                break;
            }

            let next_height = sample_height_and_gradient(heights, cols, next_x, next_z).height;
            let delta_height = next_height - sample.height;
            let capacity =
                (-delta_height).max(params.min_slope).max(0.0) * speed * volume * params.capacity;

            if sediment > capacity || delta_height > 0.0 {
                let amount = if delta_height > 0.0 {
                    sediment.min(delta_height)
                } else {
                    (sediment - capacity) * params.deposition_rate
                };
                if amount > 0.0 {
                    deposit_bilinear(heights, cols, rows, pos_x, pos_z, amount);
                    sediment -= amount;
                }
            } else {
                let amount = ((capacity - sediment) * params.erosion_rate).max(0.0);
                if amount > 0.0 {
                    sediment += erode_with_brush(heights, cols, rows, pos_x, pos_z, amount, &brush);
                }
            }

            let speed_squared = speed * speed - delta_height * params.gravity;
            if speed_squared <= 0.0 {
                break;
            }
            speed = speed_squared.sqrt();
            volume *= 1.0 - evaporation;
            pos_x = next_x;
            pos_z = next_z;
        }

        if sediment > 0.0 && inside_bilinear_bounds(pos_x, pos_z, cols, rows) {
            deposit_bilinear(heights, cols, rows, pos_x, pos_z, sediment);
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct HeightSample {
    height: f32,
    gradient_x: f32,
    gradient_z: f32,
}

fn sample_height_and_gradient(heights: &[f32], cols: usize, x: f32, z: f32) -> HeightSample {
    let cell_x = x.floor() as usize;
    let cell_z = z.floor() as usize;
    let offset_x = x - cell_x as f32;
    let offset_z = z - cell_z as f32;

    let h00 = heights[cell_z * cols + cell_x];
    let h10 = heights[cell_z * cols + cell_x + 1];
    let h01 = heights[(cell_z + 1) * cols + cell_x];
    let h11 = heights[(cell_z + 1) * cols + cell_x + 1];

    let height = h00 * (1.0 - offset_x) * (1.0 - offset_z)
        + h10 * offset_x * (1.0 - offset_z)
        + h01 * (1.0 - offset_x) * offset_z
        + h11 * offset_x * offset_z;
    let gradient_x = (h10 - h00) * (1.0 - offset_z) + (h11 - h01) * offset_z;
    let gradient_z = (h01 - h00) * (1.0 - offset_x) + (h11 - h10) * offset_x;

    HeightSample {
        height,
        gradient_x,
        gradient_z,
    }
}

fn deposit_bilinear(heights: &mut [f32], cols: usize, rows: usize, x: f32, z: f32, amount: f32) {
    if amount <= 0.0 || !inside_bilinear_bounds(x, z, cols, rows) {
        return;
    }
    let cell_x = x.floor() as usize;
    let cell_z = z.floor() as usize;
    let offset_x = x - cell_x as f32;
    let offset_z = z - cell_z as f32;

    let top_left = (1.0 - offset_x) * (1.0 - offset_z);
    let top_right = offset_x * (1.0 - offset_z);
    let bottom_left = (1.0 - offset_x) * offset_z;
    let bottom_right = offset_x * offset_z;

    heights[cell_z * cols + cell_x] += amount * top_left;
    heights[cell_z * cols + cell_x + 1] += amount * top_right;
    heights[(cell_z + 1) * cols + cell_x] += amount * bottom_left;
    heights[(cell_z + 1) * cols + cell_x + 1] += amount * bottom_right;
}

fn erode_with_brush(
    heights: &mut [f32],
    cols: usize,
    rows: usize,
    x: f32,
    z: f32,
    amount: f32,
    brush: &ErosionBrush,
) -> f32 {
    if amount <= 0.0 {
        return 0.0;
    }

    let center_x = x.floor() as i32;
    let center_z = z.floor() as i32;
    let mut eroded = 0.0_f32;

    for sample in &brush.samples {
        let cell_x = center_x + sample.dx;
        let cell_z = center_z + sample.dz;
        if cell_x < 0 || cell_z < 0 || cell_x >= cols as i32 || cell_z >= rows as i32 {
            continue;
        }
        let erosion = amount * sample.weight;
        let idx = cell_z as usize * cols + cell_x as usize;
        heights[idx] -= erosion;
        eroded += erosion;
    }

    eroded
}

fn inside_bilinear_bounds(x: f32, z: f32, cols: usize, rows: usize) -> bool {
    x >= 0.0 && z >= 0.0 && x < cols as f32 - 1.0 && z < rows as f32 - 1.0
}

#[derive(Clone, Debug)]
struct BrushSample {
    dx: i32,
    dz: i32,
    weight: f32,
}

#[derive(Clone, Debug)]
struct ErosionBrush {
    samples: Vec<BrushSample>,
}

impl ErosionBrush {
    fn new(radius: u32) -> Self {
        if radius == 0 {
            return Self {
                samples: vec![BrushSample {
                    dx: 0,
                    dz: 0,
                    weight: 1.0,
                }],
            };
        }

        let radius_i = radius as i32;
        let radius_f = radius as f32;
        let mut samples = Vec::new();
        let mut total = 0.0_f32;
        for dz in -radius_i..=radius_i {
            for dx in -radius_i..=radius_i {
                let distance = ((dx * dx + dz * dz) as f32).sqrt();
                let weight = (radius_f - distance).max(0.0);
                if weight > 0.0 {
                    samples.push(BrushSample { dx, dz, weight });
                    total += weight;
                }
            }
        }

        if total <= f32::EPSILON {
            return Self::new(0);
        }
        for sample in &mut samples {
            sample.weight /= total;
        }
        Self { samples }
    }
}

#[derive(Clone, Copy, Debug)]
struct Lcg {
    state: u32,
}

impl Lcg {
    fn new(seed: u32) -> Self {
        Self { state: seed }
    }

    fn next_f32(&mut self) -> f32 {
        self.state = self.state.wrapping_mul(1_103_515_245).wrapping_add(12_345);
        (self.state >> 16) as f32 / 65_536.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn erode_with_zero_iterations_does_not_modify_heights() {
        let mut heights = vec![1.0_f32; 25];
        let before = heights.clone();

        erode(
            &mut heights,
            5,
            5,
            &ErosionParams {
                iterations: 0,
                ..Default::default()
            },
        );

        assert_eq!(heights, before);
    }

    #[test]
    fn erode_on_flat_grid_produces_no_change() {
        let mut heights = vec![3.0_f32; 64];
        let before = heights.clone();

        erode(
            &mut heights,
            8,
            8,
            &ErosionParams {
                iterations: 1_000,
                ..Default::default()
            },
        );

        assert_eq!(heights, before);
    }

    #[test]
    fn erode_on_slope_reduces_peak_height() {
        let cols = 17;
        let rows = 17;
        let center_x = (cols / 2) as f32;
        let center_z = (rows / 2) as f32;
        let mut heights = Vec::with_capacity(cols * rows);
        for z in 0..rows {
            for x in 0..cols {
                let dx = x as f32 - center_x;
                let dz = z as f32 - center_z;
                heights.push((10.0 - (dx * dx + dz * dz).sqrt()).max(0.0));
            }
        }
        let peak_index = (rows / 2) * cols + cols / 2;
        let before = heights[peak_index];

        erode(
            &mut heights,
            cols as u32,
            rows as u32,
            &ErosionParams {
                iterations: 5_000,
                erosion_radius: 3,
                ..Default::default()
            },
        );

        assert!(heights[peak_index] < before);
    }

    #[test]
    fn erode_preserves_total_mass_approximately() {
        let cols = 24;
        let rows = 24;
        let mut heights = Vec::with_capacity(cols * rows);
        for z in 0..rows {
            for x in 0..cols {
                heights.push((x as f32 * 0.2).sin() + (z as f32 * 0.15).cos() + 4.0);
            }
        }
        let before: f32 = heights.iter().sum();

        erode(
            &mut heights,
            cols as u32,
            rows as u32,
            &ErosionParams {
                iterations: 2_000,
                erosion_radius: 2,
                ..Default::default()
            },
        );

        let after: f32 = heights.iter().sum();
        assert!(((after - before) / before).abs() <= 0.01);
    }
}
