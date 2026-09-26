use cosmic_text::{Attrs, Buffer, Color, Family, FontSystem, Metrics, Shaping, SwashCache};

use crate::{
    menu::{Hit, MenuState, Rect},
    style::Style,
};

pub struct Renderer {
    fonts: FontSystem,
    cache: SwashCache,
}

impl Renderer {
    pub fn new() -> Self {
        Self {
            fonts: FontSystem::new(),
            cache: SwashCache::new(),
        }
    }

    pub fn draw(&mut self, canvas: &mut [u8], width: u32, height: u32, menu: &MenuState) {
        canvas.fill(0);

        let style = &menu.style;
        let Some(root) = menu.root_rect(width as f64, height as f64) else {
            return;
        };

        draw_panel(canvas, width, height, root, style);

        for (index, item) in menu.config.items.iter().enumerate() {
            let Some(rect) = menu.root_item_rect(index, width as f64, height as f64) else {
                continue;
            };

            if menu.hovered == Some(Hit::Root(index)) {
                fill_rect(canvas, width, height, rect, style.colors.hover.bytes());
            }

            if item.separator_before {
                let line = Rect {
                    x: rect.x + style.separator.inset,
                    y: rect.y,
                    w: (rect.w - style.separator.inset * 2.0).max(0.0),
                    h: style.separator.width,
                };
                fill_rect(canvas, width, height, line, style.colors.separator.bytes());
            }

            self.draw_label(
                canvas,
                width,
                height,
                &item.label,
                rect,
                style.menu.padding_x,
                style.font.size,
                style.colors.text.text_color(),
                &style.font.family,
            );

            if !item.children.is_empty() {
                self.draw_label(
                    canvas,
                    width,
                    height,
                    &style.indicator.symbol,
                    Rect {
                        x: rect.x + rect.w - style.indicator.right_padding - style.indicator.width,
                        y: rect.y,
                        w: style.indicator.width,
                        h: rect.h,
                    },
                    0.0,
                    style.font.size + style.indicator.size_delta,
                    style.colors.dim.text_color(),
                    &style.font.family,
                );
            }
        }

        if let (Some(root_index), Some(submenu)) = (
            menu.open_root,
            menu.submenu_rect(width as f64, height as f64),
        ) {
            draw_panel(canvas, width, height, submenu, style);

            for (child, item) in menu.config.items[root_index].children.iter().enumerate() {
                let Some(rect) = menu.child_item_rect(child, width as f64, height as f64) else {
                    continue;
                };

                if menu.hovered
                    == Some(Hit::Child {
                        root: root_index,
                        child,
                    })
                {
                    fill_rect(canvas, width, height, rect, style.colors.hover.bytes());
                }

                self.draw_label(
                    canvas,
                    width,
                    height,
                    &item.label,
                    rect,
                    style.menu.padding_x,
                    style.font.size,
                    style.colors.text.text_color(),
                    &style.font.family,
                );
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn draw_label(
        &mut self,
        canvas: &mut [u8],
        width: u32,
        height: u32,
        text: &str,
        rect: Rect,
        padding_x: f64,
        font_size: f32,
        color: Color,
        font_family: &str,
    ) {
        let text_x = (rect.x + padding_x).round() as i32;
        let text_y = rect.y.round() as i32;
        let text_w = (rect.w - padding_x * 2.0).max(1.0) as f32;

        let mut buffer = Buffer::new(&mut self.fonts, Metrics::new(font_size, rect.h as f32));
        buffer.set_size(Some(text_w), Some(rect.h as f32));
        let family = match font_family {
            "sans-serif" => Family::SansSerif,
            "serif" => Family::Serif,
            "monospace" => Family::Monospace,
            "cursive" => Family::Cursive,
            "fantasy" => Family::Fantasy,
            name => Family::Name(name),
        };
        let attrs = Attrs::new().family(family);
        buffer.set_text(text, &attrs, Shaping::Advanced, None);

        buffer.draw(
            &mut self.fonts,
            &mut self.cache,
            color,
            |x, y, w, h, pixel| {
                blend_block(canvas, width, height, text_x + x, text_y + y, w, h, pixel);
            },
        );
    }
}

fn draw_panel(canvas: &mut [u8], width: u32, height: u32, rect: Rect, style: &Style) {
    fill_rect(canvas, width, height, rect, style.colors.background.bytes());

    let border = style.border.width;
    if border <= 0.0 {
        return;
    }

    let color = style.colors.border.bytes();
    fill_rect(
        canvas,
        width,
        height,
        Rect {
            x: rect.x,
            y: rect.y,
            w: rect.w,
            h: border,
        },
        color,
    );
    fill_rect(
        canvas,
        width,
        height,
        Rect {
            x: rect.x,
            y: rect.y + rect.h - border,
            w: rect.w,
            h: border,
        },
        color,
    );
    fill_rect(
        canvas,
        width,
        height,
        Rect {
            x: rect.x,
            y: rect.y,
            w: border,
            h: rect.h,
        },
        color,
    );
    fill_rect(
        canvas,
        width,
        height,
        Rect {
            x: rect.x + rect.w - border,
            y: rect.y,
            w: border,
            h: rect.h,
        },
        color,
    );
}

fn fill_rect(canvas: &mut [u8], width: u32, height: u32, rect: Rect, rgba: [u8; 4]) {
    let x0 = rect.x.floor().max(0.0) as u32;
    let y0 = rect.y.floor().max(0.0) as u32;
    let x1 = (rect.x + rect.w).ceil().clamp(0.0, width as f64) as u32;
    let y1 = (rect.y + rect.h).ceil().clamp(0.0, height as f64) as u32;

    for y in y0..y1 {
        for x in x0..x1 {
            let offset = ((y * width + x) * 4) as usize;
            canvas[offset] = rgba[2];
            canvas[offset + 1] = rgba[1];
            canvas[offset + 2] = rgba[0];
            canvas[offset + 3] = rgba[3];
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn blend_block(
    canvas: &mut [u8],
    width: u32,
    height: u32,
    x: i32,
    y: i32,
    w: u32,
    h: u32,
    color: Color,
) {
    for yy in 0..h as i32 {
        for xx in 0..w as i32 {
            blend_pixel(canvas, width, height, x + xx, y + yy, color);
        }
    }
}

fn blend_pixel(canvas: &mut [u8], width: u32, height: u32, x: i32, y: i32, color: Color) {
    if x < 0 || y < 0 || x >= width as i32 || y >= height as i32 {
        return;
    }

    let offset = ((y as u32 * width + x as u32) * 4) as usize;
    let alpha = color.a() as u16;
    let inv = 255 - alpha;

    let dst_b = canvas[offset] as u16;
    let dst_g = canvas[offset + 1] as u16;
    let dst_r = canvas[offset + 2] as u16;
    let dst_a = canvas[offset + 3] as u16;

    canvas[offset] = ((color.b() as u16 * alpha + dst_b * inv) / 255) as u8;
    canvas[offset + 1] = ((color.g() as u16 * alpha + dst_g * inv) / 255) as u8;
    canvas[offset + 2] = ((color.r() as u16 * alpha + dst_r * inv) / 255) as u8;
    canvas[offset + 3] = (alpha + dst_a * inv / 255).min(255) as u8;
}
