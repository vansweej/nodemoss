//! Math primitives for the rig framework.

pub use glam;
pub use glam::{Mat3, Mat4, Quat, Vec2, Vec3, Vec4};

/// Decomposed transform used for scene authoring.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Transform {
    pub translation: Vec3,
    pub rotation: Quat,
    pub scale: Vec3,
}

impl Transform {
    pub const IDENTITY: Self = Self {
        translation: Vec3::ZERO,
        rotation: Quat::IDENTITY,
        scale: Vec3::ONE,
    };

    pub fn to_mat4(self) -> Mat4 {
        Mat4::from_scale_rotation_translation(self.scale, self.rotation, self.translation)
    }
}

impl Default for Transform {
    fn default() -> Self {
        Self::IDENTITY
    }
}

/// Simple bounding sphere used for local and world bounds.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BoundingSphere {
    pub center: Vec3,
    pub radius: f32,
}

impl BoundingSphere {
    pub const ZERO: Self = Self {
        center: Vec3::ZERO,
        radius: 0.0,
    };

    pub fn transform_by(self, world: Mat4) -> Self {
        let center = world.transform_point3(self.center);
        let axis_x = world.transform_vector3(Vec3::X * self.radius).length();
        let axis_y = world.transform_vector3(Vec3::Y * self.radius).length();
        let axis_z = world.transform_vector3(Vec3::Z * self.radius).length();
        let radius = axis_x.max(axis_y).max(axis_z);

        Self { center, radius }
    }

    pub fn union(self, other: Self) -> Self {
        if self.radius <= 0.0 {
            return other;
        }
        if other.radius <= 0.0 {
            return self;
        }

        let offset = other.center - self.center;
        let distance = offset.length();

        if self.radius >= distance + other.radius {
            return self;
        }
        if other.radius >= distance + self.radius {
            return other;
        }

        let direction = if distance > 0.0 {
            offset / distance
        } else {
            Vec3::X
        };
        let min = self.center - direction * self.radius;
        let max = other.center + direction * other.radius;
        let center = (min + max) * 0.5;
        let radius = center.distance(max);

        Self { center, radius }
    }

    /// Returns `true` when the sphere lies entirely on the negative side of
    /// `plane` (i.e. the sphere is outside the half-space).
    ///
    /// `plane` is a `Vec4` where `.xyz` is the (already normalised) outward
    /// normal and `.w` is the plane distance so that the signed distance of a
    /// point `p` from the plane is `dot(p, plane.xyz) + plane.w`.
    pub fn is_outside_plane(self, plane: Vec4) -> bool {
        let signed_dist = self.center.dot(plane.truncate()) + plane.w;
        signed_dist < -self.radius
    }

    /// Returns `true` when the sphere is outside **any** of the six frustum
    /// planes, meaning it can be safely culled.
    pub fn is_outside_frustum(self, planes: &[Vec4; 6]) -> bool {
        planes.iter().any(|&p| self.is_outside_plane(p))
    }
}

impl Default for BoundingSphere {
    fn default() -> Self {
        Self::ZERO
    }
}

/// Supported projection models for camera components.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Projection {
    Perspective {
        fov_y_radians: f32,
        near: f32,
        far: f32,
    },
}

impl Projection {
    pub fn matrix(self, aspect: f32) -> Mat4 {
        match self {
            Projection::Perspective {
                fov_y_radians,
                near,
                far,
            } => Mat4::perspective_rh(fov_y_radians, aspect, near, far),
        }
    }
}

/// Lightweight camera value built from a pose and projection.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Camera {
    pub pose: Transform,
    pub projection: Projection,
}

impl Camera {
    pub fn view_matrix(self) -> Mat4 {
        self.pose.to_mat4().inverse()
    }

    pub fn projection_matrix(self, aspect: f32) -> Mat4 {
        self.projection.matrix(aspect)
    }

    pub fn projection_view_matrix(self, aspect: f32) -> Mat4 {
        self.projection_matrix(aspect) * self.view_matrix()
    }
}

/// Simple picking ray placeholder.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Ray {
    pub origin: Vec3,
    pub direction: Vec3,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn approx_eq_vec3(left: Vec3, right: Vec3) {
        assert!(
            left.abs_diff_eq(right, 1e-5),
            "left={left:?} right={right:?}"
        );
    }

    fn approx_eq_mat4(left: Mat4, right: Mat4) {
        for (lhs, rhs) in left.to_cols_array().into_iter().zip(right.to_cols_array()) {
            assert!((lhs - rhs).abs() <= 1e-5, "lhs={lhs} rhs={rhs}");
        }
    }

    #[test]
    fn transform_identity_is_default() {
        assert_eq!(Transform::default(), Transform::IDENTITY);
    }

    #[test]
    fn transform_to_mat4_matches_glam_builder() {
        let transform = Transform {
            translation: Vec3::new(1.0, 2.0, 3.0),
            rotation: Quat::from_rotation_y(0.5),
            scale: Vec3::new(2.0, 3.0, 4.0),
        };

        let actual = transform.to_mat4();
        let expected = Mat4::from_scale_rotation_translation(
            transform.scale,
            transform.rotation,
            transform.translation,
        );

        approx_eq_mat4(actual, expected);
    }

    #[test]
    fn zero_bounding_sphere_is_default() {
        assert_eq!(BoundingSphere::default(), BoundingSphere::ZERO);
    }

    #[test]
    fn bounding_sphere_transform_moves_center_and_scales_radius() {
        let sphere = BoundingSphere {
            center: Vec3::new(1.0, 0.0, -1.0),
            radius: 2.0,
        };
        let world = Mat4::from_scale_rotation_translation(
            Vec3::new(3.0, 4.0, 5.0),
            Quat::from_rotation_z(0.25),
            Vec3::new(2.0, -3.0, 4.0),
        );

        let transformed = sphere.transform_by(world);

        approx_eq_vec3(transformed.center, world.transform_point3(sphere.center));
        assert!((transformed.radius - 10.0).abs() <= 1e-5);
    }

    #[test]
    fn union_returns_other_when_self_is_zero() {
        let other = BoundingSphere {
            center: Vec3::new(1.0, 2.0, 3.0),
            radius: 4.0,
        };

        assert_eq!(BoundingSphere::ZERO.union(other), other);
    }

    #[test]
    fn union_returns_self_when_other_is_zero() {
        let sphere = BoundingSphere {
            center: Vec3::new(-1.0, 0.5, 2.0),
            radius: 3.0,
        };

        assert_eq!(sphere.union(BoundingSphere::ZERO), sphere);
    }

    #[test]
    fn union_returns_containing_sphere_when_self_contains_other() {
        let outer = BoundingSphere {
            center: Vec3::ZERO,
            radius: 10.0,
        };
        let inner = BoundingSphere {
            center: Vec3::new(1.0, 0.0, 0.0),
            radius: 2.0,
        };

        assert_eq!(outer.union(inner), outer);
    }

    #[test]
    fn union_returns_containing_sphere_when_other_contains_self() {
        let inner = BoundingSphere {
            center: Vec3::new(1.0, 0.0, 0.0),
            radius: 2.0,
        };
        let outer = BoundingSphere {
            center: Vec3::ZERO,
            radius: 10.0,
        };

        assert_eq!(inner.union(outer), outer);
    }

    #[test]
    fn union_expands_to_cover_disjoint_spheres() {
        let left = BoundingSphere {
            center: Vec3::new(-2.0, 0.0, 0.0),
            radius: 1.0,
        };
        let right = BoundingSphere {
            center: Vec3::new(4.0, 0.0, 0.0),
            radius: 2.0,
        };

        let merged = left.union(right);

        approx_eq_vec3(merged.center, Vec3::new(1.5, 0.0, 0.0));
        assert!((merged.radius - 4.5).abs() <= 1e-5);
    }

    #[test]
    fn projection_matrix_matches_glam_perspective() {
        let projection = Projection::Perspective {
            fov_y_radians: 60.0_f32.to_radians(),
            near: 0.1,
            far: 10.0,
        };

        let actual = projection.matrix(16.0 / 9.0);
        let expected = Mat4::perspective_rh(60.0_f32.to_radians(), 16.0 / 9.0, 0.1, 10.0);

        approx_eq_mat4(actual, expected);
    }

    #[test]
    fn camera_view_matrix_inverts_pose() {
        let pose = Transform {
            translation: Vec3::new(0.0, 0.0, 5.0),
            rotation: Quat::from_rotation_y(0.25),
            scale: Vec3::ONE,
        };
        let camera = Camera {
            pose,
            projection: Projection::Perspective {
                fov_y_radians: 1.0,
                near: 0.1,
                far: 100.0,
            },
        };

        let actual = camera.view_matrix();
        let expected = pose.to_mat4().inverse();

        approx_eq_mat4(actual, expected);
    }

    #[test]
    fn camera_projection_view_is_projection_times_view() {
        let camera = Camera {
            pose: Transform {
                translation: Vec3::new(1.0, 2.0, 3.0),
                rotation: Quat::from_rotation_x(0.5),
                scale: Vec3::ONE,
            },
            projection: Projection::Perspective {
                fov_y_radians: 0.9,
                near: 0.1,
                far: 25.0,
            },
        };

        let actual = camera.projection_view_matrix(4.0 / 3.0);
        let expected = camera.projection_matrix(4.0 / 3.0) * camera.view_matrix();

        approx_eq_mat4(actual, expected);
    }

    /// A plane with normal +Z at distance 0 (the XY-plane, positive side is +Z).
    fn z_plane(dist: f32) -> Vec4 {
        Vec4::new(0.0, 0.0, 1.0, -dist)
    }

    #[test]
    fn sphere_outside_plane_when_entirely_on_negative_side() {
        // Sphere at z = -5, radius 1. Plane is z=0 (normal +Z, w=0).
        // Signed dist = -5 + 0 = -5, which is < -radius (-1), so outside.
        let sphere = BoundingSphere { center: Vec3::new(0.0, 0.0, -5.0), radius: 1.0 };
        assert!(sphere.is_outside_plane(z_plane(0.0)));
    }

    #[test]
    fn sphere_inside_plane_returns_false() {
        // Sphere at z = +5, radius 1. Signed dist = 5 > -1, so inside.
        let sphere = BoundingSphere { center: Vec3::new(0.0, 0.0, 5.0), radius: 1.0 };
        assert!(!sphere.is_outside_plane(z_plane(0.0)));
    }

    #[test]
    fn sphere_straddling_plane_returns_false() {
        // Sphere at z = 0, radius 2. Signed dist = 0, which is NOT < -2, so straddles.
        let sphere = BoundingSphere { center: Vec3::ZERO, radius: 2.0 };
        assert!(!sphere.is_outside_plane(z_plane(0.0)));
    }

    #[test]
    fn sphere_outside_frustum_when_outside_any_plane() {
        // Build a trivial "frustum" of 6 planes all with normal +X at x=0.
        // Sphere at x=-5 radius 1 is outside all of them.
        let planes = [Vec4::new(1.0, 0.0, 0.0, 0.0); 6];
        let sphere = BoundingSphere { center: Vec3::new(-5.0, 0.0, 0.0), radius: 1.0 };
        assert!(sphere.is_outside_frustum(&planes));
    }

    #[test]
    fn sphere_inside_frustum_when_inside_all_planes() {
        // Box [-1,1]^3. Each plane: signed dist = dot(p, normal) + w.
        // For "x >= -1": normal = +X, w = 1  → dist at origin = 1 (inside).
        // For "x <=  1": normal = -X, w = 1  → dist at origin = 1 (inside).
        let planes = [
            Vec4::new( 1.0,  0.0,  0.0,  1.0),  // x >= -1
            Vec4::new(-1.0,  0.0,  0.0,  1.0),  // x <=  1
            Vec4::new( 0.0,  1.0,  0.0,  1.0),  // y >= -1
            Vec4::new( 0.0, -1.0,  0.0,  1.0),  // y <=  1
            Vec4::new( 0.0,  0.0,  1.0,  1.0),  // z >= -1
            Vec4::new( 0.0,  0.0, -1.0,  1.0),  // z <=  1
        ];
        let sphere = BoundingSphere { center: Vec3::ZERO, radius: 0.5 };
        assert!(!sphere.is_outside_frustum(&planes));
    }
}
