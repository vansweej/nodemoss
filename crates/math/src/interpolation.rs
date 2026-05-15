//! Keyframe interpolation utilities for animation playback.
//!
//! All functions are allocation-free and `unsafe`-free. They are designed for
//! use in `rig-anim`'s `AnimationPlayer::evaluate` but have no dependency on
//! any other rig crate.

use crate::{Quat, Vec3};

/// Interpolation mode for keyframe animation channels.
///
/// Matches the three modes defined in the glTF 2.0 specification
/// ([Appendix C](https://registry.khronos.org/glTF/specs/2.0/glTF-2.0.html#appendix-c-interpolation)).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Interpolation {
    /// Snap to the value of the previous keyframe. No interpolation.
    Step,
    /// Linear interpolation: `lerp` for Vec3, `slerp` for Quat.
    Linear,
    /// Cubic Hermite spline per glTF Appendix C. Requires tangent data.
    CubicSpline,
}

/// Find the keyframe interval containing `time` and return the lower index
/// and the normalized interpolation factor `t ∈ [0, 1]`.
///
/// Uses a cached hint (`*hint`) for O(1) amortized sequential access: if
/// `time` still falls in the interval `[times[*hint], times[*hint + 1])` the
/// result is returned immediately without a binary search.
///
/// # Edge cases
/// - `times.len() <= 1`: returns `(0, 0.0)` — caller uses the sole value.
/// - `time <= times[0]`: returns `(0, 0.0)`.
/// - `time >= times[last]`: returns `(last - 1, 1.0)`.
///
/// `*hint` is updated to the returned index before returning.
pub fn find_keyframe_index(times: &[f32], time: f32, hint: &mut usize) -> (usize, f32) {
    // Single-keyframe or empty: nothing to interpolate.
    if times.len() <= 1 {
        *hint = 0;
        return (0, 0.0);
    }

    let last = times.len() - 1;

    // Clamp below.
    if time <= times[0] {
        *hint = 0;
        return (0, 0.0);
    }

    // Clamp above.
    if time >= times[last] {
        *hint = last - 1;
        return (last - 1, 1.0);
    }

    // Fast path: check if the cached hint is still valid.
    let h = (*hint).min(last - 1);
    if time >= times[h] && time < times[h + 1] {
        let t = (time - times[h]) / (times[h + 1] - times[h]);
        return (h, t.clamp(0.0, 1.0));
    }

    // Binary search fallback.
    let i = times
        .partition_point(|&t| t <= time)
        .saturating_sub(1)
        .min(last - 1);
    *hint = i;
    let t = (time - times[i]) / (times[i + 1] - times[i]);
    (i, t.clamp(0.0, 1.0))
}

// ── Step sampling ─────────────────────────────────────────────────────────────

/// Return the value at `index` (step / no interpolation).
pub fn sample_vec3_step(values: &[Vec3], index: usize) -> Vec3 {
    values[index]
}

/// Return the value at `index` (step / no interpolation).
pub fn sample_quat_step(values: &[Quat], index: usize) -> Quat {
    values[index]
}

// ── Linear sampling ───────────────────────────────────────────────────────────

/// Linearly interpolate between `values[index]` and `values[index + 1]`.
pub fn sample_vec3_linear(values: &[Vec3], index: usize, t: f32) -> Vec3 {
    values[index].lerp(values[index + 1], t)
}

/// Spherically interpolate between `values[index]` and `values[index + 1]`.
pub fn sample_quat_linear(values: &[Quat], index: usize, t: f32) -> Quat {
    values[index].slerp(values[index + 1], t)
}

// ── Cubic Hermite spline ──────────────────────────────────────────────────────

/// Cubic Hermite spline evaluation matching glTF Appendix C.
///
/// ```text
/// p(t) = (2t³ - 3t² + 1)·v0  +  (t³ - 2t² + t)·m0
///       + (-2t³ + 3t²)·v1    +  (t³ - t²)·m1
/// ```
///
/// `m0` and `m1` must already be scaled by `deltaTime = times[i+1] - times[i]`
/// (the caller is responsible for this, matching the glTF spec).
pub fn cubic_hermite_vec3(v0: Vec3, m0: Vec3, v1: Vec3, m1: Vec3, t: f32) -> Vec3 {
    let t2 = t * t;
    let t3 = t2 * t;
    let h00 = 2.0 * t3 - 3.0 * t2 + 1.0;
    let h10 = t3 - 2.0 * t2 + t;
    let h01 = -2.0 * t3 + 3.0 * t2;
    let h11 = t3 - t2;
    v0 * h00 + m0 * h10 + v1 * h01 + m1 * h11
}

/// Cubic Hermite spline for quaternions.
///
/// Applies the same scalar coefficients as [`cubic_hermite_vec3`] to each
/// component of the quaternion, then normalizes the result. This is the
/// approach specified in glTF Appendix C for quaternion cubic spline channels.
///
/// `m0` and `m1` must already be scaled by `deltaTime`.
pub fn cubic_hermite_quat(q0: Quat, m0: Quat, q1: Quat, m1: Quat, t: f32) -> Quat {
    let t2 = t * t;
    let t3 = t2 * t;
    let h00 = 2.0 * t3 - 3.0 * t2 + 1.0;
    let h10 = t3 - 2.0 * t2 + t;
    let h01 = -2.0 * t3 + 3.0 * t2;
    let h11 = t3 - t2;
    // Treat each Quat as a Vec4 and apply coefficients component-wise.
    let result = q0 * h00 + m0 * h10 + q1 * h01 + m1 * h11;
    result.normalize()
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::f32::consts::FRAC_PI_2;

    fn approx_eq(a: f32, b: f32) {
        assert!((a - b).abs() < 1e-5, "left={a} right={b}");
    }

    fn approx_eq_vec3(a: Vec3, b: Vec3) {
        assert!(a.abs_diff_eq(b, 1e-5), "left={a:?} right={b:?}");
    }

    // ── find_keyframe_index ───────────────────────────────────────────────────

    #[test]
    fn find_keyframe_index_clamps_below() {
        let times = [1.0f32, 2.0, 3.0];
        let mut hint = 0;
        let (i, t) = find_keyframe_index(&times, 0.0, &mut hint);
        assert_eq!(i, 0);
        approx_eq(t, 0.0);
    }

    #[test]
    fn find_keyframe_index_clamps_above() {
        let times = [1.0f32, 2.0, 3.0];
        let mut hint = 0;
        let (i, t) = find_keyframe_index(&times, 5.0, &mut hint);
        assert_eq!(i, 1); // last - 1
        approx_eq(t, 1.0);
    }

    #[test]
    fn find_keyframe_index_midpoint() {
        let times = [0.0f32, 1.0, 2.0];
        let mut hint = 0;
        let (i, t) = find_keyframe_index(&times, 0.5, &mut hint);
        assert_eq!(i, 0);
        approx_eq(t, 0.5);
    }

    #[test]
    fn find_keyframe_index_uses_hint_fast_path() {
        let times = [0.0f32, 1.0, 2.0, 3.0];
        let mut hint = 1; // pre-set to interval [1, 2]
        let (i, t) = find_keyframe_index(&times, 1.5, &mut hint);
        assert_eq!(i, 1);
        approx_eq(t, 0.5);
        assert_eq!(hint, 1); // hint unchanged
    }

    #[test]
    fn find_keyframe_index_advances_hint_sequentially() {
        let times = [0.0f32, 1.0, 2.0, 3.0];
        let mut hint = 0;
        let (i0, _) = find_keyframe_index(&times, 0.5, &mut hint);
        assert_eq!(i0, 0);
        let (i1, _) = find_keyframe_index(&times, 1.5, &mut hint);
        assert_eq!(i1, 1);
        let (i2, _) = find_keyframe_index(&times, 2.5, &mut hint);
        assert_eq!(i2, 2);
    }

    #[test]
    fn find_keyframe_index_single_keyframe() {
        let times = [1.0f32];
        let mut hint = 0;
        let (i, t) = find_keyframe_index(&times, 0.5, &mut hint);
        assert_eq!(i, 0);
        approx_eq(t, 0.0);
    }

    #[test]
    fn find_keyframe_index_empty_slice() {
        let times: [f32; 0] = [];
        let mut hint = 0;
        let (i, t) = find_keyframe_index(&times, 0.5, &mut hint);
        assert_eq!(i, 0);
        approx_eq(t, 0.0);
    }

    // ── Step sampling ─────────────────────────────────────────────────────────

    #[test]
    fn step_returns_left_value() {
        let values = [Vec3::X, Vec3::Y, Vec3::Z];
        approx_eq_vec3(sample_vec3_step(&values, 1), Vec3::Y);
    }

    // ── Linear sampling ───────────────────────────────────────────────────────

    #[test]
    fn sample_vec3_linear_midpoint() {
        let values = [Vec3::ZERO, Vec3::new(2.0, 4.0, 6.0)];
        approx_eq_vec3(
            sample_vec3_linear(&values, 0, 0.5),
            Vec3::new(1.0, 2.0, 3.0),
        );
    }

    #[test]
    fn sample_vec3_linear_at_zero_returns_first() {
        let values = [Vec3::X, Vec3::Y];
        approx_eq_vec3(sample_vec3_linear(&values, 0, 0.0), Vec3::X);
    }

    #[test]
    fn sample_vec3_linear_at_one_returns_second() {
        let values = [Vec3::X, Vec3::Y];
        approx_eq_vec3(sample_vec3_linear(&values, 0, 1.0), Vec3::Y);
    }

    #[test]
    fn sample_quat_linear_identity_to_90deg() {
        let q0 = Quat::IDENTITY;
        let q1 = Quat::from_rotation_y(FRAC_PI_2);
        let values = [q0, q1];
        let mid = sample_quat_linear(&values, 0, 0.5);
        let expected = Quat::from_rotation_y(FRAC_PI_2 * 0.5);
        assert!(
            mid.abs_diff_eq(expected, 1e-5),
            "mid={mid:?} expected={expected:?}"
        );
    }

    // ── Cubic Hermite ─────────────────────────────────────────────────────────

    #[test]
    fn cubic_hermite_vec3_matches_endpoints() {
        let v0 = Vec3::new(1.0, 2.0, 3.0);
        let v1 = Vec3::new(4.0, 5.0, 6.0);
        let m0 = Vec3::ZERO;
        let m1 = Vec3::ZERO;
        approx_eq_vec3(cubic_hermite_vec3(v0, m0, v1, m1, 0.0), v0);
        approx_eq_vec3(cubic_hermite_vec3(v0, m0, v1, m1, 1.0), v1);
    }

    #[test]
    fn cubic_hermite_vec3_midpoint_zero_tangents() {
        let v0 = Vec3::ZERO;
        let v1 = Vec3::new(2.0, 0.0, 0.0);
        let m = Vec3::ZERO;
        let mid = cubic_hermite_vec3(v0, m, v1, m, 0.5);
        approx_eq_vec3(mid, Vec3::new(1.0, 0.0, 0.0));
    }
}
