use crate::capture::CroppedImage;
use crate::snip::annotate::{AnnotationDoc, AnnotationOp};
use windows::core::w;
use windows::Win32::Foundation::{COLORREF, RECT};
use windows::Win32::Graphics::Gdi::{
    CreateCompatibleBitmap, CreateCompatibleDC, CreateFontW, DeleteDC, DeleteObject, DrawTextW,
    FillRect, GetDC, GetDIBits, GetStockObject, ReleaseDC, SelectObject, SetBkMode, SetTextColor,
    ANTIALIASED_QUALITY, BITMAPINFO, BITMAPINFOHEADER, BI_RGB, BLACK_BRUSH, CLIP_DEFAULT_PRECIS,
    DEFAULT_CHARSET, DIB_RGB_COLORS, DT_CALCRECT, DT_LEFT, DT_NOCLIP, DT_SINGLELINE, DT_TOP,
    FF_SWISS, FW_SEMIBOLD, HBITMAP, HBRUSH, HDC, HFONT, HGDIOBJ, OUT_DEFAULT_PRECIS, TRANSPARENT,
};

/// Flattens all vector, mosaic, and text annotations directly onto `cropped.rgba` in-place.
pub fn composite_annotations(cropped: &mut CroppedImage, doc: &AnnotationDoc) {
    if doc.is_empty() {
        return;
    }
    let ox = cropped.screen_rect.x as f32;
    let oy = cropped.screen_rect.y as f32;

    for op in &doc.ops {
        match op {
            AnnotationOp::Mosaic { rect, block_size } => {
                apply_mosaic(
                    cropped,
                    rect.x - cropped.screen_rect.x,
                    rect.y - cropped.screen_rect.y,
                    rect.width,
                    rect.height,
                    (*block_size).max(4) as i32,
                );
            }
            AnnotationOp::Rect {
                from,
                to,
                color,
                stroke_px,
            } => {
                let x1 = (from.x as f32 - ox).min(to.x as f32 - ox);
                let y1 = (from.y as f32 - oy).min(to.y as f32 - oy);
                let x2 = (from.x as f32 - ox).max(to.x as f32 - ox);
                let y2 = (from.y as f32 - oy).max(to.y as f32 - oy);
                draw_rect_stroke(cropped, x1, y1, x2, y2, *color, (*stroke_px).max(1.0));
            }
            AnnotationOp::Arrow {
                from,
                to,
                color,
                stroke_px,
            } => {
                draw_arrow(
                    cropped,
                    from.x as f32 - ox,
                    from.y as f32 - oy,
                    to.x as f32 - ox,
                    to.y as f32 - oy,
                    *color,
                    (*stroke_px).max(1.5),
                );
            }
            AnnotationOp::Freehand {
                points,
                color,
                stroke_px,
                highlighter,
            } => {
                let mut col = *color;
                if *highlighter {
                    col[3] = 96;
                }
                let radius = (*stroke_px * 0.5).max(0.75);
                if points.len() == 1 {
                    draw_filled_disc(
                        cropped,
                        points[0].x as f32 - ox,
                        points[0].y as f32 - oy,
                        radius,
                        col,
                    );
                } else {
                    for seg in points.windows(2) {
                        draw_thick_segment(
                            cropped,
                            seg[0].x as f32 - ox,
                            seg[0].y as f32 - oy,
                            seg[1].x as f32 - ox,
                            seg[1].y as f32 - oy,
                            radius,
                            col,
                        );
                    }
                }
            }
            AnnotationOp::Text {
                pos,
                content,
                color,
                font_px,
            } => {
                if !content.is_empty() {
                    draw_text_gdi(
                        cropped,
                        pos.x - cropped.screen_rect.x,
                        pos.y - cropped.screen_rect.y,
                        content,
                        *color,
                        (*font_px).max(10.0),
                    );
                }
            }
        }
    }
}

#[inline]
fn blend_pixel(cropped: &mut CroppedImage, x: i32, y: i32, color: [u8; 4], coverage: f32) {
    if x < 0 || y < 0 || x >= cropped.width as i32 || y >= cropped.height as i32 {
        return;
    }
    let alpha = ((color[3] as f32 / 255.0) * coverage).clamp(0.0, 1.0);
    if alpha <= 0.001 {
        return;
    }
    let idx = ((y as usize) * (cropped.width as usize) + (x as usize)) * 4;
    let inv = 1.0 - alpha;
    cropped.rgba[idx] = (color[0] as f32 * alpha + cropped.rgba[idx] as f32 * inv).round() as u8;
    cropped.rgba[idx + 1] =
        (color[1] as f32 * alpha + cropped.rgba[idx + 1] as f32 * inv).round() as u8;
    cropped.rgba[idx + 2] =
        (color[2] as f32 * alpha + cropped.rgba[idx + 2] as f32 * inv).round() as u8;
    cropped.rgba[idx + 3] = 255;
}

fn apply_mosaic(
    cropped: &mut CroppedImage,
    rx: i32,
    ry: i32,
    rw: i32,
    rh: i32,
    block: i32,
) {
    let x0 = rx.clamp(0, cropped.width as i32);
    let y0 = ry.clamp(0, cropped.height as i32);
    let x1 = (rx + rw).clamp(0, cropped.width as i32);
    let y1 = (ry + rh).clamp(0, cropped.height as i32);
    let w = cropped.width as usize;

    let mut by = y0;
    while by < y1 {
        let by_end = (by + block).min(y1);
        let mut bx = x0;
        while bx < x1 {
            let bx_end = (bx + block).min(x1);
            let mut sum_r = 0u64;
            let mut sum_g = 0u64;
            let mut sum_b = 0u64;
            let mut count = 0u64;
            for py in by..by_end {
                for px in bx..bx_end {
                    let idx = (py as usize * w + px as usize) * 4;
                    sum_r += cropped.rgba[idx] as u64;
                    sum_g += cropped.rgba[idx + 1] as u64;
                    sum_b += cropped.rgba[idx + 2] as u64;
                    count += 1;
                }
            }
            if let Some(avg_r) = sum_r.checked_div(count) {
                let avg = [
                    avg_r as u8,
                    (sum_g / count) as u8,
                    (sum_b / count) as u8,
                    255,
                ];
                for py in by..by_end {
                    for px in bx..bx_end {
                        let idx = (py as usize * w + px as usize) * 4;
                        cropped.rgba[idx..idx + 4].copy_from_slice(&avg);
                    }
                }
            }
            bx += block;
        }
        by += block;
    }
}

fn draw_rect_stroke(
    cropped: &mut CroppedImage,
    x1: f32,
    y1: f32,
    x2: f32,
    y2: f32,
    color: [u8; 4],
    stroke: f32,
) {
    let half = stroke * 0.5;
    let min_x = (x1 - half - 1.0).floor() as i32;
    let max_x = (x2 + half + 1.0).ceil() as i32;
    let min_y = (y1 - half - 1.0).floor() as i32;
    let max_y = (y2 + half + 1.0).ceil() as i32;

    for y in min_y..=max_y {
        let fy = y as f32 + 0.5;
        for x in min_x..=max_x {
            let fx = x as f32 + 0.5;
            let dx = (x1 - fx).max(0.0).max(fx - x2);
            let dy = (y1 - fy).max(0.0).max(fy - y2);
            let outside_dist = (dx * dx + dy * dy).sqrt();
            let inside_dist = (fx - x1).min(x2 - fx).min(fy - y1).min(y2 - fy);
            let edge_dist = if outside_dist > 0.0 {
                outside_dist
            } else {
                inside_dist.max(0.0)
            };
            let cov = (half + 0.5 - edge_dist).clamp(0.0, 1.0);
            if cov > 0.0 {
                blend_pixel(cropped, x, y, color, cov);
            }
        }
    }
}

fn draw_filled_disc(cropped: &mut CroppedImage, cx: f32, cy: f32, radius: f32, color: [u8; 4]) {
    let min_x = (cx - radius - 1.0).floor() as i32;
    let max_x = (cx + radius + 1.0).ceil() as i32;
    let min_y = (cy - radius - 1.0).floor() as i32;
    let max_y = (cy + radius + 1.0).ceil() as i32;
    for y in min_y..=max_y {
        for x in min_x..=max_x {
            let dx = (x as f32 + 0.5) - cx;
            let dy = (y as f32 + 0.5) - cy;
            let dist = (dx * dx + dy * dy).sqrt();
            let cov = (radius + 0.5 - dist).clamp(0.0, 1.0);
            if cov > 0.0 {
                blend_pixel(cropped, x, y, color, cov);
            }
        }
    }
}

fn draw_thick_segment(
    cropped: &mut CroppedImage,
    x1: f32,
    y1: f32,
    x2: f32,
    y2: f32,
    radius: f32,
    color: [u8; 4],
) {
    let min_x = (x1.min(x2) - radius - 1.0).floor() as i32;
    let max_x = (x1.max(x2) + radius + 1.0).ceil() as i32;
    let min_y = (y1.min(y2) - radius - 1.0).floor() as i32;
    let max_y = (y1.max(y2) + radius + 1.0).ceil() as i32;

    let vx = x2 - x1;
    let vy = y2 - y1;
    let len_sq = vx * vx + vy * vy;

    for y in min_y..=max_y {
        let py = y as f32 + 0.5;
        for x in min_x..=max_x {
            let px = x as f32 + 0.5;
            let t = if len_sq > 1e-6 {
                (((px - x1) * vx + (py - y1) * vy) / len_sq).clamp(0.0, 1.0)
            } else {
                0.0
            };
            let cx = x1 + vx * t;
            let cy = y1 + vy * t;
            let dist = ((px - cx).powi(2) + (py - cy).powi(2)).sqrt();
            let cov = (radius + 0.5 - dist).clamp(0.0, 1.0);
            if cov > 0.0 {
                blend_pixel(cropped, x, y, color, cov);
            }
        }
    }
}

fn draw_arrow(
    cropped: &mut CroppedImage,
    x1: f32,
    y1: f32,
    x2: f32,
    y2: f32,
    color: [u8; 4],
    width: f32,
) {
    let dx = x2 - x1;
    let dy = y2 - y1;
    let len = (dx * dx + dy * dy).sqrt();
    if len < 2.0 {
        return;
    }
    let ux = dx / len;
    let uy = dy / len;
    let px = -uy;
    let py = ux;

    let head_len = (width * 3.6).clamp(11.0, 36.0).min(len * 0.65);
    let head_half_w = head_len * 0.58;

    let shaft_end_x = x2 - ux * (head_len * 0.72);
    let shaft_end_y = y2 - uy * (head_len * 0.72);
    draw_thick_segment(cropped, x1, y1, shaft_end_x, shaft_end_y, width * 0.5, color);

    let base_x = x2 - ux * head_len;
    let base_y = y2 - uy * head_len;
    let ax = x2;
    let ay = y2;
    let bx = base_x + px * head_half_w;
    let by = base_y + py * head_half_w;
    let cx = base_x - px * head_half_w;
    let cy = base_y - py * head_half_w;
    draw_filled_triangle(cropped, (ax, ay), (bx, by), (cx, cy), color);
}

fn draw_filled_triangle(
    cropped: &mut CroppedImage,
    a: (f32, f32),
    b: (f32, f32),
    c: (f32, f32),
    color: [u8; 4],
) {
    let min_x = a.0.min(b.0).min(c.0).floor() as i32 - 1;
    let max_x = a.0.max(b.0).max(c.0).ceil() as i32 + 1;
    let min_y = a.1.min(b.1).min(c.1).floor() as i32 - 1;
    let max_y = a.1.max(b.1).max(c.1).ceil() as i32 + 1;

    let edge = |p: (f32, f32), q: (f32, f32), r: (f32, f32)| -> f32 {
        (r.0 - p.0) * (q.1 - p.1) - (r.1 - p.1) * (q.0 - p.0)
    };
    let area = edge(a, b, c);
    if area.abs() < 1e-4 {
        return;
    }

    const OFFSETS: [(f32, f32); 4] = [(0.25, 0.25), (0.75, 0.25), (0.25, 0.75), (0.75, 0.75)];
    for y in min_y..=max_y {
        for x in min_x..=max_x {
            let mut hits = 0u8;
            for (ox, oy) in OFFSETS {
                let p = (x as f32 + ox, y as f32 + oy);
                let w0 = edge(b, c, p);
                let w1 = edge(c, a, p);
                let w2 = edge(a, b, p);
                let inside = if area > 0.0 {
                    w0 >= 0.0 && w1 >= 0.0 && w2 >= 0.0
                } else {
                    w0 <= 0.0 && w1 <= 0.0 && w2 <= 0.0
                };
                if inside {
                    hits += 1;
                }
            }
            if hits > 0 {
                blend_pixel(cropped, x, y, color, hits as f32 / 4.0);
            }
        }
    }
}

fn draw_text_gdi(
    cropped: &mut CroppedImage,
    x: i32,
    y: i32,
    text: &str,
    color: [u8; 4],
    font_px: f32,
) {
    let mut wide: Vec<u16> = text.encode_utf16().collect();
    if wide.is_empty() {
        return;
    }

    // SAFETY: Standard Win32 off-screen GDI DIB text rasterization; all handles are explicitly freed.
    unsafe {
        let screen_dc = GetDC(None);
        if screen_dc.is_invalid() {
            return;
        }
        let mem_dc: HDC = CreateCompatibleDC(Some(screen_dc));
        if mem_dc.is_invalid() {
            ReleaseDC(None, screen_dc);
            return;
        }

        let font: HFONT = CreateFontW(
            -(font_px.round() as i32),
            0,
            0,
            0,
            FW_SEMIBOLD.0 as i32,
            0,
            0,
            0,
            DEFAULT_CHARSET,
            OUT_DEFAULT_PRECIS,
            CLIP_DEFAULT_PRECIS,
            ANTIALIASED_QUALITY,
            FF_SWISS.0 as u32,
            w!("Segoe UI"),
        );
        let prev_font = SelectObject(mem_dc, HGDIOBJ(font.0));

        let mut calc_rect = RECT::default();
        let _ = DrawTextW(
            mem_dc,
            &mut wide,
            &mut calc_rect,
            DT_CALCRECT | DT_LEFT | DT_TOP | DT_SINGLELINE | DT_NOCLIP,
        );
        let tw = (calc_rect.right - calc_rect.left + 6).clamp(1, 4096);
        let th = (calc_rect.bottom - calc_rect.top + 6).clamp(1, 1024);

        let bmp: HBITMAP = CreateCompatibleBitmap(screen_dc, tw, th);
        if !bmp.is_invalid() {
            let prev_bmp = SelectObject(mem_dc, HGDIOBJ(bmp.0));
            let fill_rc = RECT {
                left: 0,
                top: 0,
                right: tw,
                bottom: th,
            };
            let black_brush = HBRUSH(GetStockObject(BLACK_BRUSH).0);
            let _ = FillRect(mem_dc, &fill_rc, black_brush);
            SetBkMode(mem_dc, TRANSPARENT);
            SetTextColor(mem_dc, COLORREF(0x00FF_FFFF));

            let mut draw_rc = RECT {
                left: 2,
                top: 2,
                right: tw,
                bottom: th,
            };
            let _ = DrawTextW(
                mem_dc,
                &mut wide,
                &mut draw_rc,
                DT_LEFT | DT_TOP | DT_SINGLELINE | DT_NOCLIP,
            );

            let mut bmi = BITMAPINFO {
                bmiHeader: BITMAPINFOHEADER {
                    biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                    biWidth: tw,
                    biHeight: -th,
                    biPlanes: 1,
                    biBitCount: 32,
                    biCompression: BI_RGB.0,
                    ..Default::default()
                },
                ..Default::default()
            };
            let mut mask = vec![0u8; (tw as usize) * (th as usize) * 4];
            let got = GetDIBits(
                mem_dc,
                bmp,
                0,
                th as u32,
                Some(mask.as_mut_ptr().cast()),
                &mut bmi,
                DIB_RGB_COLORS,
            );
            let _ = SelectObject(mem_dc, prev_bmp);
            let _ = DeleteObject(HGDIOBJ(bmp.0));

            if got > 0 {
                for py in 0..th {
                    for px in 0..tw {
                        let cov = mask[((py as usize) * (tw as usize) + (px as usize)) * 4 + 2]
                            as f32
                            / 255.0;
                        if cov > 0.02 {
                            blend_pixel(
                                cropped,
                                x + px - 1,
                                y + py - 1,
                                [0, 0, 0, 150],
                                cov,
                            );
                        }
                    }
                }
                for py in 0..th {
                    for px in 0..tw {
                        let cov = mask[((py as usize) * (tw as usize) + (px as usize)) * 4 + 2]
                            as f32
                            / 255.0;
                        if cov > 0.02 {
                            blend_pixel(cropped, x + px - 2, y + py - 2, color, cov);
                        }
                    }
                }
            }
        }

        let _ = SelectObject(mem_dc, prev_font);
        let _ = DeleteObject(HGDIOBJ(font.0));
        let _ = DeleteDC(mem_dc);
        ReleaseDC(None, screen_dc);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::capture::ScreenRect;
    use crate::snip::annotate::PALETTE;
    use crate::snip::state::ScreenPt;

    fn solid_crop(w: u32, h: u32, rgba_px: [u8; 4]) -> CroppedImage {
        let mut rgba = Vec::with_capacity((w * h * 4) as usize);
        for _ in 0..(w * h) {
            rgba.extend_from_slice(&rgba_px);
        }
        CroppedImage {
            width: w,
            height: h,
            rgba,
            screen_rect: ScreenRect {
                x: 100,
                y: 200,
                width: w as i32,
                height: h as i32,
            },
        }
    }

    #[test]
    fn mosaic_averages_pixels_irreversibly() {
        let mut crop = solid_crop(16, 16, [0, 0, 0, 255]);
        for y in 0..8 {
            for x in 0..4 {
                let idx = (y * 16 + x) * 4;
                crop.rgba[idx..idx + 4].copy_from_slice(&[200, 100, 40, 255]);
            }
        }
        let mut doc = AnnotationDoc::default();
        doc.push(AnnotationOp::Mosaic {
            rect: ScreenRect {
                x: 100,
                y: 200,
                width: 8,
                height: 8,
            },
            block_size: 8,
        });
        composite_annotations(&mut crop, &doc);
        assert_eq!(&crop.rgba[0..4], &[100, 50, 20, 255]);
        assert_eq!(&crop.rgba[(7 * 16 + 7) * 4..(7 * 16 + 7) * 4 + 4], &[100, 50, 20, 255]);
        assert_eq!(&crop.rgba[(12 * 16 + 12) * 4..(12 * 16 + 12) * 4 + 4], &[0, 0, 0, 255]);
    }

    #[test]
    fn rect_arrow_and_text_composite_onto_crop_coordinates() {
        let mut crop = solid_crop(120, 80, [20, 20, 20, 255]);
        let mut doc = AnnotationDoc::default();
        doc.push(AnnotationOp::Rect {
            from: ScreenPt { x: 110, y: 210 },
            to: ScreenPt { x: 150, y: 240 },
            color: PALETTE[0],
            stroke_px: 4.0,
        });
        doc.push(AnnotationOp::Arrow {
            from: ScreenPt { x: 110, y: 260 },
            to: ScreenPt { x: 190, y: 260 },
            color: PALETTE[3],
            stroke_px: 4.0,
        });
        doc.push(AnnotationOp::Text {
            pos: ScreenPt { x: 160, y: 210 },
            content: "OK".to_string(),
            color: PALETTE[4],
            font_px: 18.0,
        });

        composite_annotations(&mut crop, &doc);

        let rect_idx = (10 * 120 + 10) * 4;
        assert!(crop.rgba[rect_idx] > 200, "expected red rect border");

        let arrow_idx = (60 * 120 + 40) * 4;
        assert!(crop.rgba[arrow_idx + 2] > 200, "expected sky arrow shaft");

        let text_lit = (10..34)
            .flat_map(|y| (60..100).map(move |x| (x, y)))
            .any(|(x, y)| crop.rgba[(y * 120 + x) * 4] > 180);
        assert!(text_lit, "expected rendered GDI text pixels");
    }
}
