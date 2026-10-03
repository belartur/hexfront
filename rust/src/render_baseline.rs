//! Headless mesh-build baseline of the GPU path.
//!
//! Measures pure CPU cost of [`build_terrain`] (once per map) plus
//! [`build_dynamic`] (every frame) per repository map. It only prints numbers,
//! never asserts, so plain `cargo test` skips it; run explicitly with:
//!
//! ```bash
//! cd rust && cargo test --release render_baseline -- --ignored --nocapture
//! ```

use crate::editor::{EDITOR_NEW_COLS, EDITOR_NEW_ROWS, pad_map};
use crate::mapfile;
use crate::mesh::{DynamicMesh, build_dynamic, build_terrain, depth_span, max_height};

/// Rebuild the campfire inputs the way `Application::campfire_spots` does:
/// one cluster per fire trap, lifted to the tile top.
fn campfire_count(game: &crate::game::Game) -> usize {
    let mut spots = 0;
    for (tile, t) in game.board.tiles.iter() {
        if t.obstacle
            .as_ref()
            .is_some_and(|o| o.kind == crate::board::ObstacleKind::TrapFire)
        {
            let (cx, cy) = game.board.center_world(*tile);
            let z = crate::mesh::tile_top_z(&game.board, *tile);
            spots += crate::fx::campfire_cluster(cx, cy, z, tile.0, tile.1).len();
        }
    }
    spots
}

/// Time `f` over `reps` repetitions, returning milliseconds per call.
fn bench_ms(reps: usize, mut f: impl FnMut()) -> f64 {
    // Warm up once so page faults and lazy init do not pollute the sample.
    f();
    let start = std::time::Instant::now();
    for _ in 0..reps {
        f();
    }
    start.elapsed().as_secs_f64() * 1000.0 / reps as f64
}

fn bench_game(name: &str, game: &crate::game::Game, frames: usize) {
    let start = std::time::Instant::now();
    let terrain = build_terrain(&game.board);
    let terrain_ms = start.elapsed().as_secs_f64() * 1000.0;
    // Cached statics, like `Application` holds them: computed once per level.
    let span = depth_span(&game.board);
    let height = max_height(&game.board);
    let mut dynamic = DynamicMesh::default();
    let dynamic_ms = bench_ms(frames, || build_dynamic(game, 0.0, height, &mut dynamic));
    // What the cache replaces: the full-board scan cost per frame.
    let span_ms = bench_ms(frames, || {
        std::hint::black_box(depth_span(&game.board));
    });
    let height_ms = bench_ms(frames, || {
        std::hint::black_box(max_height(&game.board));
    });
    let camp_ms = bench_ms(frames, || {
        std::hint::black_box(campfire_count(game));
    });
    let tverts: usize = terrain.chunks.iter().map(|c| c.soup.vertices.len()).sum();
    println!(
        "{}: tiles={} terrain={:.1} ms ({} verts) dynamic={:.3} ms/frame ({} verts) span={:.3} height={:.3} camp={:.3} ms/frame ({} spots) cached_span=({:.0},{:.0}) cached_height={:.0}",
        name,
        game.board.tiles.len(),
        terrain_ms,
        tverts,
        dynamic_ms,
        dynamic.opaque.vertices.len(),
        span_ms,
        height_ms,
        camp_ms,
        campfire_count(game),
        span.0,
        span.1,
        height,
    );
}

#[test]
#[ignore = "prints a timing baseline, run explicitly with -- --nocapture"]
fn render_baseline() {
    let frames = 10;
    for path in mapfile::list_maps(None) {
        let name = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("?")
            .to_string();
        let game = mapfile::load_game(&path).expect("repo map must load");
        bench_game(&name, &game, frames);
        // The editor pads every level up to the standard new-map size
        // (`pad_map`), so a playtest scans a much bigger board than the file
        // holds: measure that shape too.
        if game.board.cols < EDITOR_NEW_COLS || game.board.rows < EDITOR_NEW_ROWS {
            let (board, _) = pad_map(game.board.clone(), Vec::new());
            let padded =
                crate::game::Game::new(board, game.players.clone(), game.buildings.clone(), 0);
            bench_game(&format!("{name} padded"), &padded, frames);
        }
    }
}
