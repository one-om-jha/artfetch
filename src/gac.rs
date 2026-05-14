use anyhow::Result;
use scraper::{Html, Selector};

use crate::config::UA;

fn strip_gac_size_param(url: &str) -> &str {
    url.rfind('=').map(|i| &url[..i]).unwrap_or(url)
}

pub async fn fetch_gac_metadata(
    client: &reqwest::Client,
    url: &str,
) -> Result<(String, String, String, String)> {
    let body = client
        .get(url)
        .header("User-Agent", UA)
        .send()
        .await?
        .text()
        .await?;

    let doc = Html::parse_document(&body);
    let mut title = String::new();
    let mut artist = String::new();
    let mut thumb = String::new();
    let mut image_base = String::new();

    if let Ok(sel) = Selector::parse("meta[property='og:title']") {
        if let Some(el) = doc.select(&sel).next() {
            if let Some(content) = el.value().attr("content") {
                if let Some((t, a)) = content.rsplit_once(" - ") {
                    title = t.trim().to_string();
                    artist = a.trim().to_string();
                } else {
                    title = content.trim().to_string();
                }
            }
        }
    }

    if let Ok(sel) = Selector::parse("meta[property='og:image']") {
        if let Some(el) = doc.select(&sel).next() {
            if let Some(content) = el.value().attr("content") {
                thumb = content.to_string();
                image_base = strip_gac_size_param(content).to_string();
            }
        }
    }

    if title.is_empty() {
        if let Ok(sel) = Selector::parse("title") {
            if let Some(el) = doc.select(&sel).next() {
                let text: String = el.text().collect();
                title = text
                    .split(&['\u{2013}', '\u{2014}', '|'][..])
                    .next()
                    .unwrap_or("")
                    .trim()
                    .to_string();
            }
        }
    }

    Ok((title, artist, thumb, image_base))
}
