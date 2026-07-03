use eframe::egui;
use egui::{ColorImage, TextureHandle, TextureOptions};
use image::RgbaImage;
use serde::Serialize;

const GRID_SIZE: usize = 16;
const CELL_SIZE: u32 = 64; // pixels per cell in the exported atlas
const BUTTON_PX: f32 = 32.0; // on-screen size of each grid button

#[derive(Serialize)]
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

#[derive(Serialize)]
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

struct AtlasApp {
    cells: Vec<Cell>,
    status: String,
}

impl Default for AtlasApp {
    fn default() -> Self {
        Self {
            cells: (0..GRID_SIZE * GRID_SIZE).map(|_| Cell::default()).collect(),
            status: "Click any cell to import an image. Click \"Save Atlas\" when done.".into(),
        }
    }
}

impl AtlasApp {
    fn load_image_into_cell(&mut self, ctx: &egui::Context, idx: usize) {
        if let Some(path) = rfd::FileDialog::new()
            .add_filter("Images", &["png", "jpg", "jpeg", "bmp", "gif", "tga"])
            .pick_file()
        {
            match image::open(&path) {
                Ok(img) => {
                    let resized = img
                        .resize_exact(CELL_SIZE, CELL_SIZE, image::imageops::FilterType::Lanczos3)
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

    fn save_atlas(&mut self) {
        let Some(save_path) = rfd::FileDialog::new()
            .set_file_name("atlas.png")
            .add_filter("PNG", &["png"])
            .save_file()
        else {
            return;
        };

        let atlas_width = CELL_SIZE * GRID_SIZE as u32;
        let atlas_height = CELL_SIZE * GRID_SIZE as u32;
        let mut atlas_img: RgbaImage = RgbaImage::new(atlas_width, atlas_height);
        let mut entries = Vec::new();

        for row in 0..GRID_SIZE {
            for col in 0..GRID_SIZE {
                let idx = row * GRID_SIZE + col;
                let x = col as u32 * CELL_SIZE;
                let y = row as u32 * CELL_SIZE;
                if let Some(cell_img) = &self.cells[idx].image {
                    image::imageops::overlay(&mut atlas_img, cell_img, x as i64, y as i64);
                    entries.push(AtlasEntry {
                        index: idx,
                        row,
                        col,
                        x,
                        y,
                        width: CELL_SIZE,
                        height: CELL_SIZE,
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
            cell_size: CELL_SIZE,
            grid_size: GRID_SIZE,
            atlas_width,
            atlas_height,
            entries,
        };

        match serde_json::to_string_pretty(&data) {
            Ok(json_str) => {
                if let Err(e) = std::fs::write(&json_path, json_str) {
                    self.status = format!("PNG saved, but JSON failed: {e}");
                } else {
                    self.status =
                        format!("Saved atlas to {} and {}", save_path.display(), json_path.display());
                }
            }
            Err(e) => {
                self.status = format!("Failed to serialize JSON: {e}");
            }
        }
    }
}

impl eframe::App for AtlasApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        egui::TopBottomPanel::top("top").show(ctx, |ui| {
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                ui.heading("Texture Atlas Creator (16 x 16)");
                if ui.button("💾 Save Atlas").clicked() {
                    self.save_atlas();
                }
                if ui.button("🗑 Clear All").clicked() {
                    self.cells = (0..GRID_SIZE * GRID_SIZE).map(|_| Cell::default()).collect();
                    self.status = "Cleared all cells".into();
                }
            });
            ui.label(&self.status);
            ui.add_space(4.0);
        });

        egui::CentralPanel::default().show(ctx, |ui| {
            egui::ScrollArea::both().show(ui, |ui| {
                egui::Grid::new("atlas_grid")
                    .spacing([2.0, 2.0])
                    .show(ui, |ui| {
                        for row in 0..GRID_SIZE {
                            for col in 0..GRID_SIZE {
                                let idx = row * GRID_SIZE + col;
                                let size = egui::vec2(BUTTON_PX, BUTTON_PX);

                                let response = if let Some(tex) = &self.cells[idx].texture {
                                    let img = egui::Image::new((tex.id(), size));
                                    ui.add(egui::ImageButton::new(img))
                                } else {
                                    ui.add_sized(size, egui::Button::new(""))
                                };

                                if response.clicked() {
                                    self.load_image_into_cell(ctx, idx);
                                }
                                if response.secondary_clicked() {
                                    self.clear_cell(idx);
                                }
                                response.on_hover_text(format!(
                                    "Cell {idx} (row {row}, col {col})\nLeft-click: import image\nRight-click: clear"
                                ));
                            }
                            ui.end_row();
                        }
                    });
            });
        });
    }
}

fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size([760.0, 820.0]),
        ..Default::default()
    };
    eframe::run_native(
        "Atlas Maker",
        options,
        Box::new(|_cc| Box::new(AtlasApp::default())),
    )
}
