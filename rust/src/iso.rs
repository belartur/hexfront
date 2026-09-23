//! Orthographic camera matching the game's isometric projection.
//!
//! The 2D [`Camera`] maps world `(x, y, z)` to screen pixels as
//! `sx = (x - y) * ISO_COS * zoom + w / 2 - cam.x * zoom` (and likewise for
//! `sy`). [`IsoCamera`] expresses the same transform as the matrix pair the
//! GPU shader consumes. macroquad's shader computes
//! `gl_Position = Projection * Model * vec4(position, 1)` with `Model`
//! identity and `Projection = Camera::matrix()`, and glam interprets the
//! arrays passed to `Mat4::from_cols_array_2d` as **columns** while
//! `Mat4 * Vec4` sums `m[i] * v[i]`. Both arrays below are therefore stored
//! in the row-vector layout (`out[j] = Σ_i m[i][j] * v[i]`), which is
//! exactly what the shader needs — see the `gpu_matrix_matches_2d_projection`
//! test, which reproduces that arithmetic.

use crate::camera::Camera;
use crate::constants;

/// View/projection pair reproducing the 2D camera on the GPU.
#[derive(Clone, Debug)]
pub struct IsoCamera {
    /// Row-vector view matrix, consumed as `Mat4::from_cols_array_2d` input.
    pub view: [[f32; 4]; 4],
    /// Row-vector orthographic projection matrix, same convention.
    pub proj: [[f32; 4]; 4],
    /// Near plane in ray-depth units (kept for debugging).
    #[allow(dead_code)]
    pub near: f32,
    /// Far plane in ray-depth units (kept for debugging).
    #[allow(dead_code)]
    pub far: f32,
}

impl IsoCamera {
    /// Build the GPU camera from the 2D `camera` and the board depth span.
    ///
    /// `d_min`/`d_max` bound the ray depth `D = (x + y) * ISO_SIN + z` of
    /// the terrain (see [`crate::mesh::depth_span`]). Camera space holds
    /// screen pixels (`cx = (x - y) * ISO_COS - cam.x`,
    /// `cy = (x + y) * ISO_SIN - z - cam.y`) plus `cd = D - d_max`; the
    /// projection scales pixels by `zoom` and maps `cd` onto `[0, 1]`, so
    /// the whole board lands inside the clip volume with the full depth
    /// range in use and larger `D` (closer) wins under `LessOrEqual`.
    pub fn from_camera(camera: &Camera, d_min: f64, d_max: f64) -> Self {
        let zoom = camera.zoom as f32;
        let (w, h) = camera.screen_size;
        let sx = 2.0 * zoom / w;
        let sy = -2.0 * zoom / h;
        let cos = constants::ISO_COS as f32;
        let sin = constants::ISO_SIN as f32;
        // Row-vector view matrix: row `j` of the array lists the
        // coefficients of the inputs for output component `j` (see the
        // module docs). Outputs: cx, cy, cd = D - d_max, w.
        let view = [
            [cos, sin, sin, 0.0],
            [-cos, sin, sin, 0.0],
            [0.0, -1.0, 1.0, 0.0],
            [-camera.x as f32, -camera.y as f32, -d_max as f32, 1.0],
        ];
        // Depth: cd lies in [-(d_max - d_min), 0] for terrain and slightly
        // above zero for objects standing on it, so the ortho range spans
        // the whole board (best linear depth precision) and a taller-than-
        // -terrain object still stays inside the clip volume.
        let near = 0.0f32;
        let far = (d_max - d_min).max(1.0) as f32;
        let proj = [
            [sx, 0.0, 0.0, 0.0],
            [0.0, sy, 0.0, 0.0],
            [0.0, 0.0, 1.0 / (near - far), 0.0],
            [0.0, 0.0, near / (near - far), 1.0],
        ];
        Self {
            view,
            proj,
            near,
            far,
        }
    }

    /// Apply the GPU pipeline by hand (tests only).
    ///
    /// Mirrors macroquad's shader `Projection * Model * vec4(position, 1)`
    /// with `Model` identity: glam loads each array row as a **column** and
    /// `Mat4 * Vec4` sums `m[i] * v[i]`, so the composed transform is
    /// `out[j] = Σ_i m[i][j] * v[i]`. Returns pixels and the clip depth.
    #[cfg(test)]
    pub fn project_point(&self, camera: &Camera, x: f64, y: f64, z: f64) -> (f32, f32, f32) {
        let full = mat_mul(&self.proj, &self.view);
        let n = mat_mul_vec(&full, [x as f32, y as f32, z as f32, 1.0]);
        let (w, h) = camera.screen_size;
        let sx = (n[0] / n[3] * 0.5 + 0.5) * w;
        let sy = (0.5 - n[1] / n[3] * 0.5) * h;
        (sx, sy, n[2] / n[3])
    }
}

/// glam `Mat4 * Vec4`: `m.x_axis * v.x + m.y_axis * v.y + ...` where each
/// array row is one axis (column) — mirrors the GPU exactly.
#[cfg(test)]
fn mat_mul_vec(m: &[[f32; 4]; 4], v: [f32; 4]) -> [f32; 4] {
    let mut out = [0.0f32; 4];
    for (i, row) in m.iter().enumerate() {
        for (j, value) in row.iter().enumerate() {
            out[j] += value * v[i];
        }
    }
    out
}

/// glam `Mat4 * Mat4`, following the same axis convention as
/// [`mat_mul_vec`]: `(a * b)[i] = a * b[i]`.
#[cfg(test)]
fn mat_mul(a: &[[f32; 4]; 4], b: &[[f32; 4]; 4]) -> [[f32; 4]; 4] {
    let mut out = [[0.0f32; 4]; 4];
    for (i, col) in b.iter().enumerate() {
        out[i] = mat_mul_vec(a, *col);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gpu_camera_matches_2d_projection() {
        let camera = Camera::new((1180.0, 720.0));
        let iso = IsoCamera::from_camera(&camera, 0.0, 20000.0);
        for (x, y, z) in [
            (0.0, 0.0, 0.0),
            (500.0, 200.0, 36.0),
            (300.0, 800.0, 135.0),
            (3000.0, 1500.0, 9.0),
            (11772.0, 11772.0, 90.0),
        ] {
            let (ex, ey) = camera.world_to_screen(x, y, z);
            let (sx, sy, _) = iso.project_point(&camera, x, y, z);
            assert!((sx - ex).abs() < 0.5, "x at ({x},{y},{z}): {sx} vs {ex}");
            assert!((sy - ey).abs() < 0.5, "y at ({x},{y},{z}): {sy} vs {ey}");
        }
    }

    #[test]
    fn depth_orders_by_ray_depth() {
        let mut camera = Camera::new((800.0, 600.0));
        camera.x = 100.0;
        camera.y = 50.0;
        let iso = IsoCamera::from_camera(&camera, 0.0, 4000.0);
        let (_, _, low) = iso.project_point(&camera, 100.0, 100.0, 0.0);
        let (_, _, high) = iso.project_point(&camera, 100.0, 100.0, 90.0);
        assert!(high < low, "higher tile must win: {high} vs {low}");
        // Terrain depth stays inside the clip volume (no far/near clipping).
        for d in [0.0, 1000.0, 2000.0, 3999.0] {
            let (_, _, clip) = iso.project_point(&camera, d, d, 0.0);
            assert!((-1.0..=1.0).contains(&clip), "depth {d} clipped: {clip}");
        }
    }

    #[test]
    fn pan_and_zoom_match_2d_projection() {
        let mut camera = Camera::new((1180.0, 720.0));
        camera.x = 300.0;
        camera.y = -150.0;
        camera.zoom = 1.7;
        let iso = IsoCamera::from_camera(&camera, 0.0, 20000.0);
        for (x, y, z) in [
            (0.0, 0.0, 0.0),
            (900.0, 400.0, 45.0),
            (2500.0, 1800.0, 9.0),
        ] {
            let (ex, ey) = camera.world_to_screen(x, y, z);
            let (sx, sy, _) = iso.project_point(&camera, x, y, z);
            assert!((sx - ex).abs() < 0.5, "x at ({x},{y},{z}): {sx} vs {ex}");
            assert!((sy - ey).abs() < 0.5, "y at ({x},{y},{z}): {sy} vs {ey}");
        }
    }

    #[test]
    fn coplanar_fragments_keep_equal_depth() {
        // The same world point projected twice must give bit-identical clip
        // depth, otherwise `LessOrEqual` would drop coplanar grid strokes.
        let camera = Camera::new((1180.0, 720.0));
        let iso = IsoCamera::from_camera(&camera, 0.0, 12000.0);
        let (_, _, a) = iso.project_point(&camera, 777.0, 333.0, 45.0);
        let (_, _, b) = iso.project_point(&camera, 777.0, 333.0, 45.0);
        assert_eq!(a, b);
    }
}
