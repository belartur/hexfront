//! Orthographic camera matching the game's isometric projection.
//!
//! The 2D [`Camera`] maps world `(x, y, z)` to screen pixels as
//! `sx = (x - y) * ISO_COS * zoom + w / 2 - cam.x * zoom` (and likewise for
//! `sy`). [`IsoCamera`] expresses the same transform as a view/projection
//! matrix pair for the GPU path: an orthographic camera looking along the
//! ray-depth axis `D = (x + y) * ISO_SIN + z`, so the hardware depth buffer
//! resolves occlusion like the old software buffer did (larger `D` wins,
//! ties keep draw order via `LessOrEqual`).

use crate::camera::Camera;
use crate::constants;

/// View/projection pair reproducing the 2D camera on the GPU.
#[derive(Clone, Debug)]
pub struct IsoCamera {
    /// Row-major 4x4 view matrix (row vectors: `v * view`).
    pub view: [[f32; 4]; 4],
    /// Row-major 4x4 orthographic projection matrix.
    pub proj: [[f32; 4]; 4],
    /// Near plane used for the projection (kept for debugging).
    #[allow(dead_code)]
    pub near: f32,
    /// Far plane used for the projection (kept for debugging).
    #[allow(dead_code)]
    pub far: f32,
}

impl IsoCamera {
    /// Build the GPU camera from the 2D `camera`.
    ///
    /// Camera space keeps screen axes in pixels: `cx` holds
    /// `(x - y) * ISO_COS` relative to the pan offset, `cy` holds
    /// `(x + y) * ISO_SIN - z`, and `cd` holds `d_max - D` so closer
    /// fragments (larger `D`) get smaller depth values for `LessOrEqual`.
    pub fn from_camera(camera: &Camera, d_max: f64) -> Self {
        let zoom = camera.zoom as f32;
        let (w, h) = camera.screen_size;
        let sx = 2.0 * zoom / w;
        let sy = -2.0 * zoom / h;
        // Fixed depth scale: cd = d_max - D lands in [-span, +span] around
        // zero; near/far wrap the whole scene symmetrically.
        let s = 1.0f32 / 20000.0;
        let (near, far) = (-2.0, 2.0);
        let cos = constants::ISO_COS as f32;
        let sin = constants::ISO_SIN as f32;
        // Rows of the view matrix (row-vector convention).
        // Camera space: cx = (x - y) * COS - cam.x, cy = (x + y) * SIN
        // - z - cam.y, cd = D - d_max with D = (x + y) * SIN + z, so
        // closer fragments (larger D) get larger cd and the mirrored
        // projection below turns that into smaller clip depth.
        let view = [
            [cos, sin, sin * s, 0.0],
            [-cos, sin, sin * s, 0.0],
            [0.0, -1.0, s, 0.0],
            [-camera.x as f32, -camera.y as f32, -s * d_max as f32, 1.0],
        ];
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

    /// Apply the matrix path by hand (tests only).
    #[cfg(test)]
    pub fn project_point(&self, camera: &Camera, x: f64, y: f64, z: f64) -> (f32, f32, f32) {
        let v = [x as f32, y as f32, z as f32, 1.0];
        let mut c = [0.0f32; 4];
        for (col, c_col) in c.iter_mut().enumerate() {
            *c_col = v[0] * self.view[0][col]
                + v[1] * self.view[1][col]
                + v[2] * self.view[2][col]
                + v[3] * self.view[3][col];
        }
        let mut n = [0.0f32; 4];
        for (col, n_col) in n.iter_mut().enumerate() {
            *n_col = c[0] * self.proj[0][col]
                + c[1] * self.proj[1][col]
                + c[2] * self.proj[2][col]
                + c[3] * self.proj[3][col];
        }
        let (w, h) = camera.screen_size;
        let sx = (n[0] / n[3] * 0.5 + 0.5) * w;
        let sy = (0.5 - n[1] / n[3] * 0.5) * h;
        (sx, sy, n[2] / n[3])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gpu_camera_matches_2d_projection() {
        let camera = Camera::new((1180.0, 720.0));
        let iso = IsoCamera::from_camera(&camera, 20000.0);
        for (x, y, z) in [
            (0.0, 0.0, 0.0),
            (500.0, 200.0, 36.0),
            (-300.0, 800.0, 135.0),
            (3000.0, 1500.0, 9.0),
        ] {
            let (ex, ey) = camera.world_to_screen(x, y, z);
            let (sx, sy, _) = iso.project_point(&camera, x, y, z);
            assert!((sx - ex).abs() < 1.0, "x at ({x},{y},{z}): {sx} vs {ex}");
            assert!((sy - ey).abs() < 1.0, "y at ({x},{y},{z}): {sy} vs {ey}");
        }
    }

    #[test]
    fn depth_orders_by_ray_depth() {
        let mut camera = Camera::new((800.0, 600.0));
        camera.x = 100.0;
        camera.y = 50.0;
        let iso = IsoCamera::from_camera(&camera, 20000.0);
        let (_, _, low) = iso.project_point(&camera, 100.0, 100.0, 0.0);
        let (_, _, high) = iso.project_point(&camera, 100.0, 100.0, 90.0);
        assert!(high < low, "higher tile must win: {high} vs {low}");
    }

    #[test]
    fn pan_and_zoom_match_2d_projection() {
        let mut camera = Camera::new((1180.0, 720.0));
        camera.x = 300.0;
        camera.y = -150.0;
        camera.zoom = 1.7;
        let iso = IsoCamera::from_camera(&camera, 20000.0);
        for (x, y, z) in [
            (0.0, 0.0, 0.0),
            (900.0, -400.0, 45.0),
            (2500.0, 1800.0, 9.0),
        ] {
            let (ex, ey) = camera.world_to_screen(x, y, z);
            let (sx, sy, _) = iso.project_point(&camera, x, y, z);
            assert!((sx - ex).abs() < 1.0, "x at ({x},{y},{z}): {sx} vs {ex}");
            assert!((sy - ey).abs() < 1.0, "y at ({x},{y},{z}): {sy} vs {ey}");
        }
    }
}
