//! Reading game assets on every target.
//!
//! Maps and sound recordings are runtime assets, not values compiled into the
//! binary. `macroquad::file::load_file` is the one loader that works both on
//! desktop and in a browser: it reads a file from the filesystem natively and
//! performs an HTTP request for the same path in the web build.
//!
//! The path itself still differs between targets: the native build passes
//! absolute paths rooted at the repository ([`crate::constants::maps_dir`] and
//! [`crate::constants::sounds_dir`]), while the web build passes paths relative
//! to the served page (`maps/...`, `sounds/...`). Keeping both behind this
//! module means map loading and sound loading have one implementation.
//!
//! This module needs an initialized macroquad context, so headless tests must
//! read their fixtures with `std::fs` directly and feed the bytes to the pure
//! parsers in `mapfile.rs`.

use std::path::Path;

/// Read one game asset into memory.
///
/// On desktop the future resolves immediately (miniquad reads the file in
/// place); in a browser it stays pending until the HTTP request completes. An
/// error is returned as text for the caller to log together with the path it
/// already knows; a failed read never stops the game.
pub async fn read(path: &Path) -> Result<Vec<u8>, String> {
    let url = path
        .to_str()
        .ok_or_else(|| "path is not valid UTF-8".to_string())?;
    macroquad::file::load_file(url)
        .await
        .map_err(|err| err.to_string())
}
