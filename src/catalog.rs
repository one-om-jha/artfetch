use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::Path;
use std::time::SystemTime;

use anyhow::Result;
use serde::de::Deserializer;
use serde::{Deserialize, Serialize};
use tokio::runtime::Runtime;

use crate::config::cache_dir;

const AIC_API: &str = "https://api.artic.edu/api/v1";
const AIC_IIIF: &str = "https://www.artic.edu/iiif/2";
const CLEVELAND_API: &str = "https://openaccess-api.clevelandart.org/api/artworks/";
const MET_API: &str = "https://collectionapi.metmuseum.org/public/collection/v1";
const WIKIDATA_SPARQL: &str = "https://query.wikidata.org/sparql";
const COMMONS_API: &str = "https://commons.wikimedia.org/w/api.php";
const NAMOC_COMMONS_CATEGORY: &str = "Category:Collections of the National Art Museum of China";
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
    Met,
    NationalGallery,
    Smithsonian,
    PalaceMuseum,
    ShanghaiMuseum,
    Namoc,
}

impl MuseumSource {
    pub fn label(&self) -> &'static str {
        match self {
            MuseumSource::All => "All",
            MuseumSource::AIC => "Art Inst. Chicago",
            MuseumSource::Cleveland => "Cleveland Museum",
            MuseumSource::Met => "The Met",
            MuseumSource::NationalGallery => "National Gallery",
            MuseumSource::Smithsonian => "Smithsonian",
            MuseumSource::PalaceMuseum => "Palace Museum",
            MuseumSource::ShanghaiMuseum => "Shanghai Museum",
            MuseumSource::Namoc => "Natl. Art Museum China",
        }
    }

    pub fn matches(&self, entry: &CatalogEntry) -> bool {
        match self {
            MuseumSource::All => true,
            MuseumSource::AIC => entry.source == "aic",
            MuseumSource::Cleveland => entry.source == "cleveland",
            MuseumSource::Met => entry.source == "met",
            MuseumSource::NationalGallery => entry.source == "nga",
            MuseumSource::Smithsonian => entry.source == "saam",
            MuseumSource::PalaceMuseum => entry.source == "palace",
            MuseumSource::ShanghaiMuseum => entry.source == "shanghai",
            MuseumSource::Namoc => entry.source == "namoc",
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

// ── Metropolitan Museum of Art API Types ────────────────────────────────────

#[derive(Deserialize)]
struct MetSearchResponse {
    #[allow(dead_code)]
    total: Option<u32>,
    #[serde(rename = "objectIDs")]
    object_ids: Option<Vec<u32>>,
}

#[derive(Deserialize, Default)]
#[allow(dead_code)]
struct MetObject {
    #[serde(rename = "objectID", default, deserialize_with = "null_default")]
    object_id: u32,
    #[serde(rename = "isPublicDomain", default, deserialize_with = "null_default")]
    is_public_domain: bool,
    #[serde(default, deserialize_with = "null_default")]
    title: String,
    #[serde(
        rename = "artistDisplayName",
        default,
        deserialize_with = "null_default"
    )]
    artist_display_name: String,
    #[serde(rename = "objectDate", default, deserialize_with = "null_default")]
    object_date: String,
    #[serde(default, deserialize_with = "null_default")]
    medium: String,
    #[serde(rename = "primaryImage", default, deserialize_with = "null_default")]
    primary_image: String,
    #[serde(
        rename = "primaryImageSmall",
        default,
        deserialize_with = "null_default"
    )]
    primary_image_small: String,
    #[serde(rename = "objectURL", default, deserialize_with = "null_default")]
    object_url: String,
    #[serde(rename = "objectName", default, deserialize_with = "null_default")]
    object_name: String,
    #[serde(default, deserialize_with = "null_default")]
    classification: String,
}

fn is_met_painting_or_drawing(obj: &MetObject) -> bool {
    let haystack = format!(
        "{} {} {}",
        obj.object_name.to_lowercase(),
        obj.classification.to_lowercase(),
        obj.medium.to_lowercase()
    );
    haystack.contains("painting")
        || haystack.contains("drawing")
        || haystack.contains("watercolor")
        || haystack.contains("pastel")
        || haystack.contains("chalk")
        || haystack.contains("ink")
}

fn met_to_entry(obj: MetObject) -> Option<CatalogEntry> {
    if obj.title.is_empty()
        || obj.primary_image_small.is_empty()
        || !is_met_painting_or_drawing(&obj)
    {
        return None;
    }
    let web_url = if obj.object_url.is_empty() {
        format!(
            "https://www.metmuseum.org/art/collection/search/{}",
            obj.object_id
        )
    } else {
        obj.object_url
    };
    let full_image_url = if obj.primary_image.is_empty() {
        obj.primary_image_small.clone()
    } else {
        obj.primary_image
    };

    Some(CatalogEntry {
        source: "met".into(),
        id: obj.object_id.to_string(),
        title: obj.title,
        artist: obj.artist_display_name,
        date: obj.object_date,
        medium: obj.medium,
        thumb_url: obj.primary_image_small,
        full_image_url,
        web_url,
    })
}

// ── Wikidata / Wikimedia Commons Types ──────────────────────────────────────

struct WikidataMuseum {
    source: &'static str,
    label: &'static str,
    qid: &'static str,
}

const WIKIDATA_MUSEUMS: &[WikidataMuseum] = &[
    WikidataMuseum {
        source: "nga",
        label: "National Gallery",
        qid: "Q214867",
    },
    WikidataMuseum {
        source: "saam",
        label: "Smithsonian",
        qid: "Q1192305",
    },
    WikidataMuseum {
        source: "palace",
        label: "Palace Museum",
        qid: "Q2047427",
    },
    WikidataMuseum {
        source: "shanghai",
        label: "Shanghai Museum",
        qid: "Q1051293",
    },
];

#[derive(Deserialize)]
struct WikidataResponse {
    results: WikidataResults,
}

#[derive(Deserialize)]
struct WikidataResults {
    bindings: Vec<WikidataBinding>,
}

#[derive(Deserialize)]
struct WikidataBinding {
    work: WikidataValue,
    #[serde(rename = "workLabel")]
    work_label: Option<WikidataValue>,
    image: WikidataValue,
    #[serde(rename = "creatorLabel")]
    creator_label: Option<WikidataValue>,
    date: Option<WikidataValue>,
    #[serde(rename = "mediumLabel")]
    medium_label: Option<WikidataValue>,
}

#[derive(Deserialize)]
struct WikidataValue {
    value: String,
}

fn hex_value(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

fn percent_decode_utf8(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let (Some(hi), Some(lo)) = (hex_value(bytes[i + 1]), hex_value(bytes[i + 2])) {
                out.push((hi << 4) | lo);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn title_from_commons_url(url: &str) -> String {
    let path = url.split('?').next().unwrap_or(url);
    let file = path.rsplit('/').next().unwrap_or(path);
    let decoded = percent_decode_utf8(file).replace('_', " ");
    decoded
        .rsplit_once('.')
        .map(|(title, _)| title)
        .unwrap_or(&decoded)
        .to_string()
}

fn title_from_commons_file_title(title: &str) -> String {
    let trimmed = title.strip_prefix("File:").unwrap_or(title);
    trimmed
        .rsplit_once('.')
        .map(|(title, _)| title)
        .unwrap_or(trimmed)
        .replace('_', " ")
}

fn short_year(date: &str) -> String {
    if date.len() >= 4 && date.as_bytes()[0..4].iter().all(u8::is_ascii_digit) {
        date[..4].to_string()
    } else {
        date.split('T').next().unwrap_or(date).to_string()
    }
}

fn wikidata_to_entry(source: &str, binding: WikidataBinding) -> Option<CatalogEntry> {
    let id = binding.work.value.rsplit('/').next()?.to_string();
    let image = binding.image.value;
    let label = binding
        .work_label
        .map(|v| v.value)
        .filter(|v| !v.is_empty() && v != &id)
        .unwrap_or_else(|| title_from_commons_url(&image));
    if label.is_empty() {
        return None;
    }

    Some(CatalogEntry {
        source: source.into(),
        id,
        title: label,
        artist: binding.creator_label.map(|v| v.value).unwrap_or_default(),
        date: binding
            .date
            .map(|v| short_year(&v.value))
            .unwrap_or_default(),
        medium: binding.medium_label.map(|v| v.value).unwrap_or_default(),
        thumb_url: format!("{}?width=500", image),
        full_image_url: image,
        web_url: binding.work.value,
    })
}

fn append_medium_label(entry: &mut CatalogEntry, label: Option<String>) {
    let Some(label) = label else {
        return;
    };
    if label.is_empty() {
        return;
    }
    if entry
        .medium
        .split(';')
        .any(|part| part.trim().eq_ignore_ascii_case(&label))
    {
        return;
    }
    if !entry.medium.is_empty() {
        entry.medium.push_str("; ");
    }
    entry.medium.push_str(&label);
}

fn merge_wikidata_bindings(
    source: &str,
    bindings: Vec<WikidataBinding>,
    by_work: &mut HashMap<String, CatalogEntry>,
    order: &mut Vec<String>,
) {
    for binding in bindings {
        let key = binding.work.value.clone();
        let medium = binding.medium_label.as_ref().map(|v| v.value.clone());
        if let Some(entry) = by_work.get_mut(&key) {
            append_medium_label(entry, medium);
        } else if let Some(entry) = wikidata_to_entry(source, binding) {
            order.push(key.clone());
            by_work.insert(key, entry);
        }
    }
}

// ── Wikimedia Commons Types ─────────────────────────────────────────────────

#[derive(Deserialize)]
struct CommonsCategoryResponse {
    query: CommonsCategoryQuery,
}

#[derive(Deserialize)]
struct CommonsCategoryQuery {
    categorymembers: Vec<CommonsCategoryMember>,
}

#[derive(Deserialize)]
struct CommonsCategoryMember {
    pageid: u64,
}

#[derive(Deserialize)]
struct CommonsImageInfoResponse {
    query: CommonsImageInfoQuery,
}

#[derive(Deserialize)]
struct CommonsImageInfoQuery {
    pages: HashMap<String, CommonsImagePage>,
}

#[derive(Clone, Deserialize)]
struct CommonsImagePage {
    title: String,
    imageinfo: Option<Vec<CommonsImageInfo>>,
}

#[derive(Clone, Deserialize)]
struct CommonsImageInfo {
    url: Option<String>,
    thumburl: Option<String>,
    descriptionurl: Option<String>,
}

fn commons_image_to_entry(
    source: &str,
    pageid: u64,
    page: CommonsImagePage,
) -> Option<CatalogEntry> {
    let image = page.imageinfo?.into_iter().next()?;
    let full_image_url = image.url?;
    let thumb_url = image.thumburl.unwrap_or_else(|| full_image_url.clone());
    let web_url = image
        .descriptionurl
        .unwrap_or_else(|| format!("https://commons.wikimedia.org/wiki/{}", page.title));

    Some(CatalogEntry {
        source: source.into(),
        id: pageid.to_string(),
        title: title_from_commons_file_title(&page.title),
        artist: String::new(),
        date: String::new(),
        medium: String::new(),
        thumb_url,
        full_image_url,
        web_url,
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
        eprint!(
            "\r  AIC: page {}/{} ({} works)     ",
            page,
            max_pages,
            entries.len()
        );
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

async fn fetch_met(client: &reqwest::Client) -> Vec<CatalogEntry> {
    let mut seen = HashSet::new();
    let mut ids = Vec::new();

    for (medium, query) in &[("Paintings", "painting"), ("Drawings", "drawing")] {
        let url = format!("{}/search", MET_API);
        match client
            .get(&url)
            .query(&[
                ("hasImages", "true"),
                ("isPublicDomain", "true"),
                ("medium", *medium),
                ("q", *query),
            ])
            .send()
            .await
        {
            Ok(resp) => match resp.json::<MetSearchResponse>().await {
                Ok(data) => {
                    if let Some(object_ids) = data.object_ids {
                        for id in object_ids.into_iter().take(250) {
                            if seen.insert(id) {
                                ids.push(id);
                            }
                        }
                    }
                }
                Err(_) => continue,
            },
            Err(_) => continue,
        }
    }

    let total = ids.len().max(1);
    let mut entries = Vec::new();
    for (idx, id) in ids.into_iter().enumerate() {
        let url = format!("{}/objects/{}", MET_API, id);
        if let Ok(resp) = client.get(&url).send().await {
            if let Ok(obj) = resp.json::<MetObject>().await {
                if let Some(entry) = met_to_entry(obj) {
                    entries.push(entry);
                }
            }
        }
        eprint!(
            "\r  The Met: object {}/{} ({} works)     ",
            idx + 1,
            total,
            entries.len()
        );
    }
    eprintln!(
        "\r  The Met: {} paintings/drawings.           ",
        entries.len()
    );
    entries
}

async fn fetch_wikidata_museum(
    client: &reqwest::Client,
    museum: &WikidataMuseum,
) -> Vec<CatalogEntry> {
    let base_query = format!(
        r#"SELECT DISTINCT ?work ?workLabel ?image ?creatorLabel ?date ?mediumLabel WHERE {{
  {{ ?work wdt:P195 wd:{}. }} UNION {{ ?work wdt:P276 wd:{}. }}
  ?work wdt:P18 ?image.
  OPTIONAL {{ ?work wdt:P170 ?creator. }}
  OPTIONAL {{ ?work wdt:P571 ?date. }}
  OPTIONAL {{ ?work wdt:P186 ?medium. }}
  SERVICE wikibase:label {{ bd:serviceParam wikibase:language "en,zh". }}
}} LIMIT 250"#,
        museum.qid, museum.qid
    );
    let oil_query = format!(
        r#"SELECT DISTINCT ?work ?workLabel ?image ?creatorLabel ?date ?mediumLabel WHERE {{
  {{ ?work wdt:P195 wd:{}. }} UNION {{ ?work wdt:P276 wd:{}. }}
  ?work wdt:P18 ?image.
  ?work wdt:P186 wd:Q296955.
  OPTIONAL {{ ?work wdt:P170 ?creator. }}
  OPTIONAL {{ ?work wdt:P571 ?date. }}
  OPTIONAL {{ ?work wdt:P186 ?medium. }}
  SERVICE wikibase:label {{ bd:serviceParam wikibase:language "en,zh". }}
}} LIMIT 250"#,
        museum.qid, museum.qid
    );

    let mut entries = Vec::new();
    let mut by_work = HashMap::new();
    let mut order = Vec::new();

    for query in [base_query, oil_query] {
        let response = client
            .get(WIKIDATA_SPARQL)
            .query(&[("format", "json"), ("query", &query)])
            .send()
            .await;

        if let Ok(resp) = response {
            if let Ok(data) = resp.json::<WikidataResponse>().await {
                merge_wikidata_bindings(
                    museum.source,
                    data.results.bindings,
                    &mut by_work,
                    &mut order,
                );
            }
        }
    }

    for key in order {
        if let Some(entry) = by_work.remove(&key) {
            entries.push(entry);
        }
    }

    eprintln!("  {}: {} works.", museum.label, entries.len());
    entries
}

async fn fetch_wikidata_museums(client: &reqwest::Client) -> Vec<CatalogEntry> {
    let mut entries = Vec::new();
    for museum in WIKIDATA_MUSEUMS {
        entries.extend(fetch_wikidata_museum(client, museum).await);
    }
    entries
}

async fn fetch_namoc_commons(client: &reqwest::Client) -> Vec<CatalogEntry> {
    let response = client
        .get(COMMONS_API)
        .query(&[
            ("action", "query"),
            ("list", "categorymembers"),
            ("cmtitle", NAMOC_COMMONS_CATEGORY),
            ("cmtype", "file"),
            ("cmlimit", "250"),
            ("format", "json"),
        ])
        .send()
        .await;

    let members = match response {
        Ok(resp) => match resp.json::<CommonsCategoryResponse>().await {
            Ok(data) => data.query.categorymembers,
            Err(_) => Vec::new(),
        },
        Err(_) => Vec::new(),
    };

    let mut entries = Vec::new();
    for chunk in members.chunks(50) {
        let pageids = chunk
            .iter()
            .map(|member| member.pageid.to_string())
            .collect::<Vec<_>>()
            .join("|");
        let response = client
            .get(COMMONS_API)
            .query(&[
                ("action", "query"),
                ("prop", "imageinfo"),
                ("pageids", &pageids),
                ("iiprop", "url"),
                ("iiurlwidth", "500"),
                ("format", "json"),
            ])
            .send()
            .await;

        if let Ok(resp) = response {
            if let Ok(data) = resp.json::<CommonsImageInfoResponse>().await {
                for member in chunk {
                    let key = member.pageid.to_string();
                    if let Some(page) = data.query.pages.get(&key) {
                        if let Some(entry) =
                            commons_image_to_entry("namoc", member.pageid, page.clone())
                        {
                            entries.push(entry);
                        }
                    }
                }
            }
        }
    }

    eprintln!("  NAMOC: {} works from Commons.", entries.len());
    entries
}

fn load_or_fetch_source(
    rt: &Runtime,
    client: &reqwest::Client,
    cache_path: &Path,
    label: &str,
    fetcher: fn(
        &reqwest::Client,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = Vec<CatalogEntry>> + Send + '_>,
    >,
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
    let met_path = cache_dir().join("met_catalog.json");
    let wikidata_path = cache_dir().join("wikidata_museums_v4_catalog.json");
    let namoc_path = cache_dir().join("namoc_commons_catalog.json");

    let aic = load_or_fetch_source(rt, client, &aic_path, "AIC", |c| Box::pin(fetch_aic(c)));
    let cleveland = load_or_fetch_source(rt, client, &cle_path, "Cleveland", |c| {
        Box::pin(fetch_cleveland(c))
    });
    let met = load_or_fetch_source(rt, client, &met_path, "The Met", |c| Box::pin(fetch_met(c)));
    let wikidata = load_or_fetch_source(rt, client, &wikidata_path, "Wikidata museums", |c| {
        Box::pin(fetch_wikidata_museums(c))
    });
    let namoc = load_or_fetch_source(rt, client, &namoc_path, "NAMOC", |c| {
        Box::pin(fetch_namoc_commons(c))
    });

    let mut all = aic;
    all.extend(cleveland);
    all.extend(met);
    all.extend(wikidata);
    all.extend(namoc);
    all.sort_by(|a, b| a.title.to_lowercase().cmp(&b.title.to_lowercase()));
    Ok(all)
}
