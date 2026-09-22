//! Software depth-tested primitives for the shared game renderer.
//!
//! Convex faces use pixel-centre coverage. Surface-bound strokes sample
//! their supporting depth plane. Larger depth wins; coplanar ties are won
//! by the later pass (fixed pass order, as in the visual contract).

use crate::camera::Camera;
use crate::constants;

/// Integer screen coordinates with unrounded projection and ray depth.
#[derive(Clone, Copy, Debug)]
pub struct ProjectedPoint {
    /// Rounded screen position.
    pub sx: i32,
    /// Rounded screen position.
    pub sy: i32,
    /// Unrounded projected position.
    pub projected: (f64, f64),
    /// Ray depth D = (x + y) * k + z.
    pub depth: f64,
    /// Supporting ground plane (slope, intercept) for horizontal details.
    pub ground_plane: Option<(f64, f64)>,
}

impl ProjectedPoint {
    /// Create a projected point.
    pub fn new(sx: f64, sy: f64, depth: f64) -> Self {
        Self {
            sx: sx.round() as i32,
            sy: sy.round() as i32,
            projected: (sx, sy),
            depth,
            ground_plane: None,
        }
    }
}

/// Projection adapter retaining depth without changing the UI camera.
#[derive(Clone, Debug)]
pub struct DepthCamera {
    /// Underlying view camera.
    pub camera: Camera,
}

impl DepthCamera {
    /// Wrap `camera` with depth retention.
    pub fn new(camera: Camera) -> Self {
        Self { camera }
    }
    /// Project a world vertex and retain its affine ray coordinate.
    pub fn world_to_screen(&self, x: f64, y: f64, z: f64) -> ProjectedPoint {
        let c = &self.camera;
        let sx = (x - y) * constants::ISO_COS - c.x;
        let sx = sx * c.zoom + c.screen_size.0 as f64 / 2.0;
        let sy = (x + y) * constants::ISO_SIN - z - c.y;
        let sy = sy * c.zoom + c.screen_size.1 as f64 / 2.0;
        let mut p = ProjectedPoint::new(sx, sy, (x + y) * constants::ISO_SIN + z);
        // Horizontal details belong to this plane across their width.
        p.ground_plane = Some((
            1.0 / c.zoom,
            c.y - c.screen_size.1 as f64 / (2.0 * c.zoom) + 2.0 * z,
        ));
        p
    }
    /// Retain depth on every vertex of a projected ground circle.
    pub fn screen_circle_poly(
        &self,
        cx: f64,
        cy: f64,
        radius: f64,
        wz: f64,
        n: usize,
    ) -> Vec<ProjectedPoint> {
        let mut pts = Vec::with_capacity(n);
        for i in 0..n {
            let a = 2.0 * std::f64::consts::PI * i as f64 / n as f64;
            pts.push(self.world_to_screen(cx + radius * a.cos(), cy + radius * a.sin(), wz));
        }
        pts
    }
}

/// Frame-local colour/depth target; coplanar details win exact ties.
#[derive(Clone, Debug)]
pub struct DepthBuffer {
    /// Screen width in pixels.
    pub w: usize,
    /// Screen height in pixels.
    pub h: usize,
    /// RGB image buffer.
    pub color: Vec<u8>,
    /// Per-pixel depth (larger = closer).
    pub depth: Vec<f64>,
    decal: bool,
}

impl DepthBuffer {
    /// Create a `w` x `h` buffer filled with `clear` at depth `-inf`.
    pub fn new(w: usize, h: usize, clear: [u8; 3]) -> Self {
        let mut color = vec![0u8; w * h * 3];
        for px in color.chunks_mut(3) {
            px[0] = clear[0];
            px[1] = clear[1];
            px[2] = clear[2];
        }
        Self {
            w,
            h,
            color,
            depth: vec![f64::NEG_INFINITY; w * h],
            decal: false,
        }
    }
    /// Clear the buffer with `clear` and reset depths.
    pub fn clear(&mut self, clear: [u8; 3]) {
        for px in self.color.chunks_mut(3) {
            px[0] = clear[0];
            px[1] = clear[1];
            px[2] = clear[2];
        }
        for d in self.depth.iter_mut() {
            *d = f64::NEG_INFINITY;
        }
    }
    /// Blend a planar shadow only onto equal-depth visible receivers.
    pub fn decal(&mut self, color: [u8; 4], points: &[ProjectedPoint]) {
        self.decal = true;
        self.polygon_blend(color, points, 0);
        self.decal = false;
    }
    fn paint_pixel(&mut self, x: i32, y: i32, color: [u8; 3], depth: f64) {
        if x < 0 || y < 0 || x >= self.w as i32 || y >= self.h as i32 {
            return;
        }
        let i = y as usize * self.w + x as usize;
        if depth >= self.depth[i] - constants::DEPTH_EPSILON {
            if self.decal {
                // A translucent decal may shade only its receiving surface.
                if (depth - self.depth[i]).abs() > 1e-4 {
                    return;
                }
                let a = 70.0 / 255.0;
                let dst = &mut self.color[i * 3..i * 3 + 3];
                for c in 0..3 {
                    dst[c] = ((dst[c] as f64) * (1.0 - a) + color[c] as f64 * a).round() as u8;
                }
                return;
            }
            self.depth[i] = depth;
            self.color[i * 3] = color[0];
            self.color[i * 3 + 1] = color[1];
            self.color[i * 3 + 2] = color[2];
        }
    }
    fn paint_pixel_blend(&mut self, x: i32, y: i32, color: [u8; 4], depth: f64) {
        if x < 0 || y < 0 || x >= self.w as i32 || y >= self.h as i32 {
            return;
        }
        let i = y as usize * self.w + x as usize;
        // Translucent fills: visible when not strictly behind.
        if depth + 1e-4 < self.depth[i] {
            return;
        }
        let a = color[3] as f64 / 255.0;
        let dst = &mut self.color[i * 3..i * 3 + 3];
        for c in 0..3 {
            dst[c] = ((dst[c] as f64) * (1.0 - a) + color[c] as f64 * a).round() as u8;
        }
    }
    fn plane_of(&self, points: &[ProjectedPoint]) -> Option<(f64, f64, f64)> {
        // Fit depth = origin.depth + dx*(x-ox) + dy*(y-oy) from the
        // best-conditioned triangle, including edge-on faces.
        if points.len() < 3 {
            return None;
        }
        let origin = &points[0];
        let mut best: Option<(f64, f64, f64, f64, f64, f64, f64)> = None;
        for w in points[1..].windows(2) {
            let (a, b) = (&w[0], &w[1]);
            let ax = a.projected.0 - origin.projected.0;
            let ay = a.projected.1 - origin.projected.1;
            let bx = b.projected.0 - origin.projected.0;
            let by = b.projected.1 - origin.projected.1;
            let area = (ax * by - ay * bx).abs();
            if best.is_none() || area > best.unwrap().0 {
                best = Some((
                    area,
                    ax,
                    ay,
                    bx,
                    by,
                    a.depth - origin.depth,
                    b.depth - origin.depth,
                ));
            }
        }
        let (area, ax, ay, bx, by, da, db) = best?;
        if area <= constants::DEPTH_EPSILON {
            return None;
        }
        let det = ax * by - ay * bx;
        if det.abs() <= 1e-12 {
            return None;
        }
        let dx = (da * by - db * ay) / det;
        let dy = (ax * db - bx * da) / det;
        Some((origin.depth, dx, dy))
    }
    fn inside_convex(px: f64, py: f64, points: &[ProjectedPoint]) -> bool {
        let mut pos = true;
        let mut neg = true;
        for i in 0..points.len() {
            let a = &points[i];
            let b = &points[(i + 1) % points.len()];
            let cross = (b.projected.0 - a.projected.0) * (py - a.projected.1)
                - (b.projected.1 - a.projected.1) * (px - a.projected.0);
            if cross < -constants::DEPTH_EPSILON {
                pos = false;
            }
            if cross > constants::DEPTH_EPSILON {
                neg = false;
            }
            if !pos && !neg {
                return false;
            }
        }
        true
    }
    /// Draw a planar convex polygon or its depth-tested outline.
    pub fn polygon(&mut self, color: [u8; 3], points: &[ProjectedPoint], width: i32) {
        if points.len() < 3 {
            return;
        }
        if width > 0 {
            // Outline: stroke each edge as a depth-tested line.
            for i in 0..points.len() {
                let a = &points[i];
                let b = &points[(i + 1) % points.len()];
                self.line(color, a, b, width);
            }
            return;
        }
        let (d0, dx, dy) = match self.plane_of(points) {
            Some(p) => p,
            None => return,
        };
        let origin = &points[0];
        let mut xs = Vec::new();
        let mut ys = Vec::new();
        for p in points.iter() {
            xs.push(p.projected.0);
            ys.push(p.projected.1);
        }
        let (x0, x1) = (
            xs.iter().cloned().fold(f64::INFINITY, f64::min).floor() as i32,
            xs.iter().cloned().fold(f64::NEG_INFINITY, f64::max).ceil() as i32,
        );
        let (y0, y1) = (
            ys.iter().cloned().fold(f64::INFINITY, f64::min).floor() as i32,
            ys.iter().cloned().fold(f64::NEG_INFINITY, f64::max).ceil() as i32,
        );
        for y in y0..=y1 {
            for x in x0..=x1 {
                let (fx, fy) = (x as f64 + 0.5, y as f64 + 0.5);
                if !Self::inside_convex(fx, fy, points) {
                    continue;
                }
                let depth = d0 + dx * (fx - origin.projected.0) + dy * (fy - origin.projected.1);
                self.paint_pixel(x, y, color, depth);
            }
        }
    }
    /// Draw a translucent filled convex polygon (ranges).
    pub fn polygon_blend(&mut self, color: [u8; 4], points: &[ProjectedPoint], _width: i32) {
        if points.len() < 3 {
            return;
        }
        let (d0, dx, dy) = match self.plane_of(points) {
            Some(p) => p,
            None => return,
        };
        let origin = &points[0];
        let mut xs = Vec::new();
        let mut ys = Vec::new();
        for p in points.iter() {
            xs.push(p.projected.0);
            ys.push(p.projected.1);
        }
        let (x0, x1) = (
            xs.iter().cloned().fold(f64::INFINITY, f64::min).floor() as i32,
            xs.iter().cloned().fold(f64::NEG_INFINITY, f64::max).ceil() as i32,
        );
        let (y0, y1) = (
            ys.iter().cloned().fold(f64::INFINITY, f64::min).floor() as i32,
            ys.iter().cloned().fold(f64::NEG_INFINITY, f64::max).ceil() as i32,
        );
        for y in y0..=y1 {
            for x in x0..=x1 {
                let (fx, fy) = (x as f64 + 0.5, y as f64 + 0.5);
                if !Self::inside_convex(fx, fy, points) {
                    continue;
                }
                let depth = d0 + dx * (fx - origin.projected.0) + dy * (fy - origin.projected.1);
                if self.decal {
                    let i = (y as usize).wrapping_mul(self.w).wrapping_add(x as usize);
                    if x < 0 || y < 0 || x >= self.w as i32 || y >= self.h as i32 {
                        continue;
                    }
                    if (depth - self.depth[i]).abs() > 1e-4 {
                        continue;
                    }
                    let a = 70.0 / 255.0;
                    let dst = &mut self.color[i * 3..i * 3 + 3];
                    for c in 0..3 {
                        dst[c] = ((dst[c] as f64) * (1.0 - a) + color[c] as f64 * a).round() as u8;
                    }
                } else {
                    self.paint_pixel_blend(x, y, color, depth);
                }
            }
        }
    }
    /// Draw a line with depth interpolated along its projected segment.
    pub fn line(&mut self, color: [u8; 3], a: &ProjectedPoint, b: &ProjectedPoint, width: i32) {
        let w = width.max(1);
        // Bounding box of the thick segment.
        let (x0, x1) = (a.sx.min(b.sx) - w, a.sx.max(b.sx) + w);
        let (y0, y1) = (a.sy.min(b.sy) - w, a.sy.max(b.sy) + w);
        let (ax, ay) = a.projected;
        let (dx, dy) = (b.projected.0 - ax, b.projected.1 - ay);
        let len_sq = dx * dx + dy * dy;
        let same_plane = match (a.ground_plane, b.ground_plane) {
            (Some(pa), Some(pb)) => pa == pb,
            _ => false,
        };
        let plane = if same_plane { a.ground_plane } else { None };
        let r = w as f64 / 2.0;
        for y in y0..=y1 {
            for x in x0..=x1 {
                let (fx, fy) = (x as f64 + 0.5, y as f64 + 0.5);
                // Distance from pixel centre to the segment.
                let t = if len_sq > constants::DEPTH_EPSILON {
                    ((fx - ax) * dx + (fy - ay) * dy) / len_sq
                } else {
                    0.0
                };
                let tc = t.clamp(0.0, 1.0);
                let (px, py) = (ax + dx * tc, ay + dy * tc);
                let dpx = fx - px;
                let dpy = fy - py;
                if (dpx * dpx + dpy * dpy).sqrt() > r + 0.5 {
                    continue;
                }
                let depth = if let Some((slope, intercept)) = plane {
                    fy * slope + intercept
                } else {
                    a.depth + tc * (b.depth - a.depth)
                };
                self.paint_pixel(x, y, color, depth);
            }
        }
    }
    /// Translucent line (ranges outlines, paths).
    pub fn line_blend(
        &mut self,
        color: [u8; 4],
        a: &ProjectedPoint,
        b: &ProjectedPoint,
        width: i32,
    ) {
        let w = width.max(1);
        let (x0, x1) = (a.sx.min(b.sx) - w, a.sx.max(b.sx) + w);
        let (y0, y1) = (a.sy.min(b.sy) - w, a.sy.max(b.sy) + w);
        let (ax, ay) = a.projected;
        let (dx, dy) = (b.projected.0 - ax, b.projected.1 - ay);
        let len_sq = dx * dx + dy * dy;
        let r = w as f64 / 2.0;
        for y in y0..=y1 {
            for x in x0..=x1 {
                let (fx, fy) = (x as f64 + 0.5, y as f64 + 0.5);
                let t = if len_sq > constants::DEPTH_EPSILON {
                    ((fx - ax) * dx + (fy - ay) * dy) / len_sq
                } else {
                    0.0
                };
                let tc = t.clamp(0.0, 1.0);
                let (px, py) = (ax + dx * tc, ay + dy * tc);
                let dpx = fx - px;
                let dpy = fy - py;
                if (dpx * dpx + dpy * dpy).sqrt() > r + 0.5 {
                    continue;
                }
                let depth = a.depth + tc * (b.depth - a.depth);
                self.paint_pixel_blend(x, y, color, depth);
            }
        }
    }
    /// Filled circle at a world point (projectiles): depth of the centre.
    pub fn disc(&mut self, color: [u8; 3], c: &ProjectedPoint, radius: f32) {
        let r = radius as f64;
        for y in (c.sy - radius as i32 - 1)..=(c.sy + radius as i32 + 1) {
            for x in (c.sx - radius as i32 - 1)..=(c.sx + radius as i32 + 1) {
                let (fx, fy) = (x as f64 + 0.5, y as f64 + 0.5);
                let d = ((fx - c.projected.0).powi(2) + (fy - c.projected.1).powi(2)).sqrt();
                if d <= r {
                    self.paint_pixel(x, y, color, c.depth);
                }
            }
        }
    }
}
