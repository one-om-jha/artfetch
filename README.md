# artfetch

Browse museum art from the terminal. Inline image previews via the Kitty graphics protocol (works in **Ghostty**, Kitty, WezTerm).

Sources paintings, drawings, and open-access collection images from the [Art Institute of Chicago](https://www.artic.edu/), the [Cleveland Museum of Art](https://www.clevelandart.org/), [The Met](https://www.metmuseum.org/), the National Gallery of Art, Smithsonian American Art Museum, the Palace Museum, Shanghai Museum, and the National Art Museum of China. Filter by museum or technique (oil, watercolor, tempera, ink, chalk, pastel). Download full-resolution images and auto-upscale to 5K+ with [Real-ESRGAN](https://github.com/xinntao/Real-ESRGAN).

Also supports [Google Arts & Culture](https://artsandculture.google.com/) URLs — extracts deep-zoom tiles via [dezoomify-rs](https://github.com/lovasoa/dezoomify-rs).

Single Rust executable. Linux release binaries require glibc 2.35+ and OpenSSL 3. Real-ESRGAN downloaded on demand from within the TUI.

## Install

### Homebrew (macOS and Linux)

After the first tagged release is published:

```bash
brew tap one-om-jha/artfetch https://github.com/one-om-jha/artfetch
brew install one-om-jha/artfetch/artfetch
```

This formula builds from source. Homebrew installs the build dependencies.
Use `brew upgrade` to install a newer formula version.

### Release binaries

Download the archive for your OS and CPU from
[GitHub Releases](https://github.com/one-om-jha/artfetch/releases).
Builds cover macOS (Apple Silicon and Intel, macOS 15+) and Linux (ARM64 and x86_64,
glibc 2.35+ and OpenSSL 3). Extract it and put `artfetch` on your `PATH`.
`SHA256SUMS` contains checksums for all archives.

### From source

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

On first launch, artfetch fetches museum catalog data and caches it locally (refreshes weekly).

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
| `u` | Upscale to 5K with Real-ESRGAN |
| `o` | Open in system image viewer |
| `c` | Configure download directory |
| `a` | Add a Google Arts & Culture URL |
| `Esc` | Back to browse |

Set `ARTFETCH_UPSCALE_DEBUG=1` before launching to write Real-ESRGAN command output to
`~/.cache/artfetch/upscale-debug.log` when an upscale fails.

### Filter menu

Press `f` from the browse view. Single keypress to select:

```
  Source                  Medium
  ──────                  ──────
  [1] All                 [a] All
  [2] Art Inst. Chicago   [b] Oil
  [3] Cleveland Museum    [c] Watercolor
  [4] The Met             [d] Tempera
  [5] National Gallery    [e] Ink
  [6] Smithsonian         [f] Chalk / Charcoal
  [7] Palace Museum       [g] Pastel
  [8] Shanghai Museum
  [9] Natl. Art Museum China
```

Active filters shown in the browse header as `[Source / Medium]`.

## Architecture

```
src/
  main.rs       Entry point, CLI args
  config.rs     Paths, constants, user config
  catalog.rs    CatalogEntry, museum API clients, Wikidata/Commons loader, cache
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
| `~/.cache/artfetch/met_catalog.json` | The Met catalog cache (7-day TTL) |
| `~/.cache/artfetch/wikidata_museums_v4_catalog.json` | National Gallery, Smithsonian, Palace Museum, and Shanghai Museum catalog cache (7-day TTL) |
| `~/.cache/artfetch/namoc_commons_catalog.json` | National Art Museum of China Commons catalog cache (7-day TTL) |
| `~/.cache/artfetch/*_thumb.jpg` | Thumbnail cache |
| `~/Pictures/artfetch/` | Downloaded images (default) |
| `~/.local/share/artfetch/realesrgan/` | Real-ESRGAN binary (if installed) |

## Releasing

1. Set the new version in `Cargo.toml`, then run `cargo check` to update `Cargo.lock`.
2. Set the matching `vX.Y.Z` tag in `Formula/artfetch.rb`.
3. Commit and push these changes to `main`.
4. Create and push the tag:

   ```bash
   git tag v0.2.0
   git push origin v0.2.0
   ```

The Release workflow checks the version, tests and builds all four targets, then
publishes the archives and checksums as a GitHub Release. It uses the built-in
GitHub token; no extra secrets are needed. Published releases are not overwritten.
Use a new version for fixes. Use stable version tags for Homebrew releases.

To test builds without publishing, run the Release workflow manually from Actions.
Homebrew reads the formula from this repository's default branch. Keep its tag
in sync with each stable release so `brew upgrade` sees the new version.
