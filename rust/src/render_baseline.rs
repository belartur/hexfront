//! Headless mesh-build baseline of the GPU path.
//!
//! Measures pure CPU cost of [`build_terrain`] (once per map) plus
//! [`build_dynamic`] (every frame) per repository map. Run with:
//!
//! ```bash
//! cd rust && cargo test --release render_baseline -- --nocapture
//! ```

use crate::mapfile;
use crate::mesh::{DynamicMesh, build_dynamic, build_terrain};

#[test]
fn render_baseline() {
    let frames = 10;
    for path in mapfile::list_maps(None) {
        let name = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("?")
            .to_string();
        let game = mapfile::load_game(&path).expect("repo map must load");
        let start = std::time::Instant::now();
        let terrain = build_terrain(&game.board);
        let terrain_ms = start.elapsed().as_secs_f64() * 1000.0;
        let mut dynamic = DynamicMesh::default();
        build_dynamic(&game, 0.0, &mut dynamic);
        let start = std::time::Instant::now();
        for _ in 0..frames {
            build_dynamic(&game, 0.0, &mut dynamic);
        }
        let dynamic_ms = start.elapsed().as_secs_f64() * 1000.0 / frames as f64;
        let tverts: usize = terrain.chunks.iter().map(|c| c.vertices.len()).sum();
        println!(
            "{}: tiles={} terrain={:.1} ms ({} verts) dynamic={:.3} ms/frame ({} verts)",
            name,
            game.board.tiles.len(),
            terrain_ms,
            tverts,
            dynamic_ms,
            dynamic.opaque.vertices.len(),
        );
    }
}
