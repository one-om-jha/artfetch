use std::path::{Path, PathBuf};
use std::fs;

use anyhow::Result;
use image::GenericImageView;
use serde::{Deserialize, Serialize};

use crate::catalog::CatalogEntry;
use crate::config::{cache_dir, file_is_valid, load_config, Config, FIVE_K, UA};

// ── Piece ────────────────────────────────────────────────────────────────────

#[derive(Serialize, Deserialize, Clone, Default)]
pub struct Piece {
    pub url: String,
    pub title: String,
    pub artist: String,
    pub thumb_url: String,
    pub image_base: String,
    pub primary_image: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub local_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resolution: Option<[u32; 2]>,
    #[serde(default)]
    pub upscaled: bool,
}

impl Piece {
    pub fn from_entry(e: &CatalogEntry) -> Self {
        Piece {
            url: e.web_url.clone(),
            title: e.title.clone(),
            artist: e.artist.clone(),
            thumb_url: e.thumb_url.clone(),
            primary_image: e.full_image_url.clone(),
            ..Default::default()
        }
    }

    pub fn is_downloaded(&self) -> bool {
        self.local_path
            .as_deref()
            .map(|p| Path::new(p).exists())
            .unwrap_or(false)
    }

    pub fn needs_upscale(&self) -> bool {
        match self.resolution {
            Some([w, h]) => w < FIVE_K.0 && h < FIVE_K.1 && !self.upscaled,
            None => false,
        }
    }
}

pub fn safe_name(piece: &Piece) -> String {
    let slug: String = piece
        .title
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '-' {
                c
            } else {
                '_'
            }
        })
        .take(60)
        .collect();
    format!("{}__{}", slug, piece.url.len() % 10000)
}

// ── Gallery (local collection) ───────────────────────────────────────────────

pub struct Gallery {
    pub pieces: Vec<Piece>,
    pub config: Config,
}

impl Gallery {
    pub fn load() -> Self {
        let config = load_config();
        let _ = fs::create_dir_all(&config.download_dir);
        let _ = fs::create_dir_all(cache_dir());

        let state = config.download_dir.join(".state.json");
        let pieces = fs::read_to_string(&state)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default();

        Gallery { pieces, config }
    }

    pub fn save(&self) {
        let state = self.config.download_dir.join(".state.json");
        if let Ok(json) = serde_json::to_string_pretty(&self.pieces) {
            let _ = fs::write(state, json);
        }
    }

    pub fn find_by_url(&self, url: &str) -> Option<usize> {
        self.pieces.iter().position(|p| p.url == url)
    }

    pub fn get_or_insert(&mut self, piece: Piece) -> usize {
        if let Some(idx) = self.find_by_url(&piece.url) {
            idx
        } else {
            self.pieces.push(piece);
            self.save();
            self.pieces.len() - 1
        }
    }

    pub fn preview_path(&self, piece: &Piece) -> Option<PathBuf> {
        let name = safe_name(piece);
        let preview = cache_dir().join(format!("{}_preview.jpg", name));
        if preview.exists() {
            return Some(preview);
        }
        let thumb = cache_dir().join(format!("{}_thumb.jpg", name));
        if thumb.exists() {
            return Some(thumb);
        }
        if piece.is_downloaded() {
            return piece.local_path.as_ref().map(PathBuf::from);
        }
        None
    }
}

// ── Download + Thumbnail ─────────────────────────────────────────────────────

pub async fn download_piece(
    client: &reqwest::Client,
    piece: &mut Piece,
    config: &Config,
) -> Result<String> {
    if piece.is_downloaded() {
        return Ok("Already downloaded".into());
    }

    let name = safe_name(piece);
    let _ = fs::create_dir_all(&config.download_dir);
    let mut output = config.download_dir.join(format!("{}.jpg", name));
    let mut i = 1;
    while output.exists() {
        output = config.download_dir.join(format!("{}_{}.jpg", name, i));
        i += 1;
    }

    let downloaded = if !piece.primary_image.is_empty() {
        let bytes = client
            .get(&piece.primary_image)
            .header("User-Agent", UA)
            .send()
            .await?
            .bytes()
            .await?;
        fs::write(&output, &bytes)?;
        true
    } else if !piece.image_base.is_empty() {
        let mut ok = false;

        let mut args = dezoomify_rs::Arguments::default();
        args.input_uri = Some(piece.url.clone());
        args.outfile = Some(output.clone());
        args.largest = true;
        args.logging = "error".to_string();
        if dezoomify_rs::dezoomify(&args).await.is_ok() && file_is_valid(&output) {
            ok = true;
        }

        if !ok {
            let max_url = format!("{}=s0", piece.image_base);
            let bytes = client
                .get(&max_url)
                .header("User-Agent", UA)
                .send()
                .await?
                .bytes()
                .await?;
            fs::write(&output, &bytes)?;
            if file_is_valid(&output) {
                ok = true;
            }
        }
        ok
    } else {
        return Ok("No downloadable image available".into());
    };

    if downloaded && file_is_valid(&output) {
        match image::open(&output) {
            Ok(img) => {
                let (w, h) = img.dimensions();
                let preview = cache_dir().join(format!("{}_preview.jpg", name));
                let _ = img.thumbnail(2000, 2000).save(&preview);
                piece.local_path = Some(output.to_string_lossy().into_owned());
                piece.resolution = Some([w, h]);
                let mut msg = format!("Downloaded: {}\u{00d7}{}", w, h);
                if piece.needs_upscale() {
                    msg.push_str("  [below 5K \u{2014} press u to upscale]");
                }
                Ok(msg)
            }
            Err(e) => Ok(format!("File saved but unreadable: {}", e)),
        }
    } else {
        Ok("Download failed".into())
    }
}

pub async fn cache_thumbnail(client: &reqwest::Client, piece: &Piece) {
    if piece.thumb_url.is_empty() {
        return;
    }
    let name = safe_name(piece);
    let dest = cache_dir().join(format!("{}_thumb.jpg", name));
    if dest.exists() {
        return;
    }
    let _ = fs::create_dir_all(cache_dir());
    if let Ok(resp) = client
        .get(&piece.thumb_url)
        .header("User-Agent", UA)
        .send()
        .await
    {
        if let Ok(bytes) = resp.bytes().await {
            let _ = fs::write(&dest, &bytes);
        }
    }
}
