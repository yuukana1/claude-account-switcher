//! Visual language: Claude's own design tokens — ivory canvas, layered surfaces,
//! hairline borders, clay as the single accent, and Anthropic Sans/Serif.
//! Light and dark palettes are two instances of the same token set.

use std::path::Path;
use std::sync::Arc;

use eframe::egui::{
    self, epaint::RectShape, Align2, Color32, CornerRadius, FontData, FontFamily, FontId, Mesh,
    Painter, Pos2, Rect, Shape, Stroke, StrokeKind, Vec2,
};

// ------------------------------------------------------------------ type ----

pub const FONT_DISPLAY: f32 = 34.0;
pub const FONT_TITLE: f32 = 18.0;
pub const FONT_ROW: f32 = 15.0;
pub const FONT_BODY: f32 = 13.0;
pub const FONT_SMALL: f32 = 11.8;
pub const FONT_TINY: f32 = 10.6;

pub const RADIUS_CARD: u8 = 12;
pub const RADIUS_ROW: u8 = 8;

pub fn ui_font(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name("ui".into()))
}

pub fn semi_font(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name("semibold".into()))
}

pub fn display_font(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name("display".into()))
}

// --------------------------------------------------------------- palette ----

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Light,
    Dark,
}

#[derive(Clone, Copy, Debug)]
pub struct Palette {
    pub mode: Mode,
    pub bg: Color32,
    pub surface: Color32,
    pub surface_soft: Color32,
    pub ink: Color32,
    pub muted: Color32,
    pub faint: Color32,
    pub accent: Color32,
    pub accent_hi: Color32,
    pub accent_dark: Color32,
    pub accent_soft: Color32,
    pub running: Color32,
    pub running_bg: Color32,
    pub danger: Color32,
    pub danger_bg: Color32,
    pub weekly: Color32,
    pub border: Color32,
    pub border_strong: Color32,
    pub hairline: Color32,
    pub hover: Color32,
    pub chip: Color32,
}

impl Palette {
    pub const fn light() -> Self {
        Self {
            mode: Mode::Light,
            bg: Color32::from_rgb(249, 249, 247),
            surface: Color32::from_rgb(255, 255, 255),
            surface_soft: Color32::from_rgb(252, 252, 251),
            ink: Color32::from_rgb(20, 20, 19),
            muted: Color32::from_rgb(110, 107, 100),
            faint: Color32::from_rgb(138, 134, 126),
            accent: Color32::from_rgb(217, 119, 87),
            accent_hi: Color32::from_rgb(224, 138, 108),
            accent_dark: Color32::from_rgb(198, 97, 63),
            accent_soft: Color32::from_rgb(247, 236, 230),
            running: Color32::from_rgb(62, 122, 87),
            running_bg: Color32::from_rgb(237, 244, 239),
            danger: Color32::from_rgb(166, 61, 44),
            danger_bg: Color32::from_rgb(248, 236, 233),
            weekly: Color32::from_rgb(106, 155, 204),
            border: Color32::from_rgba_unmultiplied_const(20, 20, 19, 26),
            border_strong: Color32::from_rgba_unmultiplied_const(20, 20, 19, 51),
            hairline: Color32::from_rgba_unmultiplied_const(20, 20, 19, 18),
            hover: Color32::from_rgb(246, 245, 242),
            chip: Color32::from_rgb(242, 241, 237),
        }
    }

    pub const fn dark() -> Self {
        Self {
            mode: Mode::Dark,
            bg: Color32::from_rgb(24, 24, 22),
            surface: Color32::from_rgb(35, 35, 32),
            surface_soft: Color32::from_rgb(30, 30, 28),
            ink: Color32::from_rgb(245, 244, 239),
            muted: Color32::from_rgb(166, 162, 154),
            faint: Color32::from_rgb(124, 120, 113),
            accent: Color32::from_rgb(217, 119, 87),
            accent_hi: Color32::from_rgb(230, 146, 116),
            accent_dark: Color32::from_rgb(232, 152, 122),
            accent_soft: Color32::from_rgb(53, 40, 34),
            running: Color32::from_rgb(126, 190, 152),
            running_bg: Color32::from_rgb(32, 46, 38),
            danger: Color32::from_rgb(232, 128, 108),
            danger_bg: Color32::from_rgb(56, 36, 32),
            weekly: Color32::from_rgb(130, 172, 214),
            border: Color32::from_rgba_unmultiplied_const(255, 255, 255, 24),
            border_strong: Color32::from_rgba_unmultiplied_const(255, 255, 255, 46),
            hairline: Color32::from_rgba_unmultiplied_const(255, 255, 255, 18),
            hover: Color32::from_rgb(42, 42, 38),
            chip: Color32::from_rgb(46, 46, 42),
        }
    }

    pub fn from_mode(mode: Mode) -> Self {
        match mode {
            Mode::Light => Self::light(),
            Mode::Dark => Self::dark(),
        }
    }

    pub fn is_dark(&self) -> bool {
        self.mode == Mode::Dark
    }
}

// ----------------------------------------------------------------- fonts ----

pub fn install_fonts(ctx: &egui::Context) {
    let mut defs = egui::FontDefinitions::default();

    // Claude ships Anthropic Sans and Anthropic Serif with its desktop app. If the user
    // ran tools/prepare_assets.py the static weights sit in the switcher's local folder;
    // otherwise fall back to the Windows UI face Claude's CSS resolves to.
    let anthropic_medium = anthropic_font("anthropic-medium", "AnthropicSans-Medium.ttf");
    let anthropic_semi = anthropic_font("anthropic-semibold", "AnthropicSans-SemiBold.ttf");
    let anthropic_bold = anthropic_font("anthropic-bold", "AnthropicSans-Bold.ttf");
    let anthropic_serif = anthropic_font("anthropic-serif", "AnthropicSerif-Medium.ttf");

    let yahei_ui = load_font("yahei-ui", "msyh.ttc", 1)
        .or_else(|| load_font("yahei-ui", "msyh.ttc", 0))
        .or_else(|| load_font("yahei-ui", "simhei.ttf", 0));
    let yahei_ui_bold = load_font("yahei-ui-bold", "msyhbd.ttc", 1)
        .or_else(|| load_font("yahei-ui-bold", "msyhbd.ttc", 0))
        .or_else(|| load_font("yahei-ui-bold", "simhei.ttf", 0));
    let segoe = load_font("segoe", "segoeui.ttf", 0);
    let segoe_semi = load_font("segoe-semibold", "seguisb.ttf", 0);
    let segoe_bold = load_font("segoe-bold", "segoeuib.ttf", 0);

    let mut family_ui: Vec<String> = Vec::new();
    let mut family_semi: Vec<String> = Vec::new();
    let mut family_bold: Vec<String> = Vec::new();
    let mut family_display: Vec<String> = Vec::new();

    if let Some((name, data)) = anthropic_medium {
        defs.font_data.insert(name.clone(), Arc::new(data));
        family_ui.push(name);
    } else if let Some((name, data)) = segoe_semi.clone() {
        defs.font_data.insert(name.clone(), Arc::new(data));
        family_ui.push(name);
    }
    if let Some((name, data)) = anthropic_semi {
        defs.font_data.insert(name.clone(), Arc::new(data));
        family_semi.push(name);
    }
    if let Some((name, data)) = anthropic_bold {
        defs.font_data.insert(name.clone(), Arc::new(data));
        family_bold.push(name);
    }
    if let Some((name, data)) = anthropic_serif {
        defs.font_data.insert(name.clone(), Arc::new(data));
        family_display.push(name);
    }
    if let Some((name, data)) = yahei_ui.clone() {
        defs.font_data.insert(name.clone(), Arc::new(data));
        family_ui.push(name.clone());
        family_display.push(name);
    }
    if let Some((name, data)) = yahei_ui_bold {
        defs.font_data.insert(name.clone(), Arc::new(data));
        family_semi.push(name.clone());
        family_bold.push(name);
    }
    if let Some((name, data)) = segoe {
        defs.font_data.insert(name.clone(), Arc::new(data));
        family_ui.push(name);
    }
    if let Some((name, data)) = segoe_semi {
        defs.font_data.insert(name.clone(), Arc::new(data));
        if family_semi.is_empty() {
            family_semi.push(name);
        }
    }
    if let Some((name, data)) = segoe_bold {
        defs.font_data.insert(name.clone(), Arc::new(data));
        if family_semi.is_empty() {
            family_semi.push(name.clone());
        }
        if family_bold.is_empty() {
            family_bold.push(name);
        }
    }
    if family_display.is_empty() {
        family_display = family_ui.clone();
    }
    if family_semi.is_empty() {
        family_semi = family_ui.clone();
    }
    if family_bold.is_empty() {
        family_bold = family_semi.clone();
    }

    defs.families
        .insert(FontFamily::Proportional, family_ui.clone());
    defs.families
        .insert(FontFamily::Name("ui".into()), family_ui);
    defs.families
        .insert(FontFamily::Name("semibold".into()), family_semi);
    defs.families
        .insert(FontFamily::Name("bold".into()), family_bold);
    defs.families
        .insert(FontFamily::Name("display".into()), family_display);

    ctx.set_fonts(defs);
}

/// Applies sizes, spacing and widget colours for the active palette.
pub fn install_style(ctx: &egui::Context, pal: &Palette) {
    // egui 0.33 keeps a separate style per theme and picks one from the system theme.
    // Writing only the active one would be discarded on the next system-theme change,
    // so pin the theme to the palette we are drawing and write both slots.
    ctx.set_theme(if pal.is_dark() {
        egui::ThemePreference::Dark
    } else {
        egui::ThemePreference::Light
    });

    let mut style = (*ctx.style()).clone();
    style
        .text_styles
        .insert(egui::TextStyle::Body, ui_font(FONT_BODY));
    style
        .text_styles
        .insert(egui::TextStyle::Button, semi_font(FONT_BODY));
    style
        .text_styles
        .insert(egui::TextStyle::Small, ui_font(FONT_SMALL));
    style
        .text_styles
        .insert(egui::TextStyle::Heading, display_font(FONT_TITLE));
    style.spacing.item_spacing = Vec2::new(8.0, 8.0);
    style.spacing.button_padding = Vec2::new(12.0, 6.0);
    style.spacing.menu_margin = egui::Margin::same(6);

    let mut visuals = if pal.is_dark() {
        egui::Visuals::dark()
    } else {
        egui::Visuals::light()
    };
    visuals.override_text_color = Some(pal.ink);
    visuals.panel_fill = pal.bg;
    visuals.window_fill = pal.surface;
    visuals.faint_bg_color = pal.surface_soft;
    visuals.extreme_bg_color = pal.surface_soft;
    visuals.window_stroke = Stroke::new(1.0, pal.border);
    visuals.window_corner_radius = CornerRadius::same(14);
    visuals.menu_corner_radius = CornerRadius::same(10);
    visuals.window_shadow = egui::epaint::Shadow {
        offset: [0, 10],
        blur: 30,
        spread: 2,
        color: Color32::from_black_alpha(if pal.is_dark() { 120 } else { 30 }),
    };
    visuals.popup_shadow = egui::epaint::Shadow {
        offset: [0, 8],
        blur: 24,
        spread: 2,
        color: Color32::from_black_alpha(if pal.is_dark() { 140 } else { 34 }),
    };
    // Text selection must be unmistakable inside an input: a clay tint that reads as
    // "selected", not as a stray background, plus a stronger caret.
    visuals.selection.bg_fill = if pal.is_dark() {
        mix(pal.accent, pal.surface_soft, 0.18)
    } else {
        mix(pal.accent, pal.surface, 0.24)
    };
    // Inside a text edit the selection stroke colour is also the selected text colour.
    visuals.selection.stroke = Stroke::new(
        1.0,
        if pal.is_dark() {
            Color32::from_rgb(28, 20, 17)
        } else {
            Color32::from_rgb(74, 34, 20)
        },
    );
    visuals.text_cursor.stroke = Stroke::new(1.4, pal.accent_dark);
    visuals.hyperlink_color = pal.accent_dark;
    for w in [
        &mut visuals.widgets.noninteractive,
        &mut visuals.widgets.inactive,
        &mut visuals.widgets.hovered,
        &mut visuals.widgets.active,
        &mut visuals.widgets.open,
    ] {
        w.corner_radius = CornerRadius::same(8);
        w.fg_stroke = Stroke::new(1.0, pal.ink);
    }
    visuals.widgets.noninteractive.bg_fill = pal.surface;
    visuals.widgets.noninteractive.bg_stroke = Stroke::new(1.0, pal.border);
    visuals.widgets.inactive.bg_fill = pal.surface;
    visuals.widgets.inactive.bg_stroke = Stroke::new(1.0, pal.border);
    visuals.widgets.inactive.weak_bg_fill = pal.surface;
    visuals.widgets.hovered.bg_fill = pal.hover;
    visuals.widgets.hovered.bg_stroke = Stroke::new(1.0, pal.border_strong);
    visuals.widgets.hovered.weak_bg_fill = pal.hover;
    visuals.widgets.active.bg_fill = pal.accent_soft;
    visuals.widgets.active.bg_stroke = Stroke::new(1.0, pal.accent);
    visuals.widgets.active.weak_bg_fill = pal.accent_soft;
    visuals.widgets.open.bg_fill = pal.hover;
    visuals.widgets.open.bg_stroke = Stroke::new(1.0, pal.border_strong);
    style.visuals = visuals;
    ctx.set_style_of(egui::Theme::Light, style.clone());
    ctx.set_style_of(egui::Theme::Dark, style);
}

fn load_font(name: &str, file: &str, index: u32) -> Option<(String, FontData)> {
    let path = Path::new("C:\\Windows\\Fonts").join(file);
    let bytes = std::fs::read(path).ok()?;
    Some((
        name.to_owned(),
        FontData {
            font: bytes.into(),
            index,
            tweak: Default::default(),
        },
    ))
}

fn anthropic_font(name: &str, file: &str) -> Option<(String, FontData)> {
    for dir in anthropic_dirs() {
        let path = dir.join(file);
        if let Ok(bytes) = std::fs::read(path) {
            return Some((
                name.to_owned(),
                FontData {
                    font: bytes.into(),
                    index: 0,
                    tweak: Default::default(),
                },
            ));
        }
    }
    None
}

fn anthropic_dirs() -> Vec<std::path::PathBuf> {
    let mut dirs = Vec::new();
    if let Some(local) = std::env::var_os("LOCALAPPDATA") {
        dirs.push(
            std::path::PathBuf::from(local)
                .join("ClaudeProfiles")
                .join(".switcher")
                .join("fonts"),
        );
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(parent) = exe.parent() {
            dirs.push(parent.join("fonts"));
        }
    }
    dirs
}

pub fn has_anthropic_fonts() -> bool {
    anthropic_dirs()
        .iter()
        .any(|dir| dir.join("AnthropicSans-Medium.ttf").is_file())
}

// ---------------------------------------------------------------- colors ----

pub fn mix(a: Color32, b: Color32, t: f32) -> Color32 {
    let t = t.clamp(0.0, 1.0);
    let l = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;
    Color32::from_rgba_unmultiplied(
        l(a.r(), b.r()),
        l(a.g(), b.g()),
        l(a.b(), b.b()),
        l(a.a(), b.a()),
    )
}

pub fn with_alpha(color: Color32, alpha: f32) -> Color32 {
    Color32::from_rgba_unmultiplied(
        color.r(),
        color.g(),
        color.b(),
        (alpha.clamp(0.0, 1.0) * 255.0).round() as u8,
    )
}

pub fn shade(color: Color32, amount: f32) -> Color32 {
    if amount >= 0.0 {
        mix(color, Color32::from_rgb(0, 0, 0), amount)
    } else {
        mix(color, Color32::WHITE, -amount)
    }
}

/// A muted tint for an account initial, chosen from the stored palette slot. Claude's
/// palette is warm and low-chroma, so the hues are pulled toward the clay range.
pub fn avatar_tint(index: usize, pal: &Palette) -> Color32 {
    const HUES: [Color32; 8] = [
        Color32::from_rgb(198, 97, 63),
        Color32::from_rgb(176, 116, 74),
        Color32::from_rgb(150, 118, 84),
        Color32::from_rgb(122, 106, 94),
        Color32::from_rgb(106, 118, 118),
        Color32::from_rgb(112, 110, 136),
        Color32::from_rgb(150, 104, 112),
        Color32::from_rgb(104, 128, 108),
    ];
    let base = HUES[index % HUES.len()];
    if pal.is_dark() {
        mix(base, Color32::from_rgb(20, 20, 18), 0.35)
    } else {
        base
    }
}

// ---------------------------------------------------------------- shapes ----

pub fn soft_shadow(painter: &Painter, rect: Rect, radius: u8, blur: f32, dy: f32, alpha: u8) {
    if alpha == 0 {
        return;
    }
    let shape = RectShape::filled(
        rect.translate(Vec2::new(0.0, dy)),
        CornerRadius::same(radius),
        Color32::from_black_alpha(alpha),
    )
    .with_blur_width(blur);
    painter.add(shape);
}

pub fn card(painter: &Painter, rect: Rect, radius: u8, pal: &Palette) {
    // Claude's ambient shadow ladder: tight occlusion plus a wide diffusion.
    let strength = if pal.is_dark() { 90 } else { 13 };
    soft_shadow(painter, rect, radius, 2.0, 1.0, strength);
    soft_shadow(
        painter,
        rect,
        radius,
        8.0,
        2.0,
        if pal.is_dark() { 110 } else { 20 },
    );
    painter.rect_filled(rect, CornerRadius::same(radius), pal.surface);
    painter.rect_stroke(
        rect,
        CornerRadius::same(radius),
        Stroke::new(1.0, pal.border),
        StrokeKind::Inside,
    );
}

pub fn vertical_gradient(painter: &Painter, rect: Rect, top: Color32, bottom: Color32) {
    let mut mesh = Mesh::default();
    mesh.colored_vertex(rect.left_top(), top);
    mesh.colored_vertex(rect.right_top(), top);
    mesh.colored_vertex(rect.right_bottom(), bottom);
    mesh.colored_vertex(rect.left_bottom(), bottom);
    mesh.add_triangle(0, 1, 2);
    mesh.add_triangle(0, 2, 3);
    painter.add(Shape::mesh(mesh));
}

/// The detail canvas: a barely-there vertical wash over the surface colour. Cards then
/// sit on a softly graded ground instead of a flat fill.
pub fn detail_ground(painter: &Painter, rect: Rect, pal: &Palette) {
    let bottom = if pal.is_dark() {
        mix(pal.surface_soft, Color32::BLACK, 0.22)
    } else {
        mix(pal.surface_soft, pal.ink, 0.035)
    };
    vertical_gradient(painter, rect, pal.surface_soft, bottom);
}

pub fn radial_glow(painter: &Painter, center: Pos2, radius: f32, color: Color32) {
    let mut mesh = Mesh::default();
    mesh.colored_vertex(center, color);
    let steps = 72;
    for i in 0..=steps {
        let a = i as f32 / steps as f32 * std::f32::consts::TAU;
        mesh.colored_vertex(
            center + Vec2::new(a.cos(), a.sin()) * radius,
            Color32::TRANSPARENT,
        );
    }
    for i in 0..steps {
        mesh.add_triangle(0, 1 + i as u32, 2 + i as u32);
    }
    painter.add(Shape::mesh(mesh));
}

pub fn starburst(painter: &Painter, center: Pos2, radius: f32, color: Color32) {
    let inner = radius * 0.14;
    let outer = radius * 0.82;
    let width = (radius * 0.108).max(1.1);
    let rays = 12;
    for i in 0..rays {
        let angle = -std::f32::consts::FRAC_PI_2 + i as f32 / rays as f32 * std::f32::consts::TAU;
        let dir = Vec2::new(angle.cos(), angle.sin());
        let p0 = center + dir * inner;
        let p1 = center + dir * outer;
        painter.line_segment([p0, p1], Stroke::new(width, color));
        painter.circle_filled(p0, width * 0.5, color);
        painter.circle_filled(p1, width * 0.5, color);
    }
}

pub fn logo_tile(
    painter: &Painter,
    texture: &egui::TextureHandle,
    rect: Rect,
    radius: u8,
    badge: Option<(Color32, String)>,
    letter_font: FontId,
    pal: &Palette,
) {
    soft_shadow(
        painter,
        rect,
        radius,
        10.0,
        3.0,
        if pal.is_dark() { 70 } else { 22 },
    );
    painter.image(
        texture.id(),
        rect,
        Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
        Color32::WHITE,
    );

    if let Some((color, letter)) = badge {
        let d = rect.width() * 0.42;
        let center = Pos2::new(rect.right() - d * 0.42, rect.bottom() - d * 0.42);
        painter.circle_filled(center, d * 0.5 + 2.0, pal.surface);
        painter.circle_filled(center, d * 0.5, color);
        painter.text(
            center,
            Align2::CENTER_CENTER,
            letter,
            letter_font,
            Color32::WHITE,
        );
    }
}

pub fn logo_image(painter: &Painter, texture: &egui::TextureHandle, rect: Rect) {
    painter.image(
        texture.id(),
        rect,
        Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
        Color32::WHITE,
    );
}

/// Circular avatar from a square texture, with a hairline ring in the surface colour so
/// it reads as a separate object on tinted rows.
pub fn circle_image(
    painter: &Painter,
    texture: &egui::TextureHandle,
    rect: Rect,
    ring: Color32,
    ring_width: f32,
) {
    let center = rect.center();
    let radius = rect.width() * 0.5;
    let steps = 64usize;
    let mut mesh = Mesh::with_texture(texture.id());
    mesh.vertices.push(egui::epaint::Vertex {
        pos: center,
        uv: Pos2::new(0.5, 0.5),
        color: Color32::WHITE,
    });
    for i in 0..=steps {
        let a = i as f32 / steps as f32 * std::f32::consts::TAU;
        let (s, c) = (a.sin(), a.cos());
        mesh.vertices.push(egui::epaint::Vertex {
            pos: center + Vec2::new(c, s) * radius,
            uv: Pos2::new(0.5 + c * 0.5, 0.5 + s * 0.5),
            color: Color32::WHITE,
        });
    }
    for i in 0..steps {
        mesh.add_triangle(0, 1 + i as u32, 2 + i as u32);
    }
    painter.add(Shape::mesh(mesh));
    if ring_width > 0.0 {
        painter.circle_stroke(
            center,
            radius - ring_width * 0.5,
            Stroke::new(ring_width, ring),
        );
    }
}

// ------------------------------------------------------------------ icons ----

pub fn icon_arrow_right(painter: &Painter, rect: Rect, color: Color32) {
    let r = rect.shrink(rect.width() * 0.31);
    let c = rect.center();
    let stroke = Stroke::new(1.6, color);
    painter.line_segment(
        [Pos2::new(r.left(), c.y), Pos2::new(r.right(), c.y)],
        stroke,
    );
    painter.line_segment(
        [
            Pos2::new(c.x + r.width() * 0.10, c.y - r.height() * 0.30),
            Pos2::new(r.right(), c.y),
        ],
        stroke,
    );
    painter.line_segment(
        [
            Pos2::new(c.x + r.width() * 0.10, c.y + r.height() * 0.30),
            Pos2::new(r.right(), c.y),
        ],
        stroke,
    );
}

pub fn icon_focus(painter: &Painter, rect: Rect, color: Color32) {
    let r = rect.shrink(rect.width() * 0.30);
    let stroke = Stroke::new(1.5, color);
    let l = r.width() * 0.34;
    for (corner, dx, dy) in [
        (r.left_top(), 1.0, 1.0),
        (r.right_top(), -1.0, 1.0),
        (r.left_bottom(), 1.0, -1.0),
        (r.right_bottom(), -1.0, -1.0),
    ] {
        painter.line_segment([corner, corner + Vec2::new(dx * l, 0.0)], stroke);
        painter.line_segment([corner, corner + Vec2::new(0.0, dy * l)], stroke);
    }
    painter.circle_filled(r.center(), r.width() * 0.10, color);
}

pub fn icon_plus(painter: &Painter, rect: Rect, color: Color32) {
    let r = rect.shrink(rect.width() * 0.32);
    let stroke = Stroke::new(1.6, color);
    painter.line_segment([r.left_center(), r.right_center()], stroke);
    painter.line_segment([r.center_top(), r.center_bottom()], stroke);
}

pub fn icon_transfer(painter: &Painter, rect: Rect, color: Color32) {
    let r = rect.shrink(rect.width() * 0.28);
    let stroke = Stroke::new(1.4, color);
    let y1 = r.center().y - r.height() * 0.20;
    let y2 = r.center().y + r.height() * 0.20;
    painter.line_segment([Pos2::new(r.left(), y1), Pos2::new(r.right(), y1)], stroke);
    painter.line_segment(
        [
            Pos2::new(r.right() - r.width() * 0.22, y1 - r.height() * 0.15),
            Pos2::new(r.right(), y1),
        ],
        stroke,
    );
    painter.line_segment(
        [
            Pos2::new(r.right() - r.width() * 0.22, y1 + r.height() * 0.15),
            Pos2::new(r.right(), y1),
        ],
        stroke,
    );
    painter.line_segment([Pos2::new(r.left(), y2), Pos2::new(r.right(), y2)], stroke);
    painter.line_segment(
        [
            Pos2::new(r.left() + r.width() * 0.22, y2 - r.height() * 0.15),
            Pos2::new(r.left(), y2),
        ],
        stroke,
    );
    painter.line_segment(
        [
            Pos2::new(r.left() + r.width() * 0.22, y2 + r.height() * 0.15),
            Pos2::new(r.left(), y2),
        ],
        stroke,
    );
}

pub fn icon_dots(painter: &Painter, rect: Rect, color: Color32) {
    let c = rect.center();
    let gap = rect.width() * 0.17;
    for i in -1..=1 {
        painter.circle_filled(Pos2::new(c.x + i as f32 * gap, c.y), 1.5, color);
    }
}

pub fn icon_minimize(painter: &Painter, rect: Rect, color: Color32) {
    let r = rect.shrink(rect.width() * 0.34);
    painter.line_segment(
        [
            Pos2::new(r.left(), r.center().y),
            Pos2::new(r.right(), r.center().y),
        ],
        Stroke::new(1.3, color),
    );
}

pub fn icon_close(painter: &Painter, rect: Rect, color: Color32) {
    let r = rect.shrink(rect.width() * 0.34);
    let stroke = Stroke::new(1.3, color);
    painter.line_segment([r.left_top(), r.right_bottom()], stroke);
    painter.line_segment([r.right_top(), r.left_bottom()], stroke);
}

pub fn icon_check(painter: &Painter, rect: Rect, color: Color32) {
    let r = rect.shrink(rect.width() * 0.24);
    let stroke = Stroke::new(1.9, color);
    painter.line_segment(
        [
            Pos2::new(r.left(), r.center().y + r.height() * 0.04),
            Pos2::new(r.center().x - r.width() * 0.06, r.bottom()),
        ],
        stroke,
    );
    painter.line_segment(
        [
            Pos2::new(r.center().x - r.width() * 0.02, r.bottom()),
            Pos2::new(r.right(), r.top()),
        ],
        stroke,
    );
}

pub fn icon_refresh(painter: &Painter, rect: Rect, color: Color32) {
    let c = rect.center();
    let radius = rect.width() * 0.26;
    let stroke = Stroke::new(1.4, color);
    let steps = 26;
    let start = -std::f32::consts::FRAC_PI_2 + 0.55;
    let end = start + std::f32::consts::TAU - 0.9;
    let mut prev = c + Vec2::new(start.cos(), start.sin()) * radius;
    for i in 1..=steps {
        let a = start + (end - start) * i as f32 / steps as f32;
        let p = c + Vec2::new(a.cos(), a.sin()) * radius;
        painter.line_segment([prev, p], stroke);
        prev = p;
    }
    let tip = prev;
    painter.circle_filled(tip, 1.8, color);
    painter.line_segment(
        [tip + Vec2::new(-3.2, -0.6), tip + Vec2::new(0.4, 2.5)],
        Stroke::new(1.4, color),
    );
}

pub fn icon_pencil(painter: &Painter, rect: Rect, color: Color32) {
    // A pen seen at 45°: tip, two parallel edges, and the cap.
    let r = rect.shrink(rect.width() * 0.28);
    let stroke = Stroke::new(1.35, color);
    let tip = Pos2::new(r.left(), r.bottom());
    let shoulder_a = Pos2::new(r.left() + r.width() * 0.28, r.bottom() - r.height() * 0.28);
    let shoulder_b = Pos2::new(r.right() - r.width() * 0.26, r.top() + r.height() * 0.26);
    let cap_a = Pos2::new(r.left() + r.width() * 0.42, r.top());
    let cap_b = Pos2::new(r.right(), r.top() + r.height() * 0.58);
    painter.line_segment([tip, shoulder_a], stroke);
    painter.line_segment([shoulder_a, cap_a], stroke);
    painter.line_segment([shoulder_b, cap_b], stroke);
    painter.line_segment([cap_a, cap_b], stroke);
    // Nib line where the point ends.
    painter.line_segment([shoulder_a, shoulder_b], Stroke::new(1.0, color));
}

pub fn icon_folder(painter: &Painter, rect: Rect, color: Color32) {
    let r = rect.shrink(rect.width() * 0.22);
    let stroke = Stroke::new(1.35, color);
    let body = Rect::from_min_max(
        Pos2::new(r.left(), r.top() + r.height() * 0.22),
        Pos2::new(r.right(), r.bottom() - r.height() * 0.06),
    );
    painter.rect_stroke(body, CornerRadius::same(2), stroke, StrokeKind::Inside);
    // The tab on the top edge.
    painter.line_segment(
        [
            Pos2::new(body.left() + r.width() * 0.06, body.top()),
            Pos2::new(body.left() + r.width() * 0.36, body.top()),
        ],
        Stroke::new(2.0, color),
    );
}

pub fn icon_chat(painter: &Painter, rect: Rect, color: Color32) {
    let r = rect.shrink(rect.width() * 0.16);
    painter.rect_stroke(
        r,
        CornerRadius::same(4),
        Stroke::new(1.15, color),
        StrokeKind::Inside,
    );
    // Little tail at the bottom-left, the way Claude's conversation glyph reads.
    painter.line_segment(
        [
            Pos2::new(r.left() + r.width() * 0.20, r.bottom()),
            Pos2::new(r.left() + r.width() * 0.20, r.bottom() + r.height() * 0.22),
        ],
        Stroke::new(1.15, color),
    );
    painter.line_segment(
        [
            Pos2::new(r.left() + r.width() * 0.20, r.bottom() + r.height() * 0.22),
            Pos2::new(r.left() + r.width() * 0.42, r.bottom()),
        ],
        Stroke::new(1.15, color),
    );
}

pub fn icon_sun(painter: &Painter, rect: Rect, color: Color32) {
    let c = rect.center();
    let r = rect.width() * 0.19;
    painter.circle_stroke(c, r, Stroke::new(1.4, color));
    let stroke = Stroke::new(1.4, color);
    for i in 0..8 {
        let a = i as f32 / 8.0 * std::f32::consts::TAU;
        let dir = Vec2::new(a.cos(), a.sin());
        painter.line_segment([c + dir * (r * 1.7), c + dir * (r * 2.5)], stroke);
    }
}

pub fn icon_moon(painter: &Painter, rect: Rect, color: Color32) {
    let c = rect.center();
    let r = rect.width() * 0.27;
    // A crescent: the disc with a shifted disc subtracted using a clay-coloured mask is
    // not possible in a single stroke, so draw an arc that closes like a moon.
    let steps = 30;
    let stroke = Stroke::new(1.4, color);
    let start = -std::f32::consts::FRAC_PI_2 + 0.5;
    let end = start + std::f32::consts::TAU - 1.0;
    let mut prev = c + Vec2::new(start.cos(), start.sin()) * r;
    for i in 1..=steps {
        let a = start + (end - start) * i as f32 / steps as f32;
        let p = c + Vec2::new(a.cos(), a.sin()) * r;
        painter.line_segment([prev, p], stroke);
        prev = p;
    }
    // Inner bite, drawn with the background colour by the caller's palette.
    let inner_center = c + Vec2::new(r * 0.55, -r * 0.35);
    let mut prev = inner_center + Vec2::new(0.0, r * 0.72);
    for i in 1..=20 {
        let a = -std::f32::consts::FRAC_PI_2 + std::f32::consts::PI * 1.35 * i as f32 / 20.0;
        let p = inner_center + Vec2::new(a.cos(), a.sin()) * r * 0.78;
        painter.line_segment([prev, p], stroke);
        prev = p;
    }
}

// ------------------------------------------------------------------- text ----

pub fn truncate(ctx: &egui::Context, text: &str, font: &FontId, max_width: f32) -> String {
    let galley = ctx.fonts_mut(|f| f.layout_no_wrap(text.to_owned(), font.clone(), Color32::WHITE));
    if galley.size().x <= max_width {
        return text.to_owned();
    }
    let chars: Vec<char> = text.chars().collect();
    let mut lo = 0usize;
    let mut hi = chars.len();
    while lo < hi {
        let mid = (lo + hi).div_ceil(2);
        let probe: String = chars[..mid].iter().collect::<String>() + "…";
        let w = ctx
            .fonts_mut(|f| f.layout_no_wrap(probe, font.clone(), Color32::WHITE))
            .size()
            .x;
        if w <= max_width {
            lo = mid;
        } else {
            hi = mid - 1;
        }
    }
    chars[..lo].iter().collect::<String>() + "…"
}

// --------------------------------------------------------------- app icon ----

/// The Claude mark shipped with the desktop app, when it can be found on disk.
pub fn official_logo_path() -> Option<std::path::PathBuf> {
    if let Some(local) = std::env::var_os("LOCALAPPDATA") {
        let path = std::path::PathBuf::from(local)
            .join("ClaudeProfiles")
            .join(".switcher")
            .join("logo-source.png");
        if path.is_file() {
            return Some(path);
        }
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(parent) = exe.parent() {
            let path = parent.join("logo.png");
            if path.is_file() {
                return Some(path);
            }
        }
    }
    None
}

pub fn icon_rgba(size: u32) -> Vec<u8> {
    if let Some(path) = official_logo_path() {
        if let Ok(image) = image::open(path) {
            let image = image
                .resize_exact(size, size, image::imageops::FilterType::Lanczos3)
                .to_rgba8();
            return image.into_raw();
        }
    }
    procedural_icon_rgba(size)
}

fn procedural_icon_rgba(size: u32) -> Vec<u8> {
    let s = size as f32;
    let radius = s * 0.225;
    let half = s * 0.5;
    let inner = s * 0.058;
    let outer = s * 0.408;
    let ray_w = s * 0.047 * 0.5;
    let rays = 12;
    let dirs: Vec<(f32, f32)> = (0..rays)
        .map(|i| {
            let a = -std::f32::consts::FRAC_PI_2 + i as f32 / rays as f32 * std::f32::consts::TAU;
            (a.cos(), a.sin())
        })
        .collect();

    let mut out = Vec::with_capacity((size * size * 4) as usize);
    let ss = 3;
    for y in 0..size {
        for x in 0..size {
            let mut r = 0.0f32;
            let mut g = 0.0f32;
            let mut b = 0.0f32;
            let mut a = 0.0f32;
            for sy in 0..ss {
                for sx in 0..ss {
                    let px = x as f32 + (sx as f32 + 0.5) / ss as f32;
                    let py = y as f32 + (sy as f32 + 0.5) / ss as f32;
                    if !inside_round_rect(px, py, s, radius) {
                        continue;
                    }
                    let t = py / s;
                    let te = t * t * (3.0 - 2.0 * t);
                    let (cr, cg, cb) = (
                        232.0 + (188.0 - 232.0) * te,
                        150.0 + (88.0 - 150.0) * te,
                        118.0 + (60.0 - 118.0) * te,
                    );
                    let mut white = false;
                    let dx = px - half;
                    let dy = py - half;
                    for (ux, uy) in &dirs {
                        let proj = dx * ux + dy * uy;
                        if proj < inner || proj > outer {
                            continue;
                        }
                        let perp = (dx * uy - dy * ux).abs();
                        if perp <= ray_w {
                            white = true;
                            break;
                        }
                    }
                    if white {
                        r += 255.0;
                        g += 250.0;
                        b += 246.0;
                    } else {
                        r += cr;
                        g += cg;
                        b += cb;
                    }
                    a += 255.0;
                }
            }
            let n = (ss * ss) as f32;
            let cov = a / (n * 255.0);
            // `a` already carries the per-sample 255, so the colour average is r/a.
            let (rr, gg, bb) = if a > 0.0 {
                (r / a, g / a, b / a)
            } else {
                (0.0, 0.0, 0.0)
            };
            out.push((rr * 255.0).round() as u8);
            out.push((gg * 255.0).round() as u8);
            out.push((bb * 255.0).round() as u8);
            out.push((cov * 255.0).round() as u8);
        }
    }
    out
}

fn inside_round_rect(x: f32, y: f32, s: f32, r: f32) -> bool {
    let hx = s * 0.5;
    let qx = (x - hx).abs() - (hx - r);
    let qy = (y - hx).abs() - (hx - r);
    let outside = (qx.max(0.0).powi(2) + qy.max(0.0).powi(2)).sqrt();
    outside + qx.max(qy).min(0.0) <= r
}

pub fn app_icon(size: u32) -> egui::IconData {
    egui::IconData {
        rgba: icon_rgba(size),
        width: size,
        height: size,
    }
}

/// White twelve-ray mark with a transparent background, used for watermark and title.
pub fn starburst_rgba(size: u32) -> Vec<u8> {
    let s = size as f32;
    let half = s * 0.5;
    let inner = s * 0.10;
    let outer = s * 0.47;
    let ray_w = s * 0.036;
    let rays = 12;
    let dirs: Vec<(f32, f32)> = (0..rays)
        .map(|i| {
            let a = -std::f32::consts::FRAC_PI_2 + i as f32 / rays as f32 * std::f32::consts::TAU;
            (a.cos(), a.sin())
        })
        .collect();

    let mut out = Vec::with_capacity((size * size * 4) as usize);
    let ss = 4;
    for y in 0..size {
        for x in 0..size {
            let mut covered = 0.0f32;
            for sy in 0..ss {
                for sx in 0..ss {
                    let px = x as f32 + (sx as f32 + 0.5) / ss as f32;
                    let py = y as f32 + (sy as f32 + 0.5) / ss as f32;
                    let dx = px - half;
                    let dy = py - half;
                    let mut inside = false;
                    for (ux, uy) in &dirs {
                        let proj = dx * ux + dy * uy;
                        if proj < inner || proj > outer {
                            continue;
                        }
                        let perp = (dx * uy - dy * ux).abs();
                        if perp <= ray_w {
                            inside = true;
                            break;
                        }
                    }
                    if inside {
                        covered += 1.0;
                    }
                }
            }
            let alpha = (covered / (ss * ss) as f32 * 255.0).round() as u8;
            out.extend_from_slice(&[255, 250, 246, alpha]);
        }
    }
    out
}
