# Atlas Maker

A pure-Rust desktop app with a 16x16 clickable grid. Click any cell to import
an image into it; click **Save Atlas** to export a combined `atlas.png` plus
an `atlas.json` describing where each image lives in the sheet.

## Requirements

- Rust (install via https://rustup.rs — use a recent stable, 1.75+; on Linux
  make sure you also have the usual GUI dev packages: on Debian/Ubuntu run
  `sudo apt install libgtk-3-dev libxcb-render0-dev libxcb-shape0-dev libxcb-xfixes0-dev libxkbcommon-dev libssl-dev`)

## Build & run

```bash
cargo run --release
```

The first build will download and compile all dependencies (egui, eframe,
image, rfd, serde) — this can take a few minutes.

## How to use

1. The window shows a 16x16 grid of empty buttons.
2. **Left-click** a cell to open a native file picker and import a PNG/JPG/etc.
   The image is resized to fit the cell (64x64 px by default) and shown as a
   thumbnail in the grid.
3. **Right-click** a cell to clear it.
4. Click **Save Atlas** and choose a filename (e.g. `atlas.png`). This writes:
   - `atlas.png` — a single 1024x1024 image (16 columns x 16 rows x 64px cells)
     with every imported image packed into its grid position.
   - `atlas.json` — same name, `.json` extension — metadata for every filled
     cell:

     ```json
     {
       "cell_size": 64,
       "grid_size": 16,
       "atlas_width": 1024,
       "atlas_height": 1024,
       "entries": [
         {
           "index": 0,
           "row": 0,
           "col": 0,
           "x": 0,
           "y": 0,
           "width": 64,
           "height": 64,
           "source_file": "grass.png"
         }
       ]
     }
     ```

   Empty cells are skipped entirely (no entry in `entries`), so the JSON only
   lists cells you actually filled in.

## Customizing

- Change the on-screen size of each grid button: edit `BUTTON_PX` in `src/main.rs`.
- Change the exported pixel size of each atlas cell: edit `CELL_SIZE`.
- Change the grid dimensions (e.g. 8x8 or 32x32): edit `GRID_SIZE`.
