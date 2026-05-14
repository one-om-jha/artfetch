use std::collections::HashSet;
use std::path::Path;
use std::time::SystemTime;
use std::fs;

use anyhow::Result;
use serde::de::Deserializer;
use serde::{Deserialize, Serialize};
use tokio::runtime::Runtime;

use crate::config::cache_dir;

const AIC_API: &str = "https://api.artic.edu/api/v1";
const AIC_IIIF: &str = "https://www.artic.edu/iiif/2";
const CLEVELAND_API: &str = "https://openaccess-api.clevelandart.org/api/artworks/";
const CATALOG_TTL_SECS: u64 = 7 * 24 * 3600;

// ── Unified Catalog Entry ────────────────────────────────────────────────────

#[derive(Serialize, Deserialize, Clone)]
pub struct CatalogEntry {
    pub source: String,
    pub id: String,
    pub title: String,
    pub artist: String,
    pub date: String,
    pub medium: String,
    pub thumb_url: String,
    pub full_image_url: String,
    pub web_url: String,
}

// ── Museum Source Filter ─────────────────────────────────────────────────────

#[derive(Clone, Copy, PartialEq)]
pub enum MuseumSource {
    All,
    AIC,
    Cleveland,
}

impl MuseumSource {
    pub fn label(&self) -> &'static str {
        match self {
            MuseumSource::All => "All",
            MuseumSource::AIC => "Art Inst. Chicago",
            MuseumSource::Cleveland => "Cleveland Museum",
        }
    }

    pub fn matches(&self, entry: &CatalogEntry) -> bool {
        match self {
            MuseumSource::All => true,
            MuseumSource::AIC => entry.source == "aic",
            MuseumSource::Cleveland => entry.source == "cleveland",
        }
    }
}

// ── Medium Filter (technique-based) ──────────────────────────────────────────

#[derive(Clone, Copy, PartialEq)]
pub enum Medium {
    All,
    Oil,
    Watercolor,
    Tempera,
    Ink,
    Chalk,
    Pastel,
}

impl Medium {
    pub fn matches(&self, entry: &CatalogEntry) -> bool {
        let m = entry.medium.to_lowercase();
        match self {
            Medium::All => true,
            Medium::Oil => m.contains("oil"),
            Medium::Watercolor => m.contains("watercolor") || m.contains("gouache"),
            Medium::Tempera => m.contains("tempera"),
            Medium::Ink => m.contains("ink"),
            Medium::Chalk => {
                m.contains("chalk")
                    || m.contains("charcoal")
                    || m.contains("crayon")
                    || m.contains("graphite")
            }
            Medium::Pastel => m.contains("pastel"),
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            Medium::All => "All",
            Medium::Oil => "Oil",
            Medium::Watercolor => "Watercolor",
            Medium::Tempera => "Tempera",
            Medium::Ink => "Ink",
            Medium::Chalk => "Chalk/Charcoal",
            Medium::Pastel => "Pastel",
        }
    }
}

// ── AIC API Types ────────────────────────────────────────────────────────────

fn null_default<'de, D, T>(d: D) -> std::result::Result<T, D::Error>
where
    D: Deserializer<'de>,
    T: Default + Deserialize<'de>,
{
    Option::deserialize(d).map(|o| o.unwrap_or_default())
}

#[derive(Deserialize)]
struct AicPageResponse {
    #[allow(dead_code)]
    pagination: AicPagination,
    data: Vec<AicObject>,
}

#[derive(Deserialize)]
#[allow(dead_code)]
struct AicPagination {
    total: u32,
    total_pages: u32,
    current_page: u32,
}

#[derive(Deserialize, Default)]
#[allow(dead_code)]
struct AicObject {
    #[serde(default, deserialize_with = "null_default")]
    id: u32,
    #[serde(default, deserialize_with = "null_default")]
    title: String,
    #[serde(default, deserialize_with = "null_default")]
    artist_title: String,
    #[serde(default, deserialize_with = "null_default")]
    date_display: String,
    #[serde(default, deserialize_with = "null_default")]
    medium_display: String,
    #[serde(default, deserialize_with = "null_default")]
    classification_title: String,
    #[serde(default, deserialize_with = "null_default")]
    image_id: String,
    #[serde(default, deserialize_with = "null_default")]
    is_public_domain: bool,
}

fn is_painting_or_drawing(obj: &AicObject) -> bool {
    let c = obj.classification_title.to_lowercase();
    c.contains("painting")
        || c.contains("oil on")
        || c.contains("drawing")
        || c.contains("watercolor")
        || c.contains("crayon")
        || c.contains("chalk")
        || c.contains("pastel")
}

fn aic_to_entry(obj: &AicObject) -> CatalogEntry {
    let thumb = if obj.image_id.is_empty() {
        String::new()
    } else {
        format!("{}/{}/full/400,/0/default.jpg", AIC_IIIF, obj.image_id)
    };
    let full = if obj.image_id.is_empty() {
        String::new()
    } else {
        format!("{}/{}/full/max/0/default.jpg", AIC_IIIF, obj.image_id)
    };
    CatalogEntry {
        source: "aic".into(),
        id: obj.id.to_string(),
        title: obj.title.clone(),
        artist: obj.artist_title.clone(),
        date: obj.date_display.clone(),
        medium: obj.medium_display.clone(),
        thumb_url: thumb,
        full_image_url: full,
        web_url: format!("https://www.artic.edu/artworks/{}", obj.id),
    }
}

// ── Cleveland API Types ──────────────────────────────────────────────────────

#[derive(Deserialize)]
struct ClevelandResponse {
    #[allow(dead_code)]
    info: ClevelandInfo,
    data: Vec<ClevelandObject>,
}

#[derive(Deserialize)]
#[allow(dead_code)]
struct ClevelandInfo {
    total: u32,
}

#[derive(Deserialize)]
struct ClevelandObject {
    #[serde(default)]
    id: u32,
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    creation_date: Option<String>,
    #[serde(default)]
    creators: Option<Vec<ClevelandCreator>>,
    #[serde(default)]
    technique: Option<String>,
    #[serde(default)]
    images: Option<ClevelandImages>,
}

#[derive(Deserialize)]
struct ClevelandCreator {
    description: Option<String>,
}

#[derive(Deserialize)]
struct ClevelandImages {
    web: Option<ClevelandImageUrl>,
    print: Option<ClevelandImageUrl>,
}

#[derive(Deserialize)]
struct ClevelandImageUrl {
    url: Option<String>,
}

fn strip_parenthetical(s: &str) -> &str {
    s.find(" (").map(|i| &s[..i]).unwrap_or(s)
}

fn cleveland_to_entry(obj: ClevelandObject) -> Option<CatalogEntry> {
    let title = obj.title.as_deref().unwrap_or("").to_string();
    if title.is_empty() {
        return None;
    }
    let images = obj.images?;
    let thumb_url = images.web.and_then(|w| w.url).unwrap_or_default();
    let full_image_url = images.print.and_then(|p| p.url).unwrap_or_default();
    if thumb_url.is_empty() {
        return None;
    }
    let artist = obj
        .creators
        .as_ref()
        .and_then(|c| c.first())
        .and_then(|c| c.description.as_deref())
        .map(|d| strip_parenthetical(d).to_string())
        .unwrap_or_default();

    Some(CatalogEntry {
        source: "cleveland".into(),
        id: obj.id.to_string(),
        title,
        artist,
        date: obj.creation_date.unwrap_or_default(),
        medium: obj.technique.unwrap_or_default(),
        thumb_url,
        full_image_url,
        web_url: format!("https://www.clevelandart.org/art/{}", obj.id),
    })
}

// ── Catalog Cache + Fetch ────────────────────────────────────────────────────

#[derive(Serialize, Deserialize)]
struct CatalogCache {
    fetched_at: u64,
    entries: Vec<CatalogEntry>,
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap()
        .as_secs()
}

fn load_cache(path: &Path) -> Option<Vec<CatalogEntry>> {
    let data = fs::read_to_string(path).ok()?;
    let cache: CatalogCache = serde_json::from_str(&data).ok()?;
    if now_secs() - cache.fetched_at < CATALOG_TTL_SECS {
        Some(cache.entries)
    } else {
        None
    }
}

fn save_cache(path: &Path, entries: &[CatalogEntry]) {
    let _ = fs::create_dir_all(cache_dir());
    let cache = CatalogCache {
        fetched_at: now_secs(),
        entries: entries.to_vec(),
    };
    if let Ok(json) = serde_json::to_string(&cache) {
        let _ = fs::write(path, json);
    }
}

async fn fetch_aic(client: &reqwest::Client) -> Vec<CatalogEntry> {
    let fields = "id,title,artist_title,date_display,medium_display,classification_title,image_id,is_public_domain";
    let limit = 100;
    let max_pages = 10;
    let mut seen = HashSet::new();
    let mut entries = Vec::new();

    eprint!("  AIC: loading highlights...");
    for page in 1..=5 {
        let url = format!(
            "{}/artworks/search?q=*&query%5Bterm%5D%5Bis_boosted%5D=true&limit={}&page={}&fields={}",
            AIC_API, limit, page, fields
        );
        if let Ok(resp) = client.get(&url).send().await {
            if let Ok(data) = resp.json::<AicPageResponse>().await {
                for obj in data.data {
                    if !obj.title.is_empty()
                        && !obj.image_id.is_empty()
                        && is_painting_or_drawing(&obj)
                        && seen.insert(obj.id)
                    {
                        entries.push(aic_to_entry(&obj));
                    }
                }
            }
        }
    }

    for page in 1..=max_pages {
        let url = format!(
            "{}/artworks/search?q=*&query%5Bterm%5D%5Bis_public_domain%5D=true&limit={}&page={}&fields={}",
            AIC_API, limit, page, fields
        );
        if let Ok(resp) = client.get(&url).send().await {
            if let Ok(data) = resp.json::<AicPageResponse>().await {
                for obj in data.data {
                    if !obj.title.is_empty()
                        && !obj.image_id.is_empty()
                        && is_painting_or_drawing(&obj)
                        && seen.insert(obj.id)
                    {
                        entries.push(aic_to_entry(&obj));
                    }
                }
            }
        }
        eprint!("\r  AIC: page {}/{} ({} works)     ", page, max_pages, entries.len());
    }
    eprintln!("\r  AIC: {} paintings/drawings.           ", entries.len());
    entries
}

async fn fetch_cleveland(client: &reqwest::Client) -> Vec<CatalogEntry> {
    let fields = "id,title,creation_date,creators,type,technique,images";
    let limit = 100;
    let max_pages = 10;
    let mut entries = Vec::new();

    for art_type in &["Painting", "Drawing"] {
        for page in 0..max_pages {
            let skip = page * limit;
            let url = format!(
                "{}?has_image=1&type={}&limit={}&skip={}&fields={}",
                CLEVELAND_API, art_type, limit, skip, fields
            );
            match client.get(&url).send().await {
                Ok(resp) => match resp.json::<ClevelandResponse>().await {
                    Ok(data) => {
                        if data.data.is_empty() {
                            break;
                        }
                        for obj in data.data {
                            if let Some(entry) = cleveland_to_entry(obj) {
                                entries.push(entry);
                            }
                        }
                    }
                    Err(_) => break,
                },
                Err(_) => break,
            }
            eprint!(
                "\r  Cleveland ({}): page {}/{} ({} works)     ",
                art_type,
                page + 1,
                max_pages,
                entries.len()
            );
        }
    }
    eprintln!(
        "\r  Cleveland: {} paintings/drawings.           ",
        entries.len()
    );
    entries
}

fn load_or_fetch_source(
    rt: &Runtime,
    client: &reqwest::Client,
    cache_path: &Path,
    label: &str,
    fetcher: fn(&reqwest::Client) -> std::pin::Pin<Box<dyn std::future::Future<Output = Vec<CatalogEntry>> + Send + '_>>,
) -> Vec<CatalogEntry> {
    match load_cache(cache_path) {
        Some(entries) => {
            eprintln!("  {}: {} works from cache.", label, entries.len());
            entries
        }
        None => {
            let entries = rt.handle().block_on(fetcher(client));
            save_cache(cache_path, &entries);
            entries
        }
    }
}

pub fn load_or_fetch_all(rt: &Runtime, client: &reqwest::Client) -> Result<Vec<CatalogEntry>> {
    let aic_path = cache_dir().join("aic_catalog.json");
    let cle_path = cache_dir().join("cleveland_catalog.json");

    let aic = load_or_fetch_source(rt, client, &aic_path, "AIC", |c| Box::pin(fetch_aic(c)));
    let cleveland = load_or_fetch_source(rt, client, &cle_path, "Cleveland", |c| {
        Box::pin(fetch_cleveland(c))
    });

    let mut all = aic;
    all.extend(cleveland);
    all.sort_by(|a, b| a.title.to_lowercase().cmp(&b.title.to_lowercase()));
    Ok(all)
}
