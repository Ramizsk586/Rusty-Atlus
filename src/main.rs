use eframe::egui;
use egui::{Color32, ColorImage, CornerRadius, Stroke, TextureHandle, TextureOptions};
use image::{Rgba, RgbaImage};
use serde::{Deserialize, Serialize};

// ── Theme palette (Catppuccin Mocha inspired) ──────────────────────────────
const C_BG: Color32 = Color32::from_rgb(17, 17, 27); // crust
const C_MANTLE: Color32 = Color32::from_rgb(24, 24, 37); // mantle
const C_SURFACE0: Color32 = Color32::from_rgb(49, 50, 68); // surface0
const C_SURFACE1: Color32 = Color32::from_rgb(69, 71, 90); // surface1
const C_SURFACE2: Color32 = Color32::from_rgb(88, 91, 112); // surface2
const C_TEXT: Color32 = Color32::from_rgb(205, 214, 244); // text
const C_SUBTEXT: Color32 = Color32::from_rgb(108, 112, 134); // subtext0
const C_BLUE: Color32 = Color32::from_rgb(137, 180, 250);
const C_GREEN: Color32 = Color32::from_rgb(166, 227, 161);
const C_RED: Color32 = Color32::from_rgb(243, 139, 168);
const C_YELLOW: Color32 = Color32::from_rgb(249, 226, 175);
const C_MAUVE: Color32 = Color32::from_rgb(203, 166, 247);
const C_TEAL: Color32 = Color32::from_rgb(148, 226, 213);

const TARGET_CELL_PX: f32 = 40.0; // used for window sizing only
const GRID_SPACING: f32 = 1.5;
const CELL_ROUND: u8 = 3;

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

// A saved, named texture that shows up in the "block types" library and can
// be re-loaded into the pixel canvas for further editing.
struct BlockType {
    name: String,
    image: RgbaImage,
    texture: Option<TextureHandle>,
}

#[derive(Clone, Copy, PartialEq, Eq, Default)]
enum PreviewShape {
    Wall,
    #[default]
    Cube,
    Cross,
    Torch,
}

impl PreviewShape {
    fn label(&self) -> &'static str {
        match self {
            PreviewShape::Wall => "Wall (X-Y)",
            PreviewShape::Cube => "Cube",
            PreviewShape::Cross => "Cross / X",
            PreviewShape::Torch => "Torch",
        }
    }
}

// ── Texture workspace ────────────────────────────────────────────────────────

struct TextureWorkspace {
    canvas: RgbaImage,
    texture: Option<TextureHandle>,
    checker_texture: Option<TextureHandle>,
    brush_color: Color32,
    tool: Tool,
    canvas_size_setting: usize, // 8, 16, 32, 64
    zoom: f32,
    show_grid: bool,
    mirror_x: bool,
    mirror_y: bool,
    // Undo / Redo
    undo_stack: Vec<RgbaImage>,
    redo_stack: Vec<RgbaImage>,
    // Pixel coord under cursor
    cursor_pixel: Option<(u32, u32)>,
    // Last painted pixel (for drag interpolation)
    last_pixel: Option<(u32, u32)>,
}

const MAX_UNDO: usize = 50;

impl Default for TextureWorkspace {
    fn default() -> Self {
        Self {
            canvas: RgbaImage::from_pixel(16, 16, Rgba([0, 0, 0, 0])),
            texture: None,
            checker_texture: None,
            brush_color: Color32::from_rgb(205, 214, 244),
            tool: Tool::Brush,
            canvas_size_setting: 16,
            zoom: 1.0,
            show_grid: true,
            mirror_x: false,
            mirror_y: false,
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
            cursor_pixel: None,
            last_pixel: None,
        }
    }
}

impl TextureWorkspace {
    fn ensure_texture(&mut self, ctx: &egui::Context) {
        let color_img = ColorImage::from_rgba_unmultiplied(
            [self.canvas.width() as usize, self.canvas.height() as usize],
            self.canvas.as_raw(),
        );
        if let Some(tex) = &mut self.texture {
            tex.set(color_img, TextureOptions::NEAREST);
        } else {
            self.texture = Some(ctx.load_texture("tex_canvas", color_img, TextureOptions::NEAREST));
        }
    }

    fn ensure_checker(&mut self, ctx: &egui::Context) {
        if self.checker_texture.is_some() {
            return;
        }
        let sz = 64;
        let mut img = RgbaImage::new(sz, sz);
        let check = 8;
        for y in 0..sz {
            for x in 0..sz {
                let light = ((x / check) + (y / check)) % 2 == 0;
                let v = if light { 45 } else { 35 };
                img.put_pixel(x, y, Rgba([v, v, v, 255]));
            }
        }
        let color_img = ColorImage::from_rgba_unmultiplied(
            [sz as usize, sz as usize],
            img.as_raw(),
        );
        self.checker_texture = Some(
            ctx.load_texture("checker", color_img, TextureOptions::NEAREST),
        );
    }

    fn push_undo(&mut self) {
        self.undo_stack.push(self.canvas.clone());
        if self.undo_stack.len() > MAX_UNDO {
            self.undo_stack.remove(0);
        }
        self.redo_stack.clear();
    }

    fn undo(&mut self, ctx: &egui::Context) {
        if let Some(prev) = self.undo_stack.pop() {
            self.redo_stack.push(self.canvas.clone());
            self.canvas = prev;
            self.texture = None;
            self.ensure_texture(ctx);
        }
    }

    fn redo(&mut self, ctx: &egui::Context) {
        if let Some(next) = self.redo_stack.pop() {
            self.undo_stack.push(self.canvas.clone());
            self.canvas = next;
            self.texture = None;
            self.ensure_texture(ctx);
        }
    }

    /// Colors exactly the single target pixel (plus mirrored counterparts, if
    /// mirroring is on). No radius, no falloff — one click, one pixel.
    fn stamp(&mut self, cx: u32, cy: u32) {
        let (rr, gg, bb, aa) = match self.tool {
            Tool::Brush => (self.brush_color.r(), self.brush_color.g(), self.brush_color.b(), self.brush_color.a()),
            Tool::Eraser => (0, 0, 0, 0),
            _ => return,
        };
        let w = self.canvas.width() as i32;
        let h = self.canvas.height() as i32;

        let mut positions: Vec<(i32, i32)> = vec![(cx as i32, cy as i32)];
        if self.mirror_x || self.mirror_y {
            let mx = w - 1 - cx as i32;
            let my = h - 1 - cy as i32;
            if self.mirror_x { positions.push((mx, cy as i32)); }
            if self.mirror_y { positions.push((cx as i32, my)); }
            if self.mirror_x && self.mirror_y { positions.push((mx, my)); }
        }

        for (x, y) in positions {
            if x >= 0 && y >= 0 && x < w && y < h {
                let pixel = self.canvas.get_pixel_mut(x as u32, y as u32);
                *pixel = Rgba([rr, gg, bb, aa]);
            }
        }
    }

    /// Bresenham line interpolation between two pixels for smooth dragging
    fn stamp_line(&mut self, x0: u32, y0: u32, x1: u32, y1: u32) {
        let mut x = x0 as i32;
        let mut y = y0 as i32;
        let dx = (x1 as i32 - x).abs();
        let dy = (y1 as i32 - y).abs();
        let sx = if x < x1 as i32 { 1 } else { -1 };
        let sy = if y < y1 as i32 { 1 } else { -1 };
        let mut err = dx - dy;

        loop {
            self.stamp(x as u32, y as u32);
            if x == x1 as i32 && y == y1 as i32 {
                break;
            }
            let e2 = 2 * err;
            if e2 > -dy {
                err -= dy;
                x += sx;
            }
            if e2 < dx {
                err += dx;
                y += sy;
            }
        }
    }

    /// Flood-fill a region
    fn flood_fill(&mut self, start_x: u32, start_y: u32) {
        let w = self.canvas.width();
        let h = self.canvas.height();
        if start_x >= w || start_y >= h {
            return;
        }
        let target = *self.canvas.get_pixel(start_x, start_y);
        let fill = Rgba([
            self.brush_color.r(),
            self.brush_color.g(),
            self.brush_color.b(),
            self.brush_color.a(),
        ]);
        if target == fill {
            return;
        }

        let mut stack = vec![(start_x, start_y)];
        let mut visited = std::collections::HashSet::new();

        while let Some((x, y)) = stack.pop() {
            if x >= w || y >= h {
                continue;
            }
            let key = (x, y);
            if visited.contains(&key) {
                continue;
            }
            visited.insert(key);
let px = *self.canvas.get_pixel(x, y);
                                     if px.0 != target.0 {
                continue;
            }
            *self.canvas.get_pixel_mut(x, y) = fill;
            stack.push((x.wrapping_add(1), y));
            stack.push((x.wrapping_sub(1), y));
            stack.push((x, y.wrapping_add(1)));
            stack.push((x, y.wrapping_sub(1)));
        }
    }

    /// Pick color from canvas at position
    fn pick_color(&mut self, x: u32, y: u32) {
        if x < self.canvas.width() && y < self.canvas.height() {
            let px = self.canvas.get_pixel(x, y);
            self.brush_color = Color32::from_rgba_unmultiplied(px[0], px[1], px[2], px[3]);
        }
    }

    fn resize_canvas(&mut self, new_size: usize, ctx: &egui::Context) {
        self.push_undo();
        let old = self.canvas.clone();
        self.canvas = RgbaImage::new(new_size as u32, new_size as u32);
        // Copy overlapping region
        let copy_w = old.width().min(new_size as u32);
        let copy_h = old.height().min(new_size as u32);
        for y in 0..copy_h {
            for x in 0..copy_w {
                *self.canvas.get_pixel_mut(x, y) = *old.get_pixel(x, y);
            }
        }
        self.canvas_size_setting = new_size;
        self.texture = None;
        self.ensure_texture(ctx);
    }

    fn canvas_size(&self) -> egui::Vec2 {
        egui::vec2(self.canvas.width() as f32, self.canvas.height() as f32)
    }
}

// ── Status type ─────────────────────────────────────────────────────────────

#[derive(Clone, Copy, PartialEq)]
enum StatusKind {
    Info,
    Success,
    Error,
}

// ── Texture tools ───────────────────────────────────────────────────────────

#[derive(Clone, Copy, PartialEq, Eq, Default)]
enum Tool {
    #[default]
    Brush,
    Eraser,
    Fill,
    Eyedropper,
}

impl Tool {
    fn label(&self) -> &'static str {
        match self {
            Tool::Brush => "Brush",
            Tool::Eraser => "Eraser",
            Tool::Fill => "Fill",
            Tool::Eyedropper => "Pick",
        }
    }

    fn icon(&self) -> &'static str {
        match self {
            Tool::Brush => "\u{1f58c}",
            Tool::Eraser => "\u{1f9f9}",
            Tool::Fill => "\u{1f9a7}",
            Tool::Eyedropper => "\u{1f4a7}",
        }
    }
}

#[derive(Clone, Copy, PartialEq)]
enum AppMode {
    Atlas,
    TextureCreator,
}

// ── App ─────────────────────────────────────────────────────────────────────

struct AtlasApp {
    cells: Vec<Cell>,
    grid_size: usize,
    cell_size: u32,
    status: String,
    status_kind: StatusKind,
    last_auto_size: Option<egui::Vec2>,
    mode: AppMode,
    texture: TextureWorkspace,
    block_types: Vec<BlockType>,
    show_preview_window: bool,
    preview_shape: PreviewShape,
    preview_yaw: f32,
    preview_pitch: f32,
    show_custom_color_window: bool,
    blocking_window_rects: Vec<egui::Rect>,
}

impl Default for AtlasApp {
    fn default() -> Self {
        let gs = 10;
        Self {
            cells: (0..gs * gs).map(|_| Cell::default()).collect(),
            grid_size: gs,
            cell_size: 64,
            status: "Left-click a cell to import an image".into(),
            status_kind: StatusKind::Info,
            last_auto_size: None,
            mode: AppMode::Atlas,
            texture: TextureWorkspace::default(),
            block_types: Vec::new(),
            show_preview_window: false,
            preview_shape: PreviewShape::Cube,
            preview_yaw: 0.6,
            preview_pitch: 0.5,
            show_custom_color_window: false,
            blocking_window_rects: Vec::new(),
        }
    }
}

// ── Helpers ─────────────────────────────────────────────────────────────────

fn apply_theme(ctx: &egui::Context) {
    let mut v = egui::Visuals::dark();
    v.panel_fill = C_MANTLE;
    v.window_fill = C_MANTLE;
    v.override_text_color = Some(C_TEXT);
    v.hyperlink_color = C_BLUE;
    v.faint_bg_color = C_SURFACE0;
    v.extreme_bg_color = C_BG;
    v.code_bg_color = C_SURFACE0;
    v.selection.bg_fill = C_BLUE.gamma_multiply(0.3);
    v.selection.stroke = Stroke::new(1.0, C_BLUE);
    let mut style: egui::Style = (*ctx.style()).clone();
    style.spacing.scroll.bar_width = 10.0;
    ctx.set_style(style);
    ctx.set_visuals(v);
}

fn accent_button(ui: &mut egui::Ui, label: &str, fill: Color32, text_col: Color32) -> bool {
    let btn = egui::Button::new(
        egui::RichText::new(label).color(text_col).size(13.0),
    )
    .fill(fill)
    .corner_radius(CornerRadius::same(5));
    ui.add(btn).clicked()
}

fn filled_count(cells: &[Cell]) -> usize {
    cells.iter().filter(|c| c.image.is_some()).count()
}

// ── Core logic ──────────────────────────────────────────────────────────────

impl AtlasApp {
    fn set_status(&mut self, msg: impl Into<String>, kind: StatusKind) {
        self.status = msg.into();
        self.status_kind = kind;
    }

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
        self.last_auto_size = None;
        self.set_status(
            format!("Grid resized to {new_size} x {new_size}"),
            StatusKind::Success,
        );
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
                        TextureOptions::NEAREST,
                    );

                    let src_name = path
                        .file_name()
                        .map(|s| s.to_string_lossy().to_string())
                        .unwrap_or_default();
                    let r = idx / self.grid_size;
                    let c = idx % self.grid_size;

                    let cell = &mut self.cells[idx];
                    cell.image = Some(resized);
                    cell.texture = Some(texture);
                    cell.source_file = src_name.clone();

                    self.set_status(
                        format!("Loaded '{}' into cell [{r} , {c}]", src_name),
                        StatusKind::Success,
                    );
                }
                Err(e) => {
                    self.set_status(format!("Failed to load image: {e}"), StatusKind::Error);
                }
            }
        }
    }

    fn clear_cell(&mut self, idx: usize) {
        self.cells[idx] = Cell::default();
        self.set_status(format!("Cleared cell {}", idx), StatusKind::Info);
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
            self.set_status(format!("Failed to save PNG: {e}"), StatusKind::Error);
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
                    self.set_status(format!("PNG saved, JSON write failed: {e}"), StatusKind::Error);
                } else {
                    self.set_status(
                        format!(
                            "Saved atlas  ->  {}  &  {}",
                            save_path.display(),
                            json_path.display()
                        ),
                        StatusKind::Success,
                    );
                }
            }
            Err(e) => {
                self.set_status(format!("Failed to serialise JSON: {e}"), StatusKind::Error);
            }
        }
    }

    // ── Load ────────────────────────────────────────────────────────────────

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
                self.set_status(format!("Failed to read file: {e}"), StatusKind::Error);
                return;
            }
        };

        let data: AtlasData = match serde_json::from_str(&json_str) {
            Ok(d) => d,
            Err(e) => {
                self.set_status(format!("Failed to parse atlas JSON: {e}"), StatusKind::Error);
                return;
            }
        };

        let png_path = path.with_extension("png");
        let atlas_img = match image::open(&png_path) {
            Ok(i) => i.to_rgba8(),
            Err(e) => {
                self.set_status(
                    format!("Could not open atlas image '{}': {e}", png_path.display()),
                    StatusKind::Error,
                );
                return;
            }
        };

        self.cell_size = data.cell_size;
        self.resize_grid(data.grid_size);

        let mut loaded = 0usize;
        for entry in &data.entries {
            if entry.row >= self.grid_size || entry.col >= self.grid_size {
                continue;
            }
            let idx = entry.row * self.grid_size + entry.col;

            let w = entry.width.min(atlas_img.width().saturating_sub(entry.x));
            let h = entry.height.min(atlas_img.height().saturating_sub(entry.y));
            if w == 0 || h == 0 {
                continue;
            }

            let sub = image::imageops::crop_imm(&atlas_img, entry.x, entry.y, w, h).to_image();

            let color_image = ColorImage::from_rgba_unmultiplied(
                [sub.width() as usize, sub.height() as usize],
                sub.as_raw(),
            );
            let texture = ctx.load_texture(
                format!("cell_{idx}"),
                color_image,
                TextureOptions::NEAREST,
            );

            self.cells[idx].image = Some(sub);
            self.cells[idx].texture = Some(texture);
            self.cells[idx].source_file = entry.source_file.clone();
            loaded += 1;
        }

        self.set_status(
            format!(
                "Loaded '{}'  --  {}x{} grid,  {} tiles restored",
                path.file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_default(),
                data.grid_size,
                data.grid_size,
                loaded,
            ),
            StatusKind::Success,
        );
    }

    fn set_mode(&mut self, mode: AppMode) {
        if self.mode != mode {
            self.mode = mode;
            self.last_auto_size = None;
            match mode {
                AppMode::Atlas => {
                    self.set_status("Switched to Atlas Creator", StatusKind::Info);
                }
                AppMode::TextureCreator => {
                    self.set_status("Switched to Texture Creator  |  Shortcuts: B E G I  Ctrl+Z/Y  M X", StatusKind::Info);
                }
            }
        }
    }

    fn import_texture(&mut self, ctx: &egui::Context) {
        if let Some(path) = rfd::FileDialog::new()
            .add_filter("Images", &["png", "jpg", "jpeg", "bmp", "gif", "tga", "webp"])
            .pick_file()
        {
            match image::open(&path) {
                Ok(img) => {
                    let sz = self.texture.canvas_size_setting;
                    let resized = img
                        .resize_exact(
                            sz as u32,
                            sz as u32,
                            image::imageops::FilterType::Nearest,
                        )
                        .to_rgba8();
                    self.texture.push_undo();
                    self.texture.canvas = resized;
                    self.texture.texture = None;
                    self.texture.ensure_texture(ctx);
                    self.set_status(format!("{}x{} texture imported", sz, sz), StatusKind::Success);
                }
                Err(e) => {
                    self.set_status(format!("Failed to import texture: {e}"), StatusKind::Error);
                }
            }
        }
    }

    // ── Block types library ─────────────────────────────────────────────────

    fn save_current_as_block(&mut self, ctx: &egui::Context) {
        let image = self.texture.canvas.clone();
        let color_image = ColorImage::from_rgba_unmultiplied(
            [image.width() as usize, image.height() as usize],
            image.as_raw(),
        );
        let idx = self.block_types.len();
        let texture = ctx.load_texture(
            format!("blocktype_{idx}"),
            color_image,
            TextureOptions::NEAREST,
        );
        let name = format!("block_{}", idx + 1);
        self.block_types.push(BlockType {
            name: name.clone(),
            image,
            texture: Some(texture),
        });
        self.set_status(format!("Saved current texture as '{name}'"), StatusKind::Success);
    }

    fn load_block_into_canvas(&mut self, idx: usize, ctx: &egui::Context) {
        if let Some(block) = self.block_types.get(idx) {
            self.texture.push_undo();
            self.texture.canvas = block.image.clone();
            self.texture.canvas_size_setting = block.image.width() as usize;
            self.texture.texture = None;
            self.texture.ensure_texture(ctx);
            self.set_status(format!("Loaded block '{}' into canvas", block.name), StatusKind::Success);
        }
    }

    fn delete_block(&mut self, idx: usize) {
        if idx < self.block_types.len() {
            let name = self.block_types.remove(idx).name;
            self.set_status(format!("Deleted block '{name}'"), StatusKind::Info);
        }
    }

    fn save_texture(&mut self) {
        let sz = self.texture.canvas_size_setting;
        let Some(path) = rfd::FileDialog::new()
            .set_file_name(format!("texture_{}x{}.png", sz, sz))
            .add_filter("PNG", &["png"])
            .save_file()
        else { return };

        if let Err(e) = self.texture.canvas.save(&path) {
            self.set_status(format!("Failed to save texture: {e}"), StatusKind::Error);
        } else {
            self.set_status(
                format!("Texture saved  ->  {}", path.display()),
                StatusKind::Success,
            );
        }
    }

    // ── Auto-resize ─────────────────────────────────────────────────────────

    fn auto_resize(&mut self, ctx: &egui::Context) {
        if self.mode != AppMode::Atlas {
            return;
        }
        let grid_px = self.grid_size as f32 * (TARGET_CELL_PX + GRID_SPACING) - GRID_SPACING;
        let panel_margin_h = 20.0;
        let panel_margin_v = 12.0;
        let top_h = 120.0;
        let bottom_h = 34.0;

        let ideal_w = grid_px + panel_margin_h;
        let ideal_h = grid_px + top_h + bottom_h + panel_margin_v;

        let max_dim = 1100.0;
        let target = egui::vec2(ideal_w.min(max_dim), ideal_h.min(max_dim));

        if self.last_auto_size != Some(target) {
            ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(target));
            self.last_auto_size = Some(target);
        }
    }

    fn needs_scroll(&self, avail_w: f32, avail_h: f32) -> bool {
        let gs = self.grid_size as f32;
        let spacing_total = (gs - 1.0) * GRID_SPACING;
        let cell_sz = ((avail_w - spacing_total) / gs).max(4.0);
        let grid_px = cell_sz * gs + spacing_total;
        grid_px > avail_h || grid_px > avail_w
    }

    // ── Keyboard shortcuts for texture mode ─────────────────────────────────

    fn handle_texture_shortcuts(&mut self, ctx: &egui::Context) {
        ctx.input(|i| {
            for key in ['b', 'e', 'g', 'i'] {
                if let Some(k) = egui::Key::from_name(&key.to_string()) {
                    if i.key_pressed(k) {
                        self.texture.tool = match key {
                            'b' => Tool::Brush,
                            'e' => Tool::Eraser,
                            'g' => Tool::Fill,
                            'i' => Tool::Eyedropper,
                            _ => self.texture.tool,
                        };
                    }
                }
            }
            if i.modifiers.command && i.key_pressed(egui::Key::Z) {
                if i.modifiers.shift {
                    self.texture.redo(ctx);
                } else {
                    self.texture.undo(ctx);
                }
            }
            if i.modifiers.command && i.key_pressed(egui::Key::Y) {
                self.texture.redo(ctx);
            }
            if i.key_pressed(egui::Key::M) {
                self.texture.mirror_x = !self.texture.mirror_x;
            }
            if i.key_pressed(egui::Key::X) && !i.modifiers.command {
                self.texture.mirror_y = !self.texture.mirror_y;
            }
            if i.key_pressed(egui::Key::H) {
                self.texture.show_grid = !self.texture.show_grid;
            }
            if i.key_pressed(egui::Key::Plus) || i.key_pressed(egui::Key::Equals) {
                self.texture.zoom = (self.texture.zoom + 0.25).min(4.0);
            }
            if i.key_pressed(egui::Key::Minus) {
                self.texture.zoom = (self.texture.zoom - 0.25).max(0.25);
            }
        });
    }
}

// ── egui App trait ──────────────────────────────────────────────────────────

impl eframe::App for AtlasApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.auto_resize(ctx);

        if self.mode == AppMode::TextureCreator {
            self.handle_texture_shortcuts(ctx);
        }

        // ── Top toolbar ─────────────────────────────────────────────────────
        egui::TopBottomPanel::top("top_bar")
            .frame(egui::Frame {
                fill: C_BG,
                inner_margin: egui::Margin::symmetric(14, 0),
                ..Default::default()
            })
            .show(ctx, |ui| {
                // -- Row 1 : mode tabs + buttons --
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    // Styled tabs
                    for (mode, label) in [(AppMode::Atlas, "Atlas"), (AppMode::TextureCreator, "Texture")] {
                        let is_active = self.mode == mode;
                        let (fill, text_col) = if is_active {
                            (C_BLUE.gamma_multiply(0.2), C_BLUE)
                        } else {
                            (C_SURFACE0, C_SUBTEXT)
                        };
                        let btn = egui::Button::new(
                                egui::RichText::new(label).color(text_col).size(13.0),
                            )
                            .fill(fill)
                            .corner_radius(CornerRadius::same(5));
                            if ui.add(btn).clicked() {
                            self.set_mode(mode);
                        }
                        ui.add_space(4.0);
                    }

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        match self.mode {
                            AppMode::Atlas => {
                                if accent_button(ui, "  Clear All  ", C_SURFACE1, C_RED) {
                                    self.cells = (0..self.grid_size * self.grid_size)
                                        .map(|_| Cell::default())
                                        .collect();
                                    self.set_status("Cleared all cells", StatusKind::Info);
                                }
                                if accent_button(ui, "  Save Atlas  ", C_BLUE.gamma_multiply(0.25), C_GREEN) {
                                    self.save_atlas();
                                }
                                if accent_button(ui, "  Load Atlas  ", C_BLUE.gamma_multiply(0.25), C_BLUE) {
                                    self.load_atlas(ctx);
                                }
                            }
                            AppMode::TextureCreator => {
                                // Undo/Redo buttons in toolbar
                                let undo_col = if self.texture.undo_stack.is_empty() { C_SURFACE0 } else { C_SURFACE1 };
                                let redo_col = if self.texture.redo_stack.is_empty() { C_SURFACE0 } else { C_SURFACE1 };
                                let undo_txt = if self.texture.undo_stack.is_empty() { C_SURFACE2 } else { C_TEXT };
                                let redo_txt = if self.texture.redo_stack.is_empty() { C_SURFACE2 } else { C_TEXT };

                                if accent_button(ui, " \u{21a9} ", undo_col, undo_txt) {
                                    self.texture.undo(ctx);
                                }
                                if accent_button(ui, " \u{21aa} ", redo_col, redo_txt) {
                                    self.texture.redo(ctx);
                                }
                            }
                        }
                    });
                });

                ui.add_space(6.0);

                // Separator line
                ui.painter().rect_filled(
                    egui::Rect::from_min_size(
                        ui.cursor().left_top(),
                        egui::vec2(ui.available_width(), 1.0),
                    ),
                    0.0,
                    C_SURFACE0,
                );
                ui.add_space(6.0);

                // -- Row 2 : settings --
                ui.horizontal(|ui| {
                    match self.mode {
                        AppMode::Atlas => {
ui.label(
                                egui::RichText::new("Grid").small().color(C_SUBTEXT),
                            );
                            let gs_before = self.grid_size;
                         ui.add(
                             egui::DragValue::new(&mut self.grid_size)
                                 .range(10..=64)
                                 .speed(0.5)
                                 .suffix(" x"),
                         );
                            if self.grid_size != gs_before {
                                self.resize_grid(self.grid_size);
                            }

                            ui.add_space(16.0);

                            // Cell pixel size
                            ui.label(
                                egui::RichText::new("Tile px").small().color(C_SUBTEXT),
                            );
                            ui.add(
                                egui::DragValue::new(&mut self.cell_size)
                                    .range(8..=512)
                                    .speed(4),
                            );

                            // Tile count badge
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                let filled = filled_count(&self.cells);
                                let total = self.grid_size * self.grid_size;

                                let label = format!("{filled} / {total}");
                                let font = egui::FontId::proportional(12.0);
                                let text_w = ui.fonts(|f| f.layout_no_wrap(
                                    label.clone(),
                                    font.clone(),
                                    C_TEXT,
                                 ).size().x);

                                let badge_w = text_w + 22.0;
                                let badge_h = 22.0;

                                let (rect, _resp) = ui.allocate_exact_size(
                                    egui::vec2(badge_w, badge_h),
                                    egui::Sense::hover(),
                                );

                                let painter = ui.painter();
                                painter.rect_filled(
                                    rect,
                                    CornerRadius::same((badge_h * 0.5) as u8),
                                    C_SURFACE0,
                                );

                                if filled > 0 && total > 0 {
                                    let frac = filled as f32 / total as f32;
                                    let fill_w = (rect.width() - 4.0) * frac;
                                    let fill_rect = egui::Rect::from_min_size(
                                        egui::pos2(rect.min.x + 2.0, rect.min.y + 2.0),
                                        egui::vec2(fill_w, rect.height() - 4.0),
                                    );
                                    painter.rect_filled(
                                        fill_rect,
                                        CornerRadius::same(((badge_h - 4.0) * 0.5) as u8),
                                        C_BLUE.gamma_multiply(0.25),
                                    );
                                }

                                let filled_str = filled.to_string();
                                let filled_w = ui.fonts(|f| f.layout_no_wrap(
                                    filled_str.clone(),
                                    font.clone(),
                                    C_BLUE,
                                 ).size().x);

                                let separator_w = ui.fonts(|f| f.layout_no_wrap(
                                    " / ".to_string(),
                                    font.clone(),
                                    C_SUBTEXT,
                                 ).size().x);

                                let mut x = rect.center().x - (text_w * 0.5);
                                let y = rect.center().y;

                                painter.text(
                                    egui::pos2(x + filled_w * 0.5, y),
                                    egui::Align2::CENTER_CENTER,
                                    &filled_str,
                                    font.clone(),
                                    C_BLUE,
                                );
                                x += filled_w;

                                painter.text(
                                    egui::pos2(x + separator_w * 0.5, y),
                                    egui::Align2::CENTER_CENTER,
                                    " / ",
                                    font.clone(),
                                    C_SUBTEXT,
                                );
                                x += separator_w;

                                let total_str = total.to_string();
                                painter.text(
                                    egui::pos2(x + text_w - filled_w - separator_w * 0.5, y),
                                    egui::Align2::CENTER_CENTER,
                                    &total_str,
                                    font.clone(),
                                    C_SUBTEXT,
                                );
                            });
                        }
                        AppMode::TextureCreator => {
                            // Quick info bar for texture mode
                            ui.label(
                                egui::RichText::new("Pixel Art Studio")
                                    .size(12.0)
                                    .color(C_SUBTEXT),
                            );
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                // Zoom controls
                                ui.label(
                                    egui::RichText::new(format!("{:.0}%", self.texture.zoom * 100.0))
                                        .size(11.0)
                                        .color(C_SUBTEXT),
                                );
                                ui.add_space(4.0);
                                if accent_button(ui, "-", C_SURFACE0, C_SUBTEXT) {
                                    self.texture.zoom = (self.texture.zoom - 0.25).max(0.25);
                                }
                                if accent_button(ui, "+", C_SURFACE0, C_SUBTEXT) {
                                    self.texture.zoom = (self.texture.zoom + 0.25).min(4.0);
                                }
                                ui.add_space(8.0);

                                // Grid toggle
                                let grid_txt_col = if self.texture.show_grid { C_TEAL } else { C_SUBTEXT };
                                let grid_fill = if self.texture.show_grid { C_TEAL.gamma_multiply(0.15) } else { C_SURFACE0 };
                                if accent_button(ui, " Grid ", grid_fill, grid_txt_col) {
                                    self.texture.show_grid = !self.texture.show_grid;
                                }
                                ui.add_space(4.0);

                                // Mirror toggles
                                let mx_txt = if self.texture.mirror_x { C_MAUVE } else { C_SUBTEXT };
                                let mx_fill = if self.texture.mirror_x { C_MAUVE.gamma_multiply(0.15) } else { C_SURFACE0 };
                                if accent_button(ui, " \u{2194} X ", mx_fill, mx_txt) {
                                    self.texture.mirror_x = !self.texture.mirror_x;
                                }
                                let my_txt = if self.texture.mirror_y { C_MAUVE } else { C_SUBTEXT };
                                let my_fill = if self.texture.mirror_y { C_MAUVE.gamma_multiply(0.15) } else { C_SURFACE0 };
                                if accent_button(ui, " \u{2195} Y ", my_fill, my_txt) {
                                    self.texture.mirror_y = !self.texture.mirror_y;
                                }
                                ui.add_space(8.0);

                                let preview_txt = if self.show_preview_window { C_TEAL } else { C_SUBTEXT };
                                let preview_fill = if self.show_preview_window { C_TEAL.gamma_multiply(0.15) } else { C_SURFACE0 };
                                if accent_button(ui, " \u{1f9ca} 3D Preview ", preview_fill, preview_txt) {
                                    self.show_preview_window = !self.show_preview_window;
                                }
                                ui.add_space(8.0);

                                // Color button — always visible here regardless of
                                // sidebar layout, with a live swatch of the current
                                // brush color baked into the button itself.
                                let color_btn_fill = if self.show_custom_color_window { C_TEAL.gamma_multiply(0.15) } else { C_SURFACE0 };
                                let color_btn_txt = if self.show_custom_color_window { C_TEAL } else { C_SUBTEXT };
                                let color_btn = egui::Button::new(
                                    egui::RichText::new(" \u{1f3a8} Color   ").color(color_btn_txt).size(13.0),
                                )
                                .fill(color_btn_fill)
                                .corner_radius(CornerRadius::same(5));
                                let color_resp = ui.add(color_btn);
                                let swatch_rect = egui::Rect::from_min_size(
                                    color_resp.rect.right_top() + egui::vec2(-16.0, 6.0),
                                    egui::vec2(10.0, 10.0),
                                );
                                ui.painter().rect_filled(swatch_rect, CornerRadius::same(2), self.texture.brush_color);
                                ui.painter().rect_stroke(
                                    swatch_rect,
                                    CornerRadius::same(2),
                                    Stroke::new(1.0, Color32::from_gray(140)),
                                    egui::StrokeKind::Middle,
                                );
                                if color_resp.clicked() {
                                    self.show_custom_color_window = !self.show_custom_color_window;
                                }
                            });
                        }
                    }
                });

                ui.add_space(8.0);
            });

        // ── Bottom status bar ───────────────────────────────────────────────
        let status_color = match self.status_kind {
            StatusKind::Success => C_GREEN,
            StatusKind::Error => C_RED,
            StatusKind::Info => C_SUBTEXT,
        };

        egui::TopBottomPanel::bottom("status_bar")
            .frame(egui::Frame {
                fill: C_BG,
                inner_margin: egui::Margin::symmetric(14, 6),
                ..Default::default()
            })
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    let dot_color = match self.status_kind {
                        StatusKind::Success => C_GREEN,
                        StatusKind::Error => C_RED,
                        StatusKind::Info => C_SURFACE2,
                    };
                    ui.painter().circle_filled(
                        ui.cursor().left_top() + egui::vec2(5.0, 7.0),
                        3.5,
                        dot_color,
                    );
                    ui.add_space(14.0);

                    ui.label(
                        egui::RichText::new(&self.status)
                            .size(12.0)
                            .color(status_color),
                    );

                    // Show pixel coords in texture mode
                    if self.mode == AppMode::TextureCreator {
                        if let Some((px, py)) = self.texture.cursor_pixel {
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                ui.label(
                                    egui::RichText::new(format!("({px}, {py})"))
                                        .size(11.0)
                                        .color(C_YELLOW),
                                );
                                ui.add_space(12.0);
                                // Color under cursor
                                if px < self.texture.canvas.width() && py < self.texture.canvas.height() {
                                    let c = self.texture.canvas.get_pixel(px, py);
                                    let color = Color32::from_rgba_unmultiplied(c[0], c[1], c[2], c[3]);
                                    let (rect, _) = ui.allocate_exact_size(
                                        egui::vec2(14.0, 14.0),
                                        egui::Sense::hover(),
                                    );
ui.painter().rect_filled(rect, CornerRadius::same(2), color);
                                        ui.painter().rect_stroke(rect, CornerRadius::same(2), Stroke::new(0.5, C_SURFACE2), egui::StrokeKind::Middle);
                                }
                            });
                        }
                    }
                });
            });

        // ── Central content ────────────────────────────────────────────────────
        egui::CentralPanel::default()
            .frame(egui::Frame {
                fill: C_BG,
                inner_margin: egui::Margin::symmetric(10, 6),
                ..Default::default()
            })
            .show(ctx, |ui| {
                match self.mode {
                    AppMode::Atlas => {
                        if self.needs_scroll(ui.available_width(), ui.available_height()) {
                            egui::ScrollArea::both().show(ui, |ui| {
                                self.show_grid(ui, ctx);
                            });
                        } else {
                            let spacing_total = (self.grid_size as f32 - 1.0) * GRID_SPACING;
                            let avail = ui.available_height();
                            let cell_sz = ((ui.available_width() - spacing_total) / self.grid_size as f32).max(4.0);
                            let grid_h = cell_sz * self.grid_size as f32 + spacing_total;
                            if avail > grid_h {
                                ui.add_space((avail - grid_h) * 0.5);
                            }
                            self.show_grid(ui, ctx);
                        }
                    }
                    AppMode::TextureCreator => {
                        // Prototype layout: left = canvas (light gray bg, black grid),
                        // right = white panels with black borders: "color" (top) + "block types" (bottom)
                        ui.horizontal(|ui| {
                            ui.set_min_height(ui.available_height());

                            // Left: canvas / grid area (60%)
                            ui.vertical_centered(|ui| {
                                ui.add_space(6.0);
                                self.show_texture_canvas(ui, ctx);
                            });

                            ui.add_space(1.0);

                            // Right panel: white-framed "color" + "block types" sections
                            ui.vertical(|ui| {
                                ui.set_width(200.0);
                                ui.set_min_height(ui.available_height());

                                egui::ScrollArea::vertical()
                                    .auto_shrink([false, false])
                                    .show(ui, |ui| {

                                // ── Color section (top ~50%) — WHITE bg, BLACK border ──
                                egui::Frame {
                                    fill: Color32::WHITE,
                                    inner_margin: egui::Margin::symmetric(10, 8),
                                    corner_radius: CornerRadius::same(0),
                                    stroke: Stroke::new(1.0, Color32::BLACK),
                                    ..Default::default()
                                }
                                .show(ui, |ui| {
                                    ui.set_min_height(ui.available_height() * 0.45);
                                    ui.vertical(|ui| {
                                        ui.label(
                                            egui::RichText::new("color")
                                                .size(14.0)
                                                .color(Color32::BLACK)
                                                .strong(),
                                        );
                                        ui.add_space(8.0);

                                        // Full color picker — embedded directly and
                                        // always visible, exactly the wheel + hex +
                                        // alpha slider widget, no extra click needed.
                                        egui::color_picker::color_picker_color32(
                                            ui,
                                            &mut self.texture.brush_color,
                                            egui::color_picker::Alpha::OnlyBlend,
                                        );

                                        ui.add_space(8.0);

                                        // Preset palette — a second, always-reliable way to
                                        // change the paint color (in case the native color
                                        // wheel popup is finicky on some platforms).
                                        ui.label(
                                            egui::RichText::new("palette")
                                                .size(11.0)
                                                .color(Color32::from_gray(80))
                                                .strong(),
                                        );
                                        ui.add_space(4.0);
                                        const PALETTE: [Color32; 12] = [
                                            Color32::from_rgb(0, 0, 0),
                                            Color32::from_rgb(255, 255, 255),
                                            Color32::from_rgb(136, 136, 136),
                                            Color32::from_rgb(200, 60, 60),
                                            Color32::from_rgb(230, 140, 50),
                                            Color32::from_rgb(230, 210, 60),
                                            Color32::from_rgb(90, 170, 80),
                                            Color32::from_rgb(60, 140, 90),
                                            Color32::from_rgb(70, 120, 200),
                                            Color32::from_rgb(120, 80, 200),
                                            Color32::from_rgb(140, 90, 50),
                                            Color32::from_rgb(90, 60, 40),
                                        ];
                                        egui::Grid::new("palette_grid")
                                            .spacing([4.0, 4.0])
                                            .show(ui, |ui| {
                                                for (i, &c) in PALETTE.iter().enumerate() {
                                                    let size = egui::vec2(18.0, 18.0);
                                                    let (rect, resp) =
                                                        ui.allocate_exact_size(size, egui::Sense::click());
                                                    let is_active = self.texture.brush_color == c;
                                                    ui.painter().rect_filled(rect, CornerRadius::same(2), c);
                                                    ui.painter().rect_stroke(
                                                        rect,
                                                        CornerRadius::same(2),
                                                        Stroke::new(if is_active { 2.0 } else { 1.0 }, if is_active { Color32::from_rgb(70, 120, 200) } else { Color32::from_gray(160) }),
                                                        egui::StrokeKind::Middle,
                                                    );
                                                    if resp.clicked() {
                                                        self.texture.brush_color = c;
                                                    }
                                                    if (i + 1) % 6 == 0 {
                                                        ui.end_row();
                                                    }
                                                }
                                            });
                                    });
                                });

                                ui.add_space(1.0);

                                // ── Block types section (bottom ~50%) — WHITE bg, BLACK border ──
                                egui::Frame {
                                    fill: Color32::WHITE,
                                    inner_margin: egui::Margin::symmetric(10, 8),
                                    corner_radius: CornerRadius::same(0),
                                    stroke: Stroke::new(1.0, Color32::BLACK),
                                    ..Default::default()
                                }
                                .show(ui, |ui| {
                                    ui.set_min_height(ui.available_height() * 0.45);
                                    ui.vertical(|ui| {
                                        ui.horizontal(|ui| {
                                            ui.label(
                                                egui::RichText::new("block types")
                                                    .size(14.0)
                                                    .color(Color32::BLACK)
                                                    .strong(),
                                            );
                                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                                if accent_button(ui, " + Save ", Color32::from_rgb(200, 240, 210), Color32::from_rgb(20, 120, 60)) {
                                                    self.save_current_as_block(ctx);
                                                }
                                            });
                                        });
                                        ui.add_space(6.0);

                                        // Library of saved block textures
                                        let mut load_idx: Option<usize> = None;
                                        let mut delete_idx: Option<usize> = None;
                                        if self.block_types.is_empty() {
                                            ui.label(
                                                egui::RichText::new("No blocks saved yet — paint a texture, then click \"+ Save\".")
                                                    .size(10.5)
                                                    .color(Color32::from_gray(130))
                                                    .italics(),
                                            );
                                        } else {
                                            egui::ScrollArea::vertical()
                                                .max_height(110.0)
                                                .show(ui, |ui| {
                                                    egui::Grid::new("block_types_grid")
                                                        .spacing([5.0, 5.0])
                                                        .show(ui, |ui| {
                                                            let mut col = 0;
                                                            for (i, block) in self.block_types.iter().enumerate() {
                                                                let size = egui::vec2(32.0, 32.0);
                                                                let resp = if let Some(tex) = &block.texture {
                                                                    ui.add(
                                                                        egui::ImageButton::new((tex.id(), size))
                                                                            .corner_radius(CornerRadius::same(2)),
                                                                    )
                                                                } else {
                                                                    ui.add_sized(size, egui::Button::new(""))
                                                                };
                                                                if resp.clicked() {
                                                                    load_idx = Some(i);
                                                                }
resp.on_hover_text(format!(
                                                                     "{}\nLeft-click: load into canvas\nRight-click: delete",
                                                                     block.name
                                                                 ))
                                                                     .context_menu(|ui| {
                                                                    if ui.button("Load into canvas").clicked() {
                                                                        load_idx = Some(i);
                                                                        ui.close_menu();
                                                                    }
                                                                    if ui.button("Delete").clicked() {
                                                                        delete_idx = Some(i);
                                                                        ui.close_menu();
                                                                    }
                                                                });
                                                                col += 1;
                                                                if col >= 4 {
                                                                    col = 0;
                                                                    ui.end_row();
                                                                }
                                                            }
                                                        });
                                                });
                                        }
                                        if let Some(i) = load_idx {
                                            self.load_block_into_canvas(i, ctx);
                                        }
                                        if let Some(i) = delete_idx {
                                            self.delete_block(i);
                                        }

                                        ui.add_space(8.0);
                                        ui.separator();
                                        ui.add_space(4.0);

                                        // Tool selection
                                        ui.label(
                                            egui::RichText::new("tool")
                                                .size(11.0)
                                                .color(Color32::from_gray(80))
                                                .strong(),
                                        );
                                        ui.add_space(4.0);

                                        egui::Grid::new("tool_grid")
                                            .spacing([4.0, 4.0])
                                            .show(ui, |ui| {
                                                for tool in [Tool::Brush, Tool::Eraser, Tool::Fill, Tool::Eyedropper] {
                                                    let is_active = self.texture.tool == tool;
                                                    let fill = if is_active { Color32::from_rgb(200, 220, 255) } else { Color32::from_gray(235) };
                                                    let txt = if is_active { Color32::from_rgb(30, 80, 180) } else { Color32::from_gray(60) };
                                                    let label = format!(" {} {} ", tool.icon(), tool.label());
                                                    if accent_button(ui, &label, fill, txt) {
                                                        self.texture.tool = tool;
                                                    }
                                                }
                                                ui.end_row();
                                            });

                                        ui.add_space(8.0);

                                        // Canvas size
                                        ui.label(
                                            egui::RichText::new("canvas")
                                                .size(11.0)
                                                .color(Color32::from_gray(80))
                                                .strong(),
                                        );
                                        ui.add_space(4.0);
                                        ui.horizontal(|ui| {
                                            for &sz in &[8usize, 16, 32, 64] {
                                                let is_active = self.texture.canvas_size_setting == sz;
                                                let fill = if is_active { Color32::from_rgb(200, 235, 220) } else { Color32::from_gray(235) };
                                                let txt = if is_active { Color32::from_rgb(20, 120, 80) } else { Color32::from_gray(100) };
                                                let label = format!("{}x{}", sz, sz);
                                                if accent_button(ui, &label, fill, txt) {
                                                    self.texture.resize_canvas(sz, ctx);
                                                    self.set_status(
                                                        format!("Canvas resized to {}x{}", sz, sz),
                                                        StatusKind::Success,
                                                    );
                                                }
                                            }
                                        });

                                        ui.add_space(12.0);

                                        // Action buttons
                                        ui.vertical_centered(|ui| {
                                            if accent_button(ui, "  Clear  ", Color32::from_rgb(240, 200, 200), Color32::from_rgb(180, 40, 40)) {
                                                self.texture.push_undo();
                                                let sz = self.texture.canvas_size_setting;
                                                self.texture.canvas = RgbaImage::new(sz as u32, sz as u32);
                                                self.texture.texture = None;
                                                self.texture.ensure_texture(ctx);
                                                self.set_status("Canvas cleared", StatusKind::Info);
                                            }
                                            ui.add_space(4.0);
                                            ui.horizontal(|ui| {
                                                if accent_button(ui, "  Import  ", Color32::from_rgb(200, 220, 255), Color32::from_rgb(30, 80, 180)) {
                                                    self.import_texture(ctx);
                                                }
                                                ui.add_space(4.0);
                                                if accent_button(ui, "  Save  ", Color32::from_rgb(200, 240, 210), Color32::from_rgb(20, 120, 60)) {
                                                    self.save_texture();
                                                }
                                            });
                                        });
                                    });
                                });
                            });
                        });
                        });
                    }
                }
            });

        // Recomputed below from whichever floating windows are open this frame;
        // used (with one frame of latency) to stop canvas painting from
        // "leaking" through when the user is dragging inside a window on top.
        self.blocking_window_rects.clear();

        // ── Custom color picker window ───────────────────────────────────────
        if self.show_custom_color_window {
            let mut open = self.show_custom_color_window;
            let mut color = self.texture.brush_color;

            let color_window_resp = egui::Window::new("Custom Color")
                .open(&mut open)
                .resizable(false)
                .default_size([260.0, 320.0])
                .show(ctx, |ui| {
                    egui::color_picker::color_picker_color32(
                        ui,
                        &mut color,
                        egui::color_picker::Alpha::OnlyBlend,
                    );
                    ui.add_space(10.0);
                    ui.separator();
                    ui.add_space(6.0);
                    ui.horizontal(|ui| {
                        ui.label("Hex:");
                        let mut hex = format!("#{:02X}{:02X}{:02X}", color.r(), color.g(), color.b());
                        let resp = ui.add(
                            egui::TextEdit::singleline(&mut hex).desired_width(90.0),
                        );
                        if resp.lost_focus() {
                            if let Some((r, g, b)) = parse_hex_color(&hex) {
                                color = Color32::from_rgba_unmultiplied(r, g, b, color.a());
                            }
                        }
                    });
                });

            if let Some(inner) = &color_window_resp {
                self.blocking_window_rects.push(inner.response.rect);
            }
            self.show_custom_color_window = open;
            self.texture.brush_color = color;
        }

        // ── 3D shape preview window (drag to rotate) ────────────────────────
        if self.show_preview_window {
            let mut still_open = self.show_preview_window;
            let tex_id = self.texture.texture.as_ref().map(|t| t.id());
            let mut shape = self.preview_shape;
            let mut yaw = self.preview_yaw;
            let mut pitch = self.preview_pitch;

            let preview_window_resp = egui::Window::new("3D Preview")
                .open(&mut still_open)
                .resizable(true)
                .default_size([420.0, 500.0])
                .min_size([320.0, 400.0])
                .show(ctx, |ui| {
                    ui.horizontal(|ui| {
                        for s in [PreviewShape::Wall, PreviewShape::Cube, PreviewShape::Cross, PreviewShape::Torch] {
                            let active = shape == s;
                            let fill = if active { C_BLUE.gamma_multiply(0.25) } else { C_SURFACE0 };
                            let txt = if active { C_BLUE } else { C_SUBTEXT };
                            if accent_button(ui, s.label(), fill, txt) {
                                shape = s;
                            }
                        }
                    });
                    ui.add_space(8.0);

                    // Preview canvas fills whatever space the (resizable) window gives it.
                    let avail = ui.available_size();
                    let side = (avail.x.min(avail.y - 30.0)).max(200.0);
                    let (rect, resp) = ui.allocate_exact_size(
                        egui::vec2(side, side),
                        egui::Sense::click_and_drag(),
                    );
                    let painter = ui.painter();
                    painter.rect_filled(rect, CornerRadius::same(6), Color32::BLACK);
                    painter.rect_stroke(
                        rect,
                        CornerRadius::same(6),
                        Stroke::new(1.0, Color32::from_gray(70)),
                        egui::StrokeKind::Middle,
                    );

                    let is_wall = shape == PreviewShape::Wall;

                    // Drag to orbit (not applicable to the flat wall view)
                    if !is_wall && resp.dragged() {
                        let d = resp.drag_delta();
                        yaw -= d.x * 0.012;
                        pitch = (pitch - d.y * 0.012).clamp(-1.35, 1.35);
                    }
                    // Slow auto-spin when idle, so it visibly reads as 3D even before touching it
                    if !is_wall && !resp.dragged() && !resp.hovered() {
                        yaw += ctx.input(|i| i.stable_dt) * 0.25;
                    }

                    let center = rect.center();
                    let scale = side * 0.32; // world-space half-extent 1.0 maps to ~32% of the box
                    if let Some(tid) = tex_id {
                        match shape {
                            PreviewShape::Wall => {
                                // Straight-on, undistorted view of the texture — exactly
                                // the pixels on the canvas, no rotation or shading, like
                                // looking directly at one flat wall / the X-Y plane.
                                let img_side = side * 0.82;
                                let img_rect = egui::Rect::from_center_size(center, egui::vec2(img_side, img_side));
                                egui::Image::new((tid, img_rect.size()))
                                    .paint_at(ui, img_rect);
                                painter.rect_stroke(
                                    img_rect,
                                    CornerRadius::same(0),
                                    Stroke::new(1.5, Color32::from_gray(90)),
                                    egui::StrokeKind::Middle,
                                );
                            }
                            PreviewShape::Cube => {
                                draw_cube3d(painter, tid, center, scale, (1.0, 1.0, 1.0), yaw, pitch, true, Color32::WHITE);
                            }
                            PreviewShape::Cross => {
                                draw_cross3d(painter, tid, center, scale, yaw, pitch);
                            }
                            PreviewShape::Torch => {
                                // A single tall block wearing the actual canvas texture —
                                // no separate floating "head" cube, just the torch's own
                                // pixel art wrapped around one elongated shape.
                                draw_cube3d(
                                    painter, tid, center, scale,
                                    (0.28, 1.0, 0.28), yaw, pitch, true, Color32::WHITE,
                                );
                            }
                        }
                    } else {
                        painter.text(
                            rect.center(),
                            egui::Align2::CENTER_CENTER,
                            "Paint or load a texture first",
                            egui::FontId::proportional(12.0),
                            Color32::from_gray(180),
                        );
                    }

                    ui.add_space(6.0);
                    let hint = if is_wall {
                        "Flat, undistorted view of the texture (the X-Y plane / one wall)."
                    } else {
                        "Drag inside the box to rotate. Uses the texture on the canvas."
                    };
                    ui.label(egui::RichText::new(hint).size(10.5).color(C_SUBTEXT));
                });

            if let Some(inner) = &preview_window_resp {
                self.blocking_window_rects.push(inner.response.rect);
            }
            self.show_preview_window = still_open;
            self.preview_shape = shape;
            self.preview_yaw = yaw;
            self.preview_pitch = pitch;
            if still_open {
                ctx.request_repaint(); // keep the idle auto-spin / drag animating smoothly
            }
        }
    }
}

// ── Real rotatable 3D rendering (orthographic projection + backface culling) ─

/// Parses "#RRGGBB", "RRGGBB", "#RGB", or "RGB" into (r, g, b). Returns None
/// for anything else so a bad hex string just leaves the color unchanged.
fn parse_hex_color(s: &str) -> Option<(u8, u8, u8)> {
    let s = s.trim().trim_start_matches('#');
    match s.len() {
        6 => {
            let r = u8::from_str_radix(&s[0..2], 16).ok()?;
            let g = u8::from_str_radix(&s[2..4], 16).ok()?;
            let b = u8::from_str_radix(&s[4..6], 16).ok()?;
            Some((r, g, b))
        }
        3 => {
            let r = u8::from_str_radix(&s[0..1].repeat(2), 16).ok()?;
            let g = u8::from_str_radix(&s[1..2].repeat(2), 16).ok()?;
            let b = u8::from_str_radix(&s[2..3].repeat(2), 16).ok()?;
            Some((r, g, b))
        }
        _ => None,
    }
}

#[derive(Clone, Copy)]
struct Vec3 {
    x: f32,
    y: f32,
    z: f32,
}

impl Vec3 {
    const fn new(x: f32, y: f32, z: f32) -> Self {
        Self { x, y, z }
    }
}

/// Rotate a point/normal by yaw (around Y) then pitch (around X).
fn rotate_yaw_pitch(v: Vec3, yaw: f32, pitch: f32) -> Vec3 {
    let (sy, cy) = yaw.sin_cos();
    let x1 = v.x * cy + v.z * sy;
    let z1 = -v.x * sy + v.z * cy;
    let y1 = v.y;

    let (sp, cp) = pitch.sin_cos();
    let y2 = y1 * cp - z1 * sp;
    let z2 = y1 * sp + z1 * cp;
    Vec3::new(x1, y2, z2)
}

/// Simple orthographic projection: X/Y become screen offsets, Z is dropped
/// (already used for depth/culling before this point).
fn project(v: Vec3, center: egui::Pos2, scale: f32) -> egui::Pos2 {
    egui::pos2(center.x + v.x * scale, center.y - v.y * scale)
}

fn textured_quad(
    painter: &egui::Painter,
    tex_id: egui::TextureId,
    pts: [egui::Pos2; 4],
    uvs: [egui::Pos2; 4],
    tint: Color32,
) {
    let mut mesh = egui::Mesh::with_texture(tex_id);
    for i in 0..4 {
        mesh.vertices.push(egui::epaint::Vertex {
            pos: pts[i],
            uv: uvs[i],
            color: tint,
        });
    }
    mesh.indices.extend_from_slice(&[0, 1, 2, 0, 2, 3]);
    painter.add(egui::Shape::mesh(mesh));
}

fn solid_quad(painter: &egui::Painter, pts: [egui::Pos2; 4], color: Color32) {
    painter.add(egui::Shape::convex_polygon(pts.to_vec(), color, Stroke::NONE));
}

/// Draws a real 3D box (any aspect ratio) that can be rotated with yaw/pitch,
/// using proper backface culling so exactly the camera-facing faces are drawn
/// and they never overlap incorrectly. If `textured` is false, `base_color`
/// is used as a flat-shaded fill instead of sampling the block texture.
fn draw_cube3d(
    painter: &egui::Painter,
    tex_id: egui::TextureId,
    screen_center: egui::Pos2,
    scale: f32,
    half: (f32, f32, f32),
    yaw: f32,
    pitch: f32,
    textured: bool,
    base_color: Color32,
) {
    let (hx, hy, hz) = half;
    let corners_local = [
        Vec3::new(-hx, -hy, -hz), // 0
        Vec3::new(hx, -hy, -hz),  // 1
        Vec3::new(hx, hy, -hz),   // 2
        Vec3::new(-hx, hy, -hz),  // 3
        Vec3::new(-hx, -hy, hz),  // 4
        Vec3::new(hx, -hy, hz),   // 5
        Vec3::new(hx, hy, hz),    // 6
        Vec3::new(-hx, hy, hz),   // 7
    ];
    let rotated: Vec<Vec3> = corners_local
        .iter()
        .map(|c| rotate_yaw_pitch(*c, yaw, pitch))
        .collect();
    let screen: Vec<egui::Pos2> = rotated.iter().map(|v| project(*v, screen_center, scale)).collect();

    let uv00 = egui::pos2(0.0, 0.0);
    let uv10 = egui::pos2(1.0, 0.0);
    let uv11 = egui::pos2(1.0, 1.0);
    let uv01 = egui::pos2(0.0, 1.0);
    let uvs = [uv00, uv10, uv11, uv01];

    // (corner indices in perimeter order, local outward normal)
    let faces: [([usize; 4], Vec3); 6] = [
        ([3, 2, 6, 7], Vec3::new(0.0, 1.0, 0.0)),  // top
        ([0, 1, 5, 4], Vec3::new(0.0, -1.0, 0.0)), // bottom
        ([4, 5, 6, 7], Vec3::new(0.0, 0.0, 1.0)),  // front
        ([1, 0, 3, 2], Vec3::new(0.0, 0.0, -1.0)), // back
        ([5, 1, 2, 6], Vec3::new(1.0, 0.0, 0.0)),  // right
        ([0, 4, 7, 3], Vec3::new(-1.0, 0.0, 0.0)), // left
    ];

    let light = Vec3::new(0.35, 0.8, 0.5);
    let light_len = (light.x * light.x + light.y * light.y + light.z * light.z).sqrt();

    for (idx, normal) in faces.iter() {
        let rn = rotate_yaw_pitch(*normal, yaw, pitch);
        if rn.z <= 0.02 {
            continue; // back-facing, camera looks toward -Z
        }
        let dot = ((rn.x * light.x + rn.y * light.y + rn.z * light.z) / light_len).max(0.0);
        let brightness = 0.5 + 0.5 * dot;

        let pts = [screen[idx[0]], screen[idx[1]], screen[idx[2]], screen[idx[3]]];
        if textured {
            let tint = Color32::from_gray((brightness * 255.0).clamp(0.0, 255.0) as u8);
            textured_quad(painter, tex_id, pts, uvs, tint);
        } else {
            solid_quad(painter, pts, base_color.gamma_multiply(brightness));
        }
    }
}

/// Draws a Minecraft-plant-style crossed pair of textured planes that
/// genuinely sit in 3D and rotate with the camera.
fn draw_cross3d(painter: &egui::Painter, tex_id: egui::TextureId, screen_center: egui::Pos2, scale: f32, yaw: f32, pitch: f32) {
    let hw = 0.7;
    let hy = 0.9;
    let uv00 = egui::pos2(0.0, 0.0);
    let uv10 = egui::pos2(1.0, 0.0);
    let uv11 = egui::pos2(1.0, 1.0);
    let uv01 = egui::pos2(0.0, 1.0);
    let uvs = [uv00, uv10, uv11, uv01];

    let plane_a_local = [
        Vec3::new(-hw, -hy, -hw),
        Vec3::new(hw, -hy, hw),
        Vec3::new(hw, hy, hw),
        Vec3::new(-hw, hy, -hw),
    ];
    let plane_b_local = [
        Vec3::new(-hw, -hy, hw),
        Vec3::new(hw, -hy, -hw),
        Vec3::new(hw, hy, -hw),
        Vec3::new(-hw, hy, hw),
    ];

    let mut planes = [plane_a_local, plane_b_local];
    // Depth-sort so the farther plane is painted first (basic painter's algorithm).
    let avg_z = |p: &[Vec3; 4]| -> f32 {
        p.iter().map(|c| rotate_yaw_pitch(*c, yaw, pitch).z).sum::<f32>() / 4.0
    };
    planes.sort_by(|a, b| avg_z(a).partial_cmp(&avg_z(b)).unwrap());

    for local in planes.iter() {
        let screen: Vec<egui::Pos2> = local
            .iter()
            .map(|c| project(rotate_yaw_pitch(*c, yaw, pitch), screen_center, scale))
            .collect();
        let pts = [screen[0], screen[1], screen[2], screen[3]];
        textured_quad(painter, tex_id, pts, uvs, Color32::WHITE);
    }
}

// ── Grid rendering ──────────────────────────────────────────────────────────


impl AtlasApp {
    fn show_grid(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let gs = self.grid_size;

        let avail_w = ui.available_width();
        let spacing_total = (gs as f32 - 1.0) * GRID_SPACING;
        let cell_sz = ((avail_w - spacing_total) / gs as f32).max(4.0);
        let size = egui::vec2(cell_sz, cell_sz);
        let plus_font = (cell_sz * 0.45).clamp(8.0, 20.0);

        egui::Grid::new("atlas_grid")
            .spacing([GRID_SPACING, GRID_SPACING])
            .show(ui, |ui| {
                for row in 0..gs {
                    for col in 0..gs {
                        let idx = row * gs + col;
                        let is_filled = self.cells[idx].texture.is_some();

                        let response = if is_filled {
                            if let Some(tex) = &self.cells[idx].texture {
                                let img = egui::Image::new((tex.id(), size))
                                    .corner_radius(CornerRadius::same(CELL_ROUND));
                                ui.add(img).interact(egui::Sense::click())
                            } else {
                                ui.add_sized(size, egui::Button::new(""))
                            }
                        } else {
                            ui.add_sized(
                                size,
                                egui::Button::new(
                                    egui::RichText::new("+")
                                        .color(C_SUBTEXT.gamma_multiply(0.35))
                                        .size(plus_font),
                                )
                                .fill(C_SURFACE0.gamma_multiply(0.6)),
                            )
                        };

                        // --- Hover highlight ---
                        if response.hovered() {
                            let painter = ui.painter();
                            let rect = response.rect;
                            if is_filled {
                                painter.rect_stroke(
                                    rect.expand(1.0),
                                    CornerRadius::same(CELL_ROUND.saturating_add(1)),
                                    Stroke::new(1.8, C_BLUE.gamma_multiply(0.7)),
                                    egui::StrokeKind::Middle,
                                );
                            } else {
                                painter.rect_filled(
                                    rect,
                                    CornerRadius::same(CELL_ROUND),
                                    C_SURFACE0,
                                );
                                painter.rect_stroke(
                                    rect,
                                    CornerRadius::same(CELL_ROUND),
                                    Stroke::new(1.5, C_BLUE.gamma_multiply(0.5)),
                                    egui::StrokeKind::Middle,
                                );
                            }
                        }

                        if is_filled && !response.hovered() {
                            ui.painter().rect_stroke(
                                response.rect,
                                CornerRadius::same(CELL_ROUND),
                                Stroke::new(0.8, C_SURFACE1),
                                egui::StrokeKind::Middle,
                            );
                        }

                        if response.clicked() {
                            self.load_image_into_cell(ctx, idx);
                        }

                        response.context_menu(|ui| {
                            ui.set_min_width(140.0);
                            if ui.button("Import image...").clicked() {
                                self.load_image_into_cell(ctx, idx);
                                ui.close_menu();
                            }
                            if is_filled
                                && ui.button("Clear cell").clicked()
                            {
                                self.clear_cell(idx);
                                ui.close_menu();
                            }
                            if is_filled {
                                ui.separator();
                                ui.label(
                                    egui::RichText::new(format!(
                                        "Source: {}",
                                        self.cells[idx].source_file
                                    ))
                                    .small()
                                    .color(C_SUBTEXT),
                                );
                                ui.label(
                                    egui::RichText::new(format!(
                                        "Pos: row {}, col {}",
                                        row, col
                                    ))
                                    .small()
                                    .color(C_SUBTEXT),
                                );
                            }
                        });

                        response.on_hover_text(format!(
                            "Cell {}  (row {}, col {})\nLeft-click: import\nRight-click: menu",
                            idx, row, col
                        ));
                    }
                    ui.end_row();
                }
            });
    }
}

// ── Texture editor rendering ────────────────────────────────────────────────

impl AtlasApp {
fn show_texture_canvas(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        self.texture.ensure_texture(ctx);
        self.texture.ensure_checker(ctx);

        let tex_id = self.texture.texture.as_ref().map(|t| t.id());
        let sz = self.texture.canvas_size_setting;

        if tex_id.is_some() {
            ui.label(
                egui::RichText::new(format!("{} × {} Pixel Art", sz, sz))
                    .size(13.0)
                    .color(C_SUBTEXT),
            );
            ui.add_space(6.0);
        }

        let tex_sz = self.texture.canvas_size();
        let avail = ui.available_size();
        let target_dim = 512.0 * self.texture.zoom;
        let max_w = avail.x.min(target_dim).min(tex_sz.x * 32.0 * self.texture.zoom);
        let display_w = max_w.max(tex_sz.x * 8.0);
        let display_h = display_w / (tex_sz.x / tex_sz.y);

        let canvas_bg = Color32::BLACK;
        egui::Frame {
            fill: canvas_bg,
            inner_margin: egui::Margin::symmetric(2, 2),
            corner_radius: CornerRadius::same(0),
            stroke: Stroke::new(1.0, Color32::from_gray(90)),
            ..Default::default()
        }
        .show(ui, |ui| {
            ui.painter().rect_filled(
                egui::Rect::from_min_size(ui.cursor().left_top(), egui::vec2(display_w, display_h)),
                CornerRadius::same(0),
                canvas_bg,
            );

            if let Some(tex_id) = tex_id {
                let image_response = ui.add(
                    egui::Image::new((tex_id, egui::vec2(display_w, display_h)))
                );

                if self.texture.show_grid && display_w / tex_sz.x > 4.0 {
                    let painter = ui.painter();
                    let cell_w = display_w / tex_sz.x;
                    let cell_h = display_h / tex_sz.y;
                    let grid_stroke = Stroke::new(1.0, Color32::from_gray(80));

                    for i in 0..=(tex_sz.x as i32) {
                        let x = image_response.rect.left() + i as f32 * cell_w;
                        painter.line_segment(
                            [egui::pos2(x, image_response.rect.top()), egui::pos2(x, image_response.rect.bottom())],
                            grid_stroke,
                        );
                    }
                    for j in 0..=(tex_sz.y as i32) {
                        let y = image_response.rect.top() + j as f32 * cell_h;
                        painter.line_segment(
                            [egui::pos2(image_response.rect.left(), y), egui::pos2(image_response.rect.right(), y)],
                            grid_stroke,
                        );
                    }
                }

                if self.texture.mirror_x || self.texture.mirror_y {
                    let painter = ui.painter();
                    let mirror_stroke = Stroke::new(1.0, C_MAUVE.gamma_multiply(0.4));
                    if self.texture.mirror_x {
                        let mx = image_response.rect.center().x;
                        painter.line_segment(
                            [egui::pos2(mx, image_response.rect.top()), egui::pos2(mx, image_response.rect.bottom())],
                            mirror_stroke,
                        );
                    }
                    if self.texture.mirror_y {
                        let my = image_response.rect.center().y;
                        painter.line_segment(
                            [egui::pos2(image_response.rect.left(), my), egui::pos2(image_response.rect.right(), my)],
                            mirror_stroke,
                        );
                    }
                }

                let pointer_pos = ui.input(|i| i.pointer.latest_pos());
                let primary_pressed = ui.input(|i| i.pointer.primary_pressed());
                let primary_down = ui.input(|i| i.pointer.primary_down());
                let primary_released = ui.input(|i| i.pointer.primary_released());

                if primary_released {
                    self.texture.last_pixel = None;
                }

                // If the pointer is over the 3D Preview / Custom Color window
                // (floating on top of the canvas), treat it as "not on the
                // canvas" entirely — dragging to rotate the preview must never
                // paint pixels on the texture underneath it.
                let pointer_over_other_window = pointer_pos
                    .map(|pos| self.blocking_window_rects.iter().any(|r| r.contains(pos)))
                    .unwrap_or(false);
                let pointer_pos = if pointer_over_other_window { None } else { pointer_pos };
                if pointer_over_other_window {
                    self.texture.cursor_pixel = None;
                }

                if let Some(pos) = pointer_pos {
                    let rel = pos - image_response.rect.left_top();
                    let scale = display_w / tex_sz.x;
                    let img_x = (rel.x / scale).floor().max(0.0) as u32;
                    let img_y = (rel.y / scale).floor().max(0.0) as u32;
                    let in_bounds = img_x < self.texture.canvas.width() && img_y < self.texture.canvas.height();

                    if in_bounds {
                        self.texture.cursor_pixel = Some((img_x, img_y));
                    } else {
                        self.texture.cursor_pixel = None;
                    }

                    if image_response.rect.contains(pos) && in_bounds {
                        match self.texture.tool {
                            Tool::Brush | Tool::Eraser => {
                                if primary_pressed {
                                    self.texture.push_undo();
                                }
                                if primary_down {
                                    if let Some((lx, ly)) = self.texture.last_pixel {
                                        self.texture.stamp_line(lx, ly, img_x, img_y);
                                    } else {
                                        self.texture.stamp(img_x, img_y);
                                    }
                                    self.texture.last_pixel = Some((img_x, img_y));
                                    self.texture.ensure_texture(ctx);
                                }
                            }
                            Tool::Fill => {
                                if primary_pressed {
                                    self.texture.push_undo();
                                    self.texture.flood_fill(img_x, img_y);
                                    self.texture.ensure_texture(ctx);
                                }
                            }
                            Tool::Eyedropper => {
                                if primary_down {
                                    self.texture.pick_color(img_x, img_y);
                                }
                            }
                        }
                    }

                    if image_response.rect.contains(pos) {
                        let painter = ui.painter();
                        match self.texture.tool {
                            Tool::Brush | Tool::Eraser => {
                                let cursor_color = match self.texture.tool {
                                    Tool::Brush => C_BLUE.gamma_multiply(0.9),
                                    Tool::Eraser => C_RED.gamma_multiply(0.9),
                                    _ => unreachable!(),
                                };
                                let cell_rect = egui::Rect::from_min_size(
                                    image_response.rect.left_top()
                                        + egui::vec2(img_x as f32 * scale, img_y as f32 * scale),
                                    egui::vec2(scale, scale),
                                );
                                painter.rect_stroke(
                                    cell_rect,
                                    CornerRadius::same(0),
                                    Stroke::new(1.8, cursor_color),
                                    egui::StrokeKind::Middle,
                                );
                            }
                            Tool::Fill => {
                                let s = 8.0;
                                let stroke = Stroke::new(1.5, C_TEAL.gamma_multiply(0.9));
                                painter.line_segment(
                                    [egui::pos2(pos.x - s, pos.y), egui::pos2(pos.x + s, pos.y)],
                                    stroke,
                                );
                                painter.line_segment(
                                    [egui::pos2(pos.x, pos.y - s), egui::pos2(pos.x, pos.y + s)],
                                    stroke,
                                );
                            }
                            Tool::Eyedropper => {
                                painter.circle_stroke(
                                    pos,
                                    6.0,
                                    Stroke::new(1.5, C_YELLOW.gamma_multiply(0.9)),
                                );
                            }
                        }
                    }
                }
            }
        });

        ui.add_space(8.0);
        ui.horizontal(|ui| {
            let sz = self.texture.canvas_size_setting;
            ui.label(
                egui::RichText::new(format!("Canvas: {}x{}", sz, sz))
                    .size(11.0)
                    .color(C_SUBTEXT),
            );
            ui.add_space(12.0);
            ui.label(
                egui::RichText::new(format!("Tool: {}", self.texture.tool.label()))
                    .size(11.0)
                    .color(C_SUBTEXT),
            );
            ui.add_space(12.0);
            ui.label(
                egui::RichText::new(format!(
                    "Undo: {}  Redo: {}",
                    self.texture.undo_stack.len(),
                    self.texture.redo_stack.len(),
                ))
                .size(11.0)
                .color(C_SUBTEXT),
            );
        });
    }
}

// ── Entry point ─────────────────────────────────────────────────────────────

fn main() -> eframe::Result<()> {
    let default_app = AtlasApp::default();

    let grid_px =
        default_app.grid_size as f32 * (TARGET_CELL_PX + GRID_SPACING) - GRID_SPACING;
    let panel_margin_h = 20.0;
    let panel_margin_v = 12.0;
    let top_h = 120.0;
    let bottom_h = 34.0;
    let init_w = (grid_px + panel_margin_h).min(1100.0);
    let init_h = (grid_px + top_h + bottom_h + panel_margin_v).min(1100.0);

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([init_w, init_h])
            .with_min_inner_size([500.0, 400.0]),
        ..Default::default()
    };

    eframe::run_native(
        "Atlas Maker",
        options,
        Box::new(|cc| {
            apply_theme(&cc.egui_ctx);
            Ok(Box::new(default_app))
        }),
    )
}