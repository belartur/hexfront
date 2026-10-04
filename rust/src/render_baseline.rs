//! Headless mesh-build baseline of the GPU path.
//!
//! Measures pure CPU cost of [`build_terrain`] (once per map) plus
//! [`build_dynamic`] (every frame) per repository map, the `find_path`
//! cost per (source, target, kind) pair over the buildings of each map, and
//! the cost of one full AI decision. It only prints numbers, never asserts,
//! so plain `cargo test` skips it; run explicitly with:
//!
//! ```bash
//! cd rust && cargo test --release render_baseline -- --ignored --nocapture
//! ```

use crate::constants;
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
    // Pathfinding baseline: every ordered building pair, like one AI
    // decision over all pairs (kinds come from the source buildings).
    let mut pairs = 0;
    let mut path_us_total = 0.0;
    let mut path_us_worst: f64 = 0.0;
    let mut unreachable = 0;
    for si in 0..game.buildings.len() {
        for di in 0..game.buildings.len() {
            if si == di {
                continue;
            }
            let src = &game.buildings[si];
            let dst = &game.buildings[di];
            let kind = crate::entities::vehicle_kind_of(src.kind);
            let start = std::time::Instant::now();
            let found = game.board.find_path(src.tile, dst.tile, kind).is_some();
            let us = start.elapsed().as_secs_f64() * 1e6;
            pairs += 1;
            path_us_total += us;
            path_us_worst = path_us_worst.max(us);
            unreachable += (!found) as usize;
        }
    }
    // AI decisions (rules.md section 14.5 weighs every building pair). Every
    // controller is forced to fire in the same step, which is the worst case
    // a frame can get; the second pass reuses the same controllers, whose
    // routes are cached by then, and shows what a later decision costs.
    let ai_count = game.players.iter().filter(|p| !p.is_human).count();
    let mut decide_ms = 0.0;
    let mut decide_warm_ms = 0.0;
    if ai_count > 0 {
        let mut ais = crate::ai::controllers(
            game,
            *constants::ai_difficulty(constants::MAP_DEFAULT_AI_DIFFICULTY),
            1,
        );
        // Each pass gets its own copy of the match, so both start from the
        // same state and only the route cache differs between them.
        let decide = |ais: &mut [crate::ai::AiController]| {
            let mut sim = game.clone();
            let start = std::time::Instant::now();
            for ai in ais.iter_mut() {
                ai.timer = ai.diff.interval;
                ai.update(&mut sim, 0.0);
            }
            start.elapsed().as_secs_f64() * 1000.0
        };
        decide_ms = decide(&mut ais);
        decide_warm_ms = decide(&mut ais);
    }
    println!(
        "{}: tiles={} terrain={:.1} ms ({} verts) dynamic={:.3} ms/frame ({} verts) span={:.3} height={:.3} camp={:.3} ms/frame ({} spots) cached_span=({:.0},{:.0}) cached_height={:.0} path={:.1}us/pair worst={:.0}us unreachable={}/{} decide={:.1} ms/all AI ({} AI, warm={:.1} ms)",
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
        path_us_total / pairs.max(1) as f64,
        path_us_worst,
        unreachable,
        pairs,
        decide_ms,
        ai_count,
        decide_warm_ms,
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
            let (board, _, _) = pad_map(game.board.clone(), Vec::new(), Vec::new());
            let padded = crate::game::Game::new(
                board,
                game.players.clone(),
                game.buildings.clone(),
                Vec::new(),
                0,
            );
            bench_game(&format!("{name} padded"), &padded, frames);
        }
    }
}
