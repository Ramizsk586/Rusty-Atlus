use eframe::egui;
use egui::{ColorImage, TextureHandle, TextureOptions};
use image::RgbaImage;
use serde::{Deserialize, Serialize};

const BUTTON_PX: f32 = 32.0;
const GRID_SPACING: f32 = 2.0;

// ── Data types ──────────────────────────────────────────────────────────────

#[derive(Serialize, Deserialize, Clone)]
struct AtlasEntry {
    index: usize,
    row: usize,
    col: usize,
    x: u32,
    y: u32,
    width: u32,
    height: u32,
    source_file: String,
}

#[derive(Serialize, Deserialize, Clone)]
struct AtlasData {
    cell_size: u32,
    grid_size: usize,
    atlas_width: u32,
    atlas_height: u32,
    entries: Vec<AtlasEntry>,
}

#[derive(Default)]
struct Cell {
    image: Option<RgbaImage>,
    texture: Option<TextureHandle>,
    source_file: String,
}

// ── App ─────────────────────────────────────────────────────────────────────

struct AtlasApp {
    cells: Vec<Cell>,
    grid_size: usize,
    cell_size: u32,
    status: String,
    last_auto_size: Option<egui::Vec2>,
}

impl Default for AtlasApp {
    fn default() -> Self {
        let gs = 16;
        Self {
            cells: (0..gs * gs).map(|_| Cell::default()).collect(),
            grid_size: gs,
            cell_size: 64,
            status: "Left-click: import image  |  Right-click: clear cell".into(),
            last_auto_size: None,
        }
    }
}

impl AtlasApp {
    // ── Grid management ─────────────────────────────────────────────────────

    fn resize_grid(&mut self, new_size: usize) {
        if new_size == self.grid_size {
            return;
        }
        let old = self.grid_size;
        self.grid_size = new_size;
        let mut new_cells: Vec<Cell> = (0..new_size * new_size).map(|_| Cell::default()).collect();
        let keep = old.min(new_size);
        for r in 0..keep {
            for c in 0..keep {
                let src = r * old + c;
                let dst = r * new_size + c;
                new_cells[dst] = std::mem::take(&mut self.cells[src]);
            }
        }
        self.cells = new_cells;
        self.last_auto_size = None; // trigger window re-fit
        self.status = format!("Grid resized to {new_size} × {new_size}");
    }

    // ── Cell operations ─────────────────────────────────────────────────────

    fn load_image_into_cell(&mut self, ctx: &egui::Context, idx: usize) {
        if let Some(path) = rfd::FileDialog::new()
            .add_filter("Images", &["png", "jpg", "jpeg", "bmp", "gif", "tga", "webp"])
            .pick_file()
        {
            match image::open(&path) {
                Ok(img) => {
                    let resized = img
                        .resize_exact(
                            self.cell_size,
                            self.cell_size,
                            image::imageops::FilterType::Lanczos3,
                        )
                        .to_rgba8();

                    let color_image = ColorImage::from_rgba_unmultiplied(
                        [resized.width() as usize, resized.height() as usize],
                        resized.as_raw(),
                    );
                    let texture = ctx.load_texture(
                        format!("cell_{idx}"),
                        color_image,
                        TextureOptions::default(),
                    );

                    let cell = &mut self.cells[idx];
                    cell.image = Some(resized);
                    cell.texture = Some(texture);
                    cell.source_file = path
                        .file_name()
                        .map(|s| s.to_string_lossy().to_string())
                        .unwrap_or_default();

                    self.status = format!("Loaded '{}' into cell {idx}", cell.source_file);
                }
                Err(e) => {
                    self.status = format!("Failed to load image: {e}");
                }
            }
        }
    }

    fn clear_cell(&mut self, idx: usize) {
        self.cells[idx] = Cell::default();
        self.status = format!("Cleared cell {idx}");
    }

    // ── Save ────────────────────────────────────────────────────────────────

    fn save_atlas(&mut self) {
        let Some(save_path) = rfd::FileDialog::new()
            .set_file_name("atlas.png")
            .add_filter("PNG", &["png"])
            .save_file()
        else {
            return;
        };

        let gs = self.grid_size;
        let cs = self.cell_size;
        let atlas_w = cs * gs as u32;
        let atlas_h = cs * gs as u32;
        let mut atlas_img = RgbaImage::new(atlas_w, atlas_h);
        let mut entries = Vec::new();

        for row in 0..gs {
            for col in 0..gs {
                let idx = row * gs + col;
                let x = col as u32 * cs;
                let y = row as u32 * cs;
                if let Some(cell_img) = &self.cells[idx].image {
                    image::imageops::overlay(&mut atlas_img, cell_img, x as i64, y as i64);
                    entries.push(AtlasEntry {
                        index: idx,
                        row,
                        col,
                        x,
                        y,
                        width: cs,
                        height: cs,
                        source_file: self.cells[idx].source_file.clone(),
                    });
                }
            }
        }

        if let Err(e) = atlas_img.save(&save_path) {
            self.status = format!("Failed to save PNG: {e}");
            return;
        }

        let json_path = save_path.with_extension("json");
        let data = AtlasData {
            cell_size: cs,
            grid_size: gs,
            atlas_width: atlas_w,
            atlas_height: atlas_h,
            entries,
        };

        match serde_json::to_string_pretty(&data) {
            Ok(json_str) => {
                if let Err(e) = std::fs::write(&json_path, json_str) {
                    self.status = format!("PNG saved, but JSON write failed: {e}");
                } else {
                    self.status = format!(
                        "Saved atlas → {} & {}",
                        save_path.display(),
                        json_path.display()
                    );
                }
            }
            Err(e) => {
                self.status = format!("Failed to serialise JSON: {e}");
            }
        }
    }

    // ── Load (open old atlas for editing) ───────────────────────────────────

    fn load_atlas(&mut self, ctx: &egui::Context) {
        let Some(path) = rfd::FileDialog::new()
            .add_filter("Atlas JSON", &["json"])
            .pick_file()
        else {
            return;
        };

        let json_str = match std::fs::read_to_string(&path) {
            Ok(s) => s,
            Err(e) => {
                self.status = format!("Failed to read file: {e}");
                return;
            }
        };

        let data: AtlasData = match serde_json::from_str(&json_str) {
            Ok(d) => d,
            Err(e) => {
                self.status = format!("Failed to parse atlas JSON: {e}");
                return;
            }
        };

        // Load the companion PNG
        let png_path = path.with_extension("png");
        let atlas_img = match image::open(&png_path) {
            Ok(i) => i.to_rgba8(),
            Err(e) => {
                self.status = format!(
                    "Could not open atlas image '{}': {e}",
                    png_path.display()
                );
                return;
            }
        };

        // Apply the atlas settings
        self.cell_size = data.cell_size;
        self.resize_grid(data.grid_size);

        // Populate cells from entries
        let mut loaded = 0usize;
        for entry in &data.entries {
            if entry.row >= self.grid_size || entry.col >= self.grid_size {
                continue;
            }
            let idx = entry.row * self.grid_size + entry.col;

            // Crop the tile from the atlas image
            let sub = match image::imageops::crop_imm(
                &atlas_img,
                entry.x,
                entry.y,
                entry.width.min(atlas_img.width() - entry.x),
                entry.height.min(atlas_img.height() - entry.y),
            )
            .to_image()
            {
                img => img,
            };

            let color_image = ColorImage::from_rgba_unmultiplied(
                [sub.width() as usize, sub.height() as usize],
                sub.as_raw(),
            );
            let texture = ctx.load_texture(
                format!("cell_{idx}"),
                color_image,
                TextureOptions::default(),
            );

            self.cells[idx].image = Some(sub);
            self.cells[idx].texture = Some(texture);
            self.cells[idx].source_file = entry.source_file.clone();
            loaded += 1;
        }

        self.status = format!(
            "Loaded atlas '{}' — {} × {} grid, {loaded} tiles",
            path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default(),
            data.grid_size,
            data.grid_size,
        );
    }

    // ── Auto-resize window to fit content ───────────────────────────────────

    fn auto_resize(&mut self, ctx: &egui::Context) {
        let grid_px = self.grid_size as f32 * (BUTTON_PX + GRID_SPACING) - GRID_SPACING;
        let top_panel_est = 108.0; // approximate top-panel height
        let pad = 18.0;

        // Ideal: exactly fits the grid
        let ideal_w = grid_px + pad;
        let ideal_h = grid_px + top_panel_est;

        // Cap at a reasonable maximum so huge grids don't blow up
        let max_dim = 1100.0;
        let clamped_w = ideal_w.min(max_dim);
        let clamped_h = ideal_h.min(max_dim);
        let target = egui::vec2(clamped_w, clamped_h);

        if self.last_auto_size != Some(target) {
            ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(target));
            self.last_auto_size = Some(target);
        }
    }

    /// Returns true when the grid content is larger than the window and scrolling is needed.
    fn needs_scroll(&self) -> bool {
        let grid_px = self.grid_size as f32 * (BUTTON_PX + GRID_SPACING);
        grid_px > 1082.0 // max_dim - pad
    }
}

// ── egui App trait ──────────────────────────────────────────────────────────

impl eframe::App for AtlasApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Keep window tightly fitted to content
        self.auto_resize(ctx);

        // ── Top panel ───────────────────────────────────────────────────────
        egui::TopBottomPanel::top("top_bar").show(ctx, |ui| {
            ui.add_space(4.0);

            // Row 1: title + action buttons
            ui.horizontal(|ui| {
                ui.heading(egui::RichText::new(format!(
                    "Texture Atlas Creator  ({} × {})",
                    self.grid_size, self.grid_size
                )).strong());
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button("Load Atlas").clicked() {
                        self.load_atlas(ctx);
                    }
                    if ui.button("Save Atlas").clicked() {
                        self.save_atlas();
                    }
                    if ui.button("Clear All").clicked() {
                        self.cells = (0..self.grid_size * self.grid_size)
                            .map(|_| Cell::default())
                            .collect();
                        self.status = "Cleared all cells".into();
                    }
                });
            });

            ui.add_space(2.0);

            // Row 2: grid-size & cell-size controls
            ui.horizontal(|ui| {
                ui.label("Grid size:");
                let gs_before = self.grid_size;
                ui.add(
                    egui::DragValue::new(&mut self.grid_size)
                        .clamp_range(2..=64)
                        .speed(1),
                );
                if self.grid_size != gs_before {
                    self.resize_grid(self.grid_size);
                }

                ui.separator();
                ui.label("Cell px:");
                ui.add(
                    egui::DragValue::new(&mut self.cell_size)
                        .clamp_range(8..=512)
                        .speed(4),
                );
            });

            ui.add_space(2.0);

            // Row 3: status
            ui.label(
                egui::RichText::new(&self.status).small().color(egui::Color32::GRAY),
            );
            ui.add_space(4.0);
        });

        // ── Central panel (the grid) ────────────────────────────────────────
        egui::CentralPanel::default().show(ctx, |ui| {
            let scroll = self.needs_scroll();

            if scroll {
                egui::ScrollArea::both().show(ui, |ui| {
                    self.show_grid(ui, ctx);
                });
            } else {
                self.show_grid(ui, ctx);
            }
        });
    }
}

impl AtlasApp {
    fn show_grid(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let gs = self.grid_size;
        egui::Grid::new("atlas_grid")
            .spacing([GRID_SPACING, GRID_SPACING])
            .show(ui, |ui| {
                for row in 0..gs {
                    for col in 0..gs {
                        let idx = row * gs + col;
                        let size = egui::vec2(BUTTON_PX, BUTTON_PX);

                        let response = if let Some(tex) = &self.cells[idx].texture {
                            let img = egui::Image::new((tex.id(), size));
                            ui.add(egui::ImageButton::new(img))
                        } else {
                            ui.add_sized(size, egui::Button::new(""))
                        };

                        // Left-click → import
                        if response.clicked() {
                            self.load_image_into_cell(ctx, idx);
                        }

                        // Right-click → context menu
                        response.context_menu(|ui| {
                            if ui.button("Import image…").clicked() {
                                self.load_image_into_cell(ctx, idx);
                                ui.close_menu();
                            }
                            if ui.button("Clear cell").clicked() {
                                self.clear_cell(idx);
                                ui.close_menu();
                            }
                            if !self.cells[idx].source_file.is_empty() {
                                ui.separator();
                                ui.label(format!("Source: {}", self.cells[idx].source_file));
                            }
                        });

                        // Hover tooltip
                        response.on_hover_text(format!(
                            "Cell {idx}  (row {row}, col {col})\n\
                             Left-click: import image\n\
                             Right-click: context menu"
                        ));
                    }
                    ui.end_row();
                }
            });
    }
}

// ── Entry point ─────────────────────────────────────────────────────────────

fn main() -> eframe::Result<()> {
    let default_app = AtlasApp::default();

    // Compute the ideal initial window size so there's no wasted space
    let grid_px = default_app.grid_size as f32 * (BUTTON_PX + GRID_SPACING) - GRID_SPACING;
    let top_panel_est = 108.0;
    let pad = 18.0;
    let init_w = (grid_px + pad).min(1100.0);
    let init_h = (grid_px + top_panel_est).min(1100.0);

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([init_w, init_h])
            .with_min_inner_size([300.0, 250.0]),
        ..Default::default()
    };

    eframe::run_native(
        "Atlas Maker",
        options,
        Box::new(|_cc| Box::new(default_app)),
    )
}