use std::io::{self, Write};
use std::path::PathBuf;
use std::process::Command;
use std::fs;

use anyhow::Result;
use crossterm::{
    cursor,
    event::{self, Event, KeyCode, KeyEventKind, KeyModifiers},
    execute,
    terminal::{
        self, disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
    },
};
use image::GenericImageView;

use crate::catalog::{CatalogEntry, Medium, MuseumSource};
use crate::config::{cache_dir, save_config, FIVE_K};
use crate::esrgan::{esrgan_installed, install_esrgan, upscale_image};
use crate::gac::fetch_gac_metadata;
use crate::gallery::{cache_thumbnail, download_piece, safe_name, Gallery, Piece};
use crate::kitty::*;

// ── TUI Types ────────────────────────────────────────────────────────────────

#[derive(Clone, Copy, PartialEq)]
enum View {
    Browse,
    Detail,
}

pub struct BrowseState {
    page: usize,
    selected: usize,
    pub source: MuseumSource,
    pub filter: Medium,
    query: String,
    pub filtered: Vec<usize>,
}

// ── App ──────────────────────────────────────────────────────────────────────

pub struct App {
    rt: tokio::runtime::Handle,
    client: reqwest::Client,
    pub catalog: Vec<CatalogEntry>,
    gallery: Gallery,
    view: View,
    browse: BrowseState,
    current_piece: Option<Piece>,
    detail_browse_idx: Option<usize>,
    status: String,
}

impl App {
    pub fn new(
        rt: tokio::runtime::Handle,
        client: reqwest::Client,
        catalog: Vec<CatalogEntry>,
        gallery: Gallery,
    ) -> Self {
        let mut app = App {
            rt,
            client,
            catalog,
            gallery,
            view: View::Browse,
            browse: BrowseState {
                page: 0,
                selected: 0,
                source: MuseumSource::All,
                filter: Medium::All,
                query: String::new(),
                filtered: Vec::new(),
            },
            current_piece: None,
            detail_browse_idx: None,
            status: String::new(),
        };
        app.refilter();
        app
    }

    pub fn set_piece(&mut self, piece: Piece) {
        self.current_piece = Some(piece);
        self.view = View::Detail;
    }

    fn refilter(&mut self) {
        let q = self.browse.query.to_lowercase();
        self.browse.filtered = self
            .catalog
            .iter()
            .enumerate()
            .filter(|(_, e)| self.browse.source.matches(e))
            .filter(|(_, e)| self.browse.filter.matches(e))
            .filter(|(_, e)| {
                q.is_empty()
                    || e.title.to_lowercase().contains(&q)
                    || e.artist.to_lowercase().contains(&q)
            })
            .map(|(i, _)| i)
            .collect();
        self.browse.page = 0;
        self.browse.selected = 0;
    }

    fn page_size(&self) -> usize {
        let (_, rows) = terminal::size().unwrap_or((80, 24));
        (rows as usize).saturating_sub(8).max(1)
    }

    fn navigate_detail(&mut self, filtered_idx: usize) {
        if filtered_idx >= self.browse.filtered.len() {
            return;
        }
        let entry = &self.catalog[self.browse.filtered[filtered_idx]];
        let piece = Piece::from_entry(entry);
        let client = self.client.clone();
        let thumb_piece = piece.clone();
        self.rt.block_on(async {
            cache_thumbnail(&client, &thumb_piece).await;
        });
        self.current_piece = Some(piece);
        self.detail_browse_idx = Some(filtered_idx);
        self.status.clear();
    }

    fn add_gac_url(&mut self, w: &mut impl Write) -> io::Result<()> {
        let url = self.read_line_input(w, "GAC URL: ");
        if url.is_empty() {
            return Ok(());
        }
        self.status = "Fetching metadata\u{2026}".into();
        self.draw(w)?;
        let client = self.client.clone();
        match self.rt.block_on(async {
            let (title, artist, thumb, base) = fetch_gac_metadata(&client, &url).await?;
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
                self.status = format!("Added: {}", piece.title);
                self.current_piece = Some(piece);
                self.view = View::Detail;
            }
            Err(e) => {
                self.status = format!("Error: {}", e);
            }
        }
        Ok(())
    }

    // ── Input Helpers ────────────────────────────────────────────────────────

    fn read_line_input(&self, w: &mut impl Write, prompt: &str) -> String {
        let _ = write!(w, "\r\n  {}{}{}", CYAN, prompt, RST);
        let _ = w.flush();
        let _ = disable_raw_mode();
        let mut input = String::new();
        let _ = io::stdin().read_line(&mut input);
        let _ = enable_raw_mode();
        input.trim().to_string()
    }

    fn read_single_key(&self, w: &mut impl Write, prompt: &str) -> Option<char> {
        let _ = write!(w, "\r\n  {}{}{}", YELLOW, prompt, RST);
        let _ = w.flush();
        loop {
            if let Ok(Event::Key(key)) = event::read() {
                if key.kind == KeyEventKind::Press {
                    if let KeyCode::Char(c) = key.code {
                        return Some(c);
                    }
                    if key.code == KeyCode::Esc {
                        return None;
                    }
                }
            }
        }
    }

    // ── Drawing ──────────────────────────────────────────────────────────────

    fn draw(&self, w: &mut impl Write) -> io::Result<()> {
        match self.view {
            View::Browse => self.draw_browse(w),
            View::Detail => self.draw_detail(w),
        }
    }

    fn draw_browse(&self, w: &mut impl Write) -> io::Result<()> {
        let (cols, _) = terminal::size().unwrap_or((80, 24));

        kitty_clear(w)?;
        execute!(
            w,
            cursor::MoveTo(0, 0),
            terminal::Clear(terminal::ClearType::All)
        )?;

        let total = self.browse.filtered.len();
        let ps = self.page_size();
        let start = self.browse.page * ps;
        let end = (start + ps).min(total);

        let filter_tag = format!(
            "[{} / {}]",
            self.browse.source.label(),
            self.browse.filter.label()
        );
        let range = if total > 0 {
            format!("{}-{} of {}", start + 1, end, total)
        } else {
            "0 results".into()
        };
        write!(w, "  {}artfetch{}", BOLD, RST)?;
        let header_right = format!("{}  {}", filter_tag, range);
        let pad = (cols as usize).saturating_sub(12 + header_right.len());
        write!(w, "{:>pad$}{}{}{}\r\n", "", CYAN, header_right, RST)?;

        let rule_len = 60.min(cols as usize - 4);
        let rule = "\u{2500}".repeat(rule_len);
        write!(w, "  {}{}{}\r\n", DIM, rule, RST)?;

        if !self.browse.query.is_empty() {
            write!(
                w,
                "  {}search: \"{}\"{}\r\n",
                DIM, self.browse.query, RST
            )?;
        }

        if total == 0 {
            write!(w, "\r\n  {}No matching works.{}\r\n", DIM, RST)?;
        } else {
            let max_title = (cols as usize / 2).max(20);
            for i in start..end {
                let entry = &self.catalog[self.browse.filtered[i]];
                let row = i - start;
                let is_selected = row == self.browse.selected;
                let num = format!("{:>3}.", i + 1);

                let title: String = entry.title.chars().take(max_title).collect();
                let artist_date = if entry.artist.is_empty() {
                    entry.date.clone()
                } else if entry.date.is_empty() {
                    entry.artist.clone()
                } else {
                    format!("{}, {}", entry.artist, entry.date)
                };
                let artist_date: String = artist_date.chars().take(30).collect();

                let downloaded = self
                    .gallery
                    .find_by_url(&entry.web_url)
                    .and_then(|idx| {
                        if self.gallery.pieces[idx].is_downloaded() {
                            Some("\u{2713}")
                        } else {
                            None
                        }
                    })
                    .unwrap_or(" ");

                let dl_mark = if downloaded == "\u{2713}" {
                    format!("{}\u{2713}{}", GREEN, RST)
                } else {
                    " ".into()
                };

                if is_selected {
                    write!(
                        w,
                        "  {}{} {:<max_title$}  {:<30}{} {}\r\n",
                        INV, num, title, artist_date, RST, dl_mark,
                    )?;
                } else {
                    write!(
                        w,
                        "  {}{}{} {}{:<max_title$}{}  {}{:<30}{} {}\r\n",
                        DIM, num, RST, BOLD, title, RST, DIM, artist_date, RST, dl_mark,
                    )?;
                }
            }
        }

        if !self.status.is_empty() {
            write!(w, "\r\n  {}{}{}\r\n", YELLOW, self.status, RST)?;
        }

        write!(
            w,
            "\r\n  {}[f]ilter  [Enter] detail  [/] search  [j/k] select  [n/p] page  [a]dd GAC  [q]uit{}\r\n",
            DIM, RST
        )?;
        w.flush()
    }

    fn draw_filter_menu(&self, w: &mut impl Write) -> io::Result<()> {
        kitty_clear(w)?;
        execute!(
            w,
            cursor::MoveTo(0, 0),
            terminal::Clear(terminal::ClearType::All)
        )?;

        write!(w, "\r\n  {}Filter{}\r\n\r\n", BOLD, RST)?;

        let src_items: &[(&str, &str, MuseumSource)] = &[
            ("1", "All", MuseumSource::All),
            ("2", "Art Inst. Chicago", MuseumSource::AIC),
            ("3", "Cleveland Museum", MuseumSource::Cleveland),
        ];
        let med_items: &[(&str, &str, Medium)] = &[
            ("a", "All", Medium::All),
            ("b", "Oil", Medium::Oil),
            ("c", "Watercolor", Medium::Watercolor),
            ("d", "Tempera", Medium::Tempera),
            ("e", "Ink", Medium::Ink),
            ("f", "Chalk / Charcoal", Medium::Chalk),
            ("g", "Pastel", Medium::Pastel),
        ];

        write!(
            w,
            "  {}{:<24}{}{}\r\n",
            BOLD, "Source", "Medium", RST
        )?;
        let rule = "\u{2500}".repeat(20);
        write!(
            w,
            "  {}{:<24}{}{}\r\n",
            DIM, rule, rule, RST
        )?;

        let max_rows = src_items.len().max(med_items.len());
        for i in 0..max_rows {
            write!(w, "  ")?;
            if i < src_items.len() {
                let (key, label, variant) = src_items[i];
                let marker = if self.browse.source == variant {
                    CYAN
                } else {
                    ""
                };
                let rst = if marker.is_empty() { "" } else { RST };
                write!(w, "{}[{}] {:<20}{}", marker, key, label, rst)?;
            } else {
                write!(w, "{:24}", "")?;
            }
            if i < med_items.len() {
                let (key, label, variant) = med_items[i];
                let marker = if self.browse.filter == variant {
                    CYAN
                } else {
                    ""
                };
                let rst = if marker.is_empty() { "" } else { RST };
                write!(w, "{}[{}] {}{}", marker, key, label, rst)?;
            }
            write!(w, "\r\n")?;
        }

        write!(
            w,
            "\r\n  {}Press key to select, Esc to close{}\r\n",
            DIM, RST
        )?;
        w.flush()
    }

    fn draw_detail(&self, w: &mut impl Write) -> io::Result<()> {
        let (cols, _) = terminal::size().unwrap_or((80, 24));

        kitty_clear(w)?;
        execute!(
            w,
            cursor::MoveTo(0, 0),
            terminal::Clear(terminal::ClearType::All)
        )?;

        let rule_len = 60.min(cols as usize - 4);
        let rule = "\u{2500}".repeat(rule_len);
        let pos = match self.detail_browse_idx {
            Some(idx) => format!("  {}{}/{}{}", CYAN, idx + 1, self.browse.filtered.len(), RST),
            None => String::new(),
        };
        write!(
            w,
            "  {}artfetch{} {}\u{2014} Detail{}{}\r\n",
            BOLD, RST, DIM, RST, pos
        )?;
        write!(w, "  {}{}{}\r\n\r\n", DIM, rule, RST)?;

        if let Some(piece) = &self.current_piece {
            if let Some(path) = self.gallery.preview_path(piece) {
                write!(w, "  ")?;
                w.flush()?;
                if kitty_show(w, &path).is_err() {
                    write!(w, "{}[preview unavailable]{}\r\n", DIM, RST)?;
                }
            } else if !piece.thumb_url.is_empty() {
                write!(w, "  {}[loading thumbnail...]{}\r\n", DIM, RST)?;
            } else {
                write!(
                    w,
                    "  {}[no preview \u{2014} press d to download]{}\r\n",
                    DIM, RST
                )?;
            }

            write!(w, "\r\n")?;
            if !piece.title.is_empty() {
                write!(w, "  {}{}{}\r\n", BOLD, piece.title, RST)?;
            }
            if !piece.artist.is_empty() {
                write!(w, "  {}{}{}\r\n", DIM, piece.artist, RST)?;
            }

            if let Some([width, height]) = piece.resolution {
                let at_5k = width >= FIVE_K.0 || height >= FIVE_K.1;
                let (color, tag) = if at_5k {
                    (GREEN, "\u{2265} 5K")
                } else {
                    (YELLOW, "< 5K")
                };
                write!(
                    w,
                    "  {}{}\u{00d7}{} ({}){}", color, width, height, tag, RST
                )?;
                if piece.upscaled {
                    write!(w, " {}upscaled{}", CYAN, RST)?;
                }
                write!(w, "\r\n")?;
            } else if piece.is_downloaded() {
                write!(w, "  {}Downloaded{}\r\n", GREEN, RST)?;
            } else if piece.primary_image.is_empty() && piece.image_base.is_empty() {
                write!(
                    w,
                    "  {}Not available for download (non public domain){}\r\n",
                    DIM, RST
                )?;
                write!(w, "  {}View at: {}{}\r\n", DIM, piece.url, RST)?;
            }
        } else {
            write!(w, "  {}No piece selected.{}\r\n", DIM, RST)?;
        }

        if !self.status.is_empty() {
            write!(w, "\r\n  {}{}{}\r\n", YELLOW, self.status, RST)?;
        }

        write!(
            w,
            "\r\n  {}[j/k] prev/next  [d]ownload  [u]pscale  [o]pen  [a]dd GAC  [c]onfig  [Esc] back  [q]uit{}\r\n",
            DIM, RST
        )?;
        w.flush()
    }

    // ── Event Loop ───────────────────────────────────────────────────────────

    pub fn run(&mut self) -> Result<()> {
        let mut stdout = io::stdout();

        let prev_hook = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            let mut out = io::stdout();
            let _ = write!(out, "\x1b_Ga=d;\x1b\\\x1b[?1049l");
            let _ = out.flush();
            let _ = disable_raw_mode();
            prev_hook(info);
        }));

        enable_raw_mode()?;
        execute!(stdout, EnterAlternateScreen)?;

        self.draw(&mut stdout)?;

        loop {
            match event::read()? {
                Event::Key(key) if key.kind == KeyEventKind::Press => {
                    self.status.clear();

                    match self.view {
                        View::Browse => match key.code {
                            KeyCode::Char('q') => break,
                            KeyCode::Char('c')
                                if key.modifiers.contains(KeyModifiers::CONTROL) =>
                            {
                                break
                            }
                            KeyCode::Esc => break,

                            KeyCode::Char('j') | KeyCode::Down => {
                                let ps = self.page_size();
                                let page_items = self
                                    .browse
                                    .filtered
                                    .len()
                                    .saturating_sub(self.browse.page * ps)
                                    .min(ps);
                                if page_items > 0 {
                                    self.browse.selected =
                                        (self.browse.selected + 1).min(page_items - 1);
                                }
                            }
                            KeyCode::Char('k') | KeyCode::Up => {
                                self.browse.selected = self.browse.selected.saturating_sub(1);
                            }
                            KeyCode::Char('n') | KeyCode::Right => {
                                let ps = self.page_size();
                                let max_page = self
                                    .browse
                                    .filtered
                                    .len()
                                    .saturating_sub(1)
                                    / ps.max(1);
                                if self.browse.page < max_page {
                                    self.browse.page += 1;
                                    self.browse.selected = 0;
                                }
                            }
                            KeyCode::Char('p') | KeyCode::Left => {
                                if self.browse.page > 0 {
                                    self.browse.page -= 1;
                                    self.browse.selected = 0;
                                }
                            }

                            KeyCode::Char('f') => {
                                loop {
                                    self.draw_filter_menu(&mut stdout)?;
                                    if let Ok(Event::Key(fk)) = event::read() {
                                        if fk.kind != KeyEventKind::Press {
                                            continue;
                                        }
                                        match fk.code {
                                            KeyCode::Char('1') => {
                                                self.browse.source = MuseumSource::All;
                                                self.refilter();
                                            }
                                            KeyCode::Char('2') => {
                                                self.browse.source = MuseumSource::AIC;
                                                self.refilter();
                                            }
                                            KeyCode::Char('3') => {
                                                self.browse.source = MuseumSource::Cleveland;
                                                self.refilter();
                                            }
                                            KeyCode::Char('a') => {
                                                self.browse.filter = Medium::All;
                                                self.refilter();
                                            }
                                            KeyCode::Char('b') => {
                                                self.browse.filter = Medium::Oil;
                                                self.refilter();
                                            }
                                            KeyCode::Char('c') => {
                                                self.browse.filter = Medium::Watercolor;
                                                self.refilter();
                                            }
                                            KeyCode::Char('d') => {
                                                self.browse.filter = Medium::Tempera;
                                                self.refilter();
                                            }
                                            KeyCode::Char('e') => {
                                                self.browse.filter = Medium::Ink;
                                                self.refilter();
                                            }
                                            KeyCode::Char('f') => {
                                                self.browse.filter = Medium::Chalk;
                                                self.refilter();
                                            }
                                            KeyCode::Char('g') => {
                                                self.browse.filter = Medium::Pastel;
                                                self.refilter();
                                            }
                                            KeyCode::Esc | KeyCode::Enter => break,
                                            _ => break,
                                        }
                                    }
                                }
                            }

                            KeyCode::Char('/') => {
                                let q = self.read_line_input(&mut stdout, "Search: ");
                                self.browse.query = q;
                                self.refilter();
                            }

                            KeyCode::Enter => {
                                if !self.browse.filtered.is_empty() {
                                    let ps = self.page_size();
                                    let idx = self.browse.page * ps + self.browse.selected;
                                    if idx < self.browse.filtered.len() {
                                        self.navigate_detail(idx);
                                        self.view = View::Detail;
                                    }
                                }
                            }

                            KeyCode::Char('a') => {
                                self.add_gac_url(&mut stdout)?;
                            }

                            _ => continue,
                        },

                        View::Detail => match key.code {
                            KeyCode::Char('q') => break,
                            KeyCode::Char('c')
                                if key.modifiers.contains(KeyModifiers::CONTROL) =>
                            {
                                break
                            }
                            KeyCode::Esc => {
                                self.view = View::Browse;
                            }

                            KeyCode::Char('j') | KeyCode::Down | KeyCode::Right => {
                                if let Some(idx) = self.detail_browse_idx {
                                    if idx + 1 < self.browse.filtered.len() {
                                        self.navigate_detail(idx + 1);
                                    }
                                }
                            }
                            KeyCode::Char('k') | KeyCode::Up | KeyCode::Left => {
                                if let Some(idx) = self.detail_browse_idx {
                                    if idx > 0 {
                                        self.navigate_detail(idx - 1);
                                    }
                                }
                            }

                            KeyCode::Char('d') => {
                                if self.current_piece.is_some() {
                                    self.status = "Downloading\u{2026}".into();
                                    self.draw(&mut stdout)?;
                                    let client = self.client.clone();
                                    let config = self.gallery.config.clone();
                                    let mut piece = self.current_piece.clone().unwrap();
                                    let result = self.rt.block_on(async {
                                        download_piece(&client, &mut piece, &config).await
                                    });
                                    match result {
                                        Ok(msg) => {
                                            self.gallery.get_or_insert(piece.clone());
                                            if let Some(idx) =
                                                self.gallery.find_by_url(&piece.url)
                                            {
                                                self.gallery.pieces[idx] = piece.clone();
                                                self.gallery.save();
                                            }
                                            self.current_piece = Some(piece);
                                            self.status = msg;

                                            if self
                                                .current_piece
                                                .as_ref()
                                                .is_some_and(|p| p.needs_upscale())
                                            {
                                                self.try_auto_upscale(&mut stdout)?;
                                            }
                                        }
                                        Err(e) => self.status = format!("Error: {}", e),
                                    }
                                }
                            }

                            KeyCode::Char('u') => {
                                if let Some(piece) = &self.current_piece {
                                    if !piece.is_downloaded() {
                                        self.status = "Download first (press d)".into();
                                    } else if piece.upscaled {
                                        self.status = "Already upscaled".into();
                                    } else if !esrgan_installed() {
                                        if let Some('y') = self.read_single_key(
                                            &mut stdout,
                                            "Real-ESRGAN not installed. Download (~8 MB)? [y/n] ",
                                        ) {
                                            self.status =
                                                "Downloading Real-ESRGAN\u{2026}".into();
                                            self.draw(&mut stdout)?;
                                            let client = self.client.clone();
                                            match self
                                                .rt
                                                .block_on(install_esrgan(&client))
                                            {
                                                Ok(()) => {
                                                    self.status =
                                                        "Installed! Upscaling\u{2026}"
                                                            .into();
                                                    self.draw(&mut stdout)?;
                                                    self.do_upscale();
                                                }
                                                Err(e) => {
                                                    self.status =
                                                        format!("Install failed: {}", e);
                                                }
                                            }
                                        } else {
                                            self.status = "Cancelled".into();
                                        }
                                    } else {
                                        self.status =
                                            "Upscaling 2\u{00d7} with Real-ESRGAN\u{2026}"
                                                .into();
                                        self.draw(&mut stdout)?;
                                        self.do_upscale();
                                    }
                                }
                            }

                            KeyCode::Char('o') => {
                                if let Some(piece) = &self.current_piece {
                                    if let Some(path) = &piece.local_path {
                                        if piece.is_downloaded() {
                                            let _ = Command::new("open")
                                                .arg(path)
                                                .stdout(std::process::Stdio::null())
                                                .stderr(std::process::Stdio::null())
                                                .spawn();
                                            self.status = "Opened in viewer".into();
                                        }
                                    } else {
                                        self.status = "Download first (press d)".into();
                                    }
                                }
                            }

                            KeyCode::Char('a') => {
                                self.add_gac_url(&mut stdout)?;
                            }

                            KeyCode::Char('c') => {
                                let current = self
                                    .gallery
                                    .config
                                    .download_dir
                                    .to_string_lossy()
                                    .to_string();
                                let prompt = format!("Download dir [{}]: ", current);
                                let input = self.read_line_input(&mut stdout, &prompt);
                                if !input.is_empty() {
                                    let new_dir = PathBuf::from(&input);
                                    if !new_dir.exists() {
                                        if let Some('y') = self.read_single_key(
                                            &mut stdout,
                                            &format!(
                                                "Directory doesn't exist. Create {}? [y/n] ",
                                                input
                                            ),
                                        ) {
                                            let _ = fs::create_dir_all(&new_dir);
                                        }
                                    }
                                    if new_dir.exists() {
                                        self.gallery.config.download_dir = new_dir;
                                        save_config(&self.gallery.config);
                                        self.status = format!("Download dir: {}", input);
                                    } else {
                                        self.status = "Directory not created".into();
                                    }
                                }
                            }

                            _ => continue,
                        },
                    }

                    self.draw(&mut stdout)?;
                }

                Event::Resize(_, _) => {
                    self.draw(&mut stdout)?;
                }

                _ => {}
            }
        }

        kitty_clear(&mut stdout)?;
        execute!(stdout, LeaveAlternateScreen)?;
        disable_raw_mode()?;
        Ok(())
    }

    // ── Upscale ──────────────────────────────────────────────────────────────

    fn try_auto_upscale(&mut self, w: &mut impl Write) -> io::Result<()> {
        if !esrgan_installed() {
            if let Some('y') = self.read_single_key(
                w,
                "Below 5K \u{2014} upscale? Requires Real-ESRGAN (~8 MB download). [y/n] ",
            ) {
                self.status = "Downloading Real-ESRGAN\u{2026}".into();
                self.draw(w)?;
                let client = self.client.clone();
                match self.rt.block_on(install_esrgan(&client)) {
                    Ok(()) => {
                        self.status = "Upscaling 2\u{00d7}\u{2026}".into();
                        self.draw(w)?;
                        self.do_upscale();
                    }
                    Err(e) => self.status = format!("Install failed: {}", e),
                }
            }
        } else {
            self.status = "Auto-upscaling 2\u{00d7} (below 5K)\u{2026}".into();
            self.draw(w)?;
            self.do_upscale();
        }
        Ok(())
    }

    fn do_upscale(&mut self) {
        if let Some(piece) = self.current_piece.as_mut() {
            if !piece.is_downloaded() || piece.upscaled {
                return;
            }
            let input = PathBuf::from(piece.local_path.as_ref().unwrap());
            let stem = input.file_stem().unwrap_or_default().to_string_lossy();
            let ext = input.extension().unwrap_or_default().to_string_lossy();
            let output = input.with_file_name(format!("{}_2x.{}", stem, ext));

            if upscale_image(&input, &output) {
                match image::open(&output) {
                    Ok(img) => {
                        let (w, h) = img.dimensions();
                        let name = safe_name(piece);
                        let preview = cache_dir().join(format!("{}_preview.jpg", name));
                        let _ = img.thumbnail(2000, 2000).save(&preview);

                        piece.local_path = Some(output.to_string_lossy().into_owned());
                        piece.resolution = Some([w, h]);
                        piece.upscaled = true;

                        if let Some(idx) = self.gallery.find_by_url(&piece.url) {
                            self.gallery.pieces[idx] = piece.clone();
                            self.gallery.save();
                        }
                        self.status = format!("Upscaled: {}\u{00d7}{}", w, h);
                    }
                    Err(_) => self.status = "Upscaled file unreadable".into(),
                }
            } else {
                self.status = "Upscale failed".into();
            }
        }
    }
}
