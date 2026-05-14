# artfetch

Browse museum art from the terminal. Inline image previews via the Kitty graphics protocol (works in **Ghostty**, Kitty, WezTerm).

Sources paintings and drawings from the [Art Institute of Chicago](https://www.artic.edu/) and the [Cleveland Museum of Art](https://www.clevelandart.org/) open-access APIs. Filter by museum or technique (oil, watercolor, tempera, ink, chalk, pastel). Download full-resolution images and auto-upscale to 5K+ with [Real-ESRGAN](https://github.com/xinntao/Real-ESRGAN).

Also supports [Google Arts & Culture](https://artsandculture.google.com/) URLs — extracts deep-zoom tiles via [dezoomify-rs](https://github.com/lovasoa/dezoomify-rs).

Single static Rust binary. No runtime dependencies. Real-ESRGAN downloaded on demand from within the TUI.

## Install

```bash
cargo install --path .
```

Or build manually:

```bash
cargo build --release
# binary at target/release/artfetch
```

## Usage

```bash
# Browse the catalog
artfetch

# Open a Google Arts & Culture artwork directly
artfetch "https://artsandculture.google.com/asset/blossoming-chestnut-trees/2gE-tfXmAz99tA"
```

On first launch, artfetch fetches ~1000+ paintings and drawings from both museum APIs and caches them locally (refreshes weekly).

## Keybindings

### Browse

| Key | Action |
|-----|--------|
| `j` / `k` | Move selection up / down |
| `Enter` | Open detail view |
| `f` | Filter menu — switch source or technique |
| `/` | Search by title or artist |
| `n` / `p` | Next / previous page |
| `a` | Add a Google Arts & Culture URL |
| `q` | Quit |

### Detail

| Key | Action |
|-----|--------|
| `j` / `k` | Previous / next artwork |
| `d` | Download full-resolution image |
| `u` | Upscale 2× with Real-ESRGAN |
| `o` | Open in system image viewer |
| `c` | Configure download directory |
| `a` | Add a Google Arts & Culture URL |
| `Esc` | Back to browse |

### Filter menu

Press `f` from the browse view. Single keypress to select:

```
  Source                  Medium
  ──────                  ──────
  [1] All                 [a] All
  [2] Art Inst. Chicago   [b] Oil
  [3] Cleveland Museum    [c] Watercolor
                          [d] Tempera
                          [e] Ink
                          [f] Chalk / Charcoal
                          [g] Pastel
```

Active filters shown in the browse header as `[Source / Medium]`.

## Architecture

```
src/
  main.rs       Entry point, CLI args
  config.rs     Paths, constants, user config
  catalog.rs    CatalogEntry, AIC + Cleveland API clients, cache
  gallery.rs    Piece, Gallery, download, thumbnails
  app.rs        TUI views, event loop, drawing
  kitty.rs      Kitty graphics protocol, ANSI codes
  esrgan.rs     Real-ESRGAN auto-install and upscale
  gac.rs        Google Arts & Culture metadata scraping
```

## Storage

| Path | Purpose |
|------|---------|
| `~/.config/artfetch/config.json` | Download directory setting |
| `~/.cache/artfetch/aic_catalog.json` | AIC catalog cache (7-day TTL) |
| `~/.cache/artfetch/cleveland_catalog.json` | Cleveland catalog cache (7-day TTL) |
| `~/.cache/artfetch/*_thumb.jpg` | Thumbnail cache |
| `~/Pictures/artfetch/` | Downloaded images (default) |
| `~/.local/share/artfetch/realesrgan/` | Real-ESRGAN binary (if installed) |
