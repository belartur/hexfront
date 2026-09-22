//! Camera and the isometric world<->screen projection.
//!
//! The projection maps world (x, y, z) to the screen:
//!
//! ```text
//! px = (x - y) * ISO_COS
//! py = (x + y) * ISO_SIN - z
//! ```
//!
//! where z is the rendered elevation (tile height * ELEVATION_PX). The
//! camera stores a pan offset in projected space and a zoom factor; zooming
//! affects rendering only, never simulation distances.

use crate::constants;
use crate::hexgrid::SQRT3;

/// Clamp `value` into [low, high].
pub fn clamp(value: f64, low: f64, high: f64) -> f64 {
    value.max(low).min(high)
}

/// View position and zoom of the isometric viewport.
#[derive(Clone, Debug)]
pub struct Camera {
    /// Pan offset in projected space.
    pub x: f64,
    /// Pan offset in projected space.
    pub y: f64,
    /// Zoom factor (rendering only).
    pub zoom: f64,
    /// Screen size in pixels (w, h).
    pub screen_size: (f32, f32),
    /// World-space bounds `(x_min, x_max, y_min, y_max)` the view centre
    /// may not leave, or `None` for unlimited panning.
    pub bounds: Option<(f64, f64, f64, f64)>,
}

impl Camera {
    /// Create a camera for a screen of `screen_size`.
    pub fn new(screen_size: (f32, f32)) -> Self {
        Self {
            x: 0.0,
            y: 0.0,
            zoom: 1.0,
            screen_size,
            bounds: None,
        }
    }
    /// Project a world point to screen coordinates.
    pub fn world_to_screen(&self, wx: f64, wy: f64, wz: f64) -> (f32, f32) {
        let px = (wx - wy) * constants::ISO_COS;
        let py = (wx + wy) * constants::ISO_SIN - wz;
        (
            ((px - self.x) * self.zoom + self.screen_size.0 as f64 / 2.0) as f32,
            ((py - self.y) * self.zoom + self.screen_size.1 as f64 / 2.0) as f32,
        )
    }
    /// Inverse of [`Camera::world_to_screen`] for a known elevation `wz`.
    pub fn screen_to_world(&self, sx: f32, sy: f32, wz: f64) -> (f64, f64) {
        let px = (sx as f64 - self.screen_size.0 as f64 / 2.0) / self.zoom + self.x;
        let py = (sy as f64 - self.screen_size.1 as f64 / 2.0) / self.zoom + self.y + wz;
        let wx = (px / constants::ISO_COS + py / constants::ISO_SIN) / 2.0;
        let wy = (py / constants::ISO_SIN - px / constants::ISO_COS) / 2.0;
        (wx, wy)
    }
    /// Pan the view by a screen-space delta (already zoom-corrected).
    pub fn pan(&mut self, dx: f32, dy: f32) {
        self.x -= dx as f64 / self.zoom;
        self.y -= dy as f64 / self.zoom;
        self.clamp_to_bounds();
    }
    #[allow(dead_code)]
    /// Pan the view directly in projected space.
    pub fn pan_projected(&mut self, dx: f64, dy: f64) {
        self.x += dx;
        self.y += dy;
        self.clamp_to_bounds();
    }
    /// Multiplicative zoom keeping the world point under the cursor.
    pub fn zoom_at(&mut self, factor: f64, sx: f32, sy: f32) {
        let (wx, wy) = self.screen_to_world(sx, sy, 0.0);
        self.zoom = clamp(self.zoom * factor, constants::ZOOM_MIN, constants::ZOOM_MAX);
        let px = (wx - wy) * constants::ISO_COS;
        let py = (wx + wy) * constants::ISO_SIN;
        self.x = px - (sx as f64 - self.screen_size.0 as f64 / 2.0) / self.zoom;
        self.y = py - (sy as f64 - self.screen_size.1 as f64 / 2.0) / self.zoom;
        self.clamp_to_bounds();
    }
    /// Center the view on a world point.
    pub fn center_on_world(&mut self, wx: f64, wy: f64, wz: f64) {
        self.x = (wx - wy) * constants::ISO_COS;
        self.y = (wx + wy) * constants::ISO_SIN - wz;
        self.clamp_to_bounds();
    }
    /// Constrain the view centre to the area of `board`.
    pub fn limit_to_board(&mut self, board: &crate::board::Board) {
        let x_max = 1.5 * board.side * (board.cols - 1) as f64;
        let y_max = SQRT3 * board.side * (board.rows - 1) as f64
            + SQRT3 * board.side * 0.5 * (((board.cols - 1) & 1) as f64);
        self.bounds = Some((0.0, x_max, 0.0, y_max));
    }
    fn clamp_to_bounds(&mut self) {
        let Some((x_min, x_max, y_min, y_max)) = self.bounds else {
            return;
        };
        let mut wx = (self.x / constants::ISO_COS + self.y / constants::ISO_SIN) / 2.0;
        let mut wy = (self.y / constants::ISO_SIN - self.x / constants::ISO_COS) / 2.0;
        wx = clamp(wx, x_min, x_max);
        wy = clamp(wy, y_min, y_max);
        self.x = (wx - wy) * constants::ISO_COS;
        self.y = (wx + wy) * constants::ISO_SIN;
    }
    #[allow(dead_code)]
    /// Project a ground-plane circle as a polygon (an iso ellipse).
    pub fn screen_circle_poly(
        &self,
        cx: f64,
        cy: f64,
        radius: f64,
        wz: f64,
        n: usize,
    ) -> Vec<(f32, f32)> {
        let mut pts = Vec::with_capacity(n);
        for i in 0..n {
            let a = 2.0 * std::f64::consts::PI * i as f64 / n as f64;
            pts.push(self.world_to_screen(cx + radius * a.cos(), cy + radius * a.sin(), wz));
        }
        pts
    }
}
