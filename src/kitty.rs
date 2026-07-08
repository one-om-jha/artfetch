use std::fs;
use std::io::{self, Write};
use std::path::Path;

use base64::{engine::general_purpose, Engine as _};
use crossterm::terminal;
use image::GenericImageView;

use crate::config::cache_dir;

pub const RST: &str = "\x1b[0m";
pub const BOLD: &str = "\x1b[1m";
pub const DIM: &str = "\x1b[90m";
pub const GREEN: &str = "\x1b[32m";
pub const YELLOW: &str = "\x1b[33m";
pub const CYAN: &str = "\x1b[36m";
pub const INV: &str = "\x1b[7m";

pub fn kitty_clear(w: &mut impl Write) -> io::Result<()> {
    write!(w, "\x1b_Ga=d;\x1b\\")?;
    w.flush()
}

pub fn kitty_show(w: &mut impl Write, path: &Path) -> io::Result<(u32, u32)> {
    let ws = terminal::window_size().unwrap_or(terminal::WindowSize {
        columns: 80,
        rows: 24,
        width: 640,
        height: 384,
    });

    let cell_w = if ws.columns > 0 {
        ws.width as f64 / ws.columns as f64
    } else {
        8.0
    };
    let cell_h = if ws.rows > 0 {
        ws.height as f64 / ws.rows as f64
    } else {
        16.0
    };

    let fit_w = ((ws.columns.saturating_sub(4)) as f64 * cell_w) as u32;
    let fit_h = ((ws.rows.saturating_sub(14)) as f64 * cell_h) as u32;

    let img = image::open(path).map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
    let (ow, oh) = img.dimensions();

    let display = if ow > fit_w || oh > fit_h {
        img.resize(fit_w, fit_h, image::imageops::FilterType::Lanczos3)
    } else {
        img
    };

    let tmp = cache_dir().join("_kitty.png");
    display
        .save(&tmp)
        .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
    let data = fs::read(&tmp)?;

    let b64 = general_purpose::STANDARD.encode(&data);
    let total_len = b64.len();
    let mut offset = 0;
    let mut first = true;

    while offset < total_len {
        let end = (offset + 4096).min(total_len);
        let chunk = &b64[offset..end];
        let more = if end < total_len { 1 } else { 0 };
        if first {
            write!(w, "\x1b_Gf=100,a=T,m={};{}\x1b\\", more, chunk)?;
            first = false;
        } else {
            write!(w, "\x1b_Gm={};{}\x1b\\", more, chunk)?;
        }
        offset = end;
    }
    writeln!(w)?;
    w.flush()?;
    Ok((ow, oh))
}
