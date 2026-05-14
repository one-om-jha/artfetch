# artfetch

Terminal art browser with inline image previews via the Kitty graphics protocol
(works in **Ghostty**, Kitty, WezTerm).

Browse paintings and drawings from the
[Art Institute of Chicago](https://www.artic.edu/) and the
[Cleveland Museum of Art](https://www.clevelandart.org/), download full-resolution
images, and optionally upscale to 5K+ with
[Real-ESRGAN](https://github.com/xinntao/Real-ESRGAN). Also supports adding
[Google Arts & Culture](https://artsandculture.google.com/) URLs with
[dezoomify-rs](https://github.com/lovasoa/dezoomify-rs) for full deep-zoom extraction.

Single static binary — no runtime dependencies. Real-ESRGAN is auto-downloaded
on first use from within the TUI.

## Build

```bash
cargo install --path .
# or
cargo build --release
# binary at target/release/artfetch
```

## Usage

```bash
# Launch the TUI — catalog loads from cache or fetches on first run
artfetch

# Jump straight to a GAC artwork
artfetch "https://artsandculture.google.com/asset/blossoming-chestnut-trees/2gE-tfXmAz99tA"
```

## Keybindings

### Browse view

| Key | Action |
|-----|--------|
| `j`/`k` or `↑`/`↓` | Select row |
| `Enter` | Open detail view |
| `f` | Open filter menu (source + technique) |
| `/` | Search by title or artist |
| `n`/`p` | Next / previous page |
| `a` | Add a Google Arts & Culture URL |
| `q` / `Esc` | Quit |

### Detail view

| Key | Action |
|-----|--------|
| `d` | Download full-resolution image |
| `u` | Upscale 2× with Real-ESRGAN (auto-downloads on first use) |
| `o` | Open downloaded image in system viewer |
| `a` | Add a Google Arts & Culture URL |
| `c` | Configure download directory |
| `j`/`k` or `←`/`→` | Previous / next artwork |
| `Esc` | Back to browse |
| `q` | Quit |

## How it works

1. **Browse** paintings and drawings from multiple museums in a paginated, filterable list
2. **Detail** — press Enter to see an inline Kitty preview with metadata
3. **Download** — press `d` to fetch the full-resolution image (IIIF max for AIC, dezoomify for GAC)
4. **Upscale** — press `u` to run Real-ESRGAN at 2× if below 5120×2880 (auto-downloads the binary on first use)
5. **Configure** — press `c` to change the download directory

Images save to `~/Pictures/artfetch/` by default. Config at `~/.config/artfetch/config.json`.
Catalog cached at `~/.cache/artfetch/` (refreshes weekly).
