mod app;
mod catalog;
mod config;
mod esrgan;
mod gac;
mod gallery;
mod kitty;

use std::env;
use std::time::Duration;

use anyhow::{Context, Result};
use tokio::runtime::Runtime;

use app::App;
use catalog::load_or_fetch_all;
use config::UA;
use gac::fetch_gac_metadata;
use gallery::{cache_thumbnail, Gallery, Piece};

fn main() -> Result<()> {
    // Handle informational flags before network access or terminal setup.
    for arg in env::args().skip(1) {
        match arg.as_str() {
            "--version" | "-V" => {
                println!("artfetch {}", env!("CARGO_PKG_VERSION"));
                return Ok(());
            }
            "--help" | "-h" => {
                println!("Usage: artfetch [ARTWORK_URL ...]\n\nBrowse museum art in a terminal with Kitty graphics support.\n\n  -h, --help     Show this help\n  -V, --version  Show the version");
                return Ok(());
            }
            _ => {}
        }
    }

    let rt = Runtime::new().context("Failed to create async runtime")?;
    let client = reqwest::Client::builder()
        .user_agent(UA)
        .cookie_store(true)
        .timeout(Duration::from_secs(30))
        .build()
        .context("Failed to create HTTP client")?;

    let catalog = load_or_fetch_all(&rt, &client)?;
    let gallery = Gallery::load();
    let handle = rt.handle().clone();

    let mut app = App::new(handle, client.clone(), catalog, gallery);

    let urls: Vec<String> = env::args()
        .skip(1)
        .filter(|a| a.starts_with("http"))
        .collect();
    for url in &urls {
        match rt.handle().block_on(async {
            let (title, artist, thumb, base) = fetch_gac_metadata(&client, url).await?;
            let piece = Piece {
                url: url.clone(),
                title,
                artist,
                thumb_url: thumb,
                image_base: base,
                ..Default::default()
            };
            cache_thumbnail(&client, &piece).await;
            Ok::<Piece, anyhow::Error>(piece)
        }) {
            Ok(piece) => {
                eprintln!("  Added: {}", piece.title);
                app.set_piece(piece);
            }
            Err(e) => eprintln!("  Error adding {}: {}", url, e),
        }
    }

    app.run()
}
