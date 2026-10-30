//! Render Sixel pensado para manga.
//!
//! - Escala con Lanczos3 al tamaño exacto en píxeles del área (no "nearest").
//! - Páginas en escala de grises (la mayoría): 64 niveles de gris sin dithering, nítido y sin ruido.
//! - Páginas a color: 256 colores con dithering suave.

use image::imageops::FilterType;
use image::{DynamicImage, GrayImage};
use std::fmt::Write;

/// Niveles de gris de la paleta. Sixel define colores en porcentaje (0-100), así que más de ~100 no suma.
const GRAY_LEVELS: usize = 64;
/// No agrandar más que esto las imágenes chicas (más allá solo se ve borroso).
const MAX_UPSCALE: f32 = 2.0;

pub struct Encoded {
    pub sixel: String,
    /// Tamaño final en celdas de terminal.
    pub cols: u16,
    pub rows: u16,
    pub out: (u32, u32),
    pub gray: bool,
}

/// Escala `img` para que entre en `cols`×`rows` celdas (de `font` píxeles cada una) y lo codifica.
pub fn encode(img: &DynamicImage, cols: u16, rows: u16, font: (u16, u16)) -> Encoded {
    let (fw, fh) = (font.0 as u32, font.1 as u32);
    let (max_w, max_h) = (cols as u32 * fw, rows as u32 * fh);
    let scale = (max_w as f32 / img.width() as f32)
        .min(max_h as f32 / img.height() as f32)
        .min(MAX_UPSCALE);
    let w = ((img.width() as f32 * scale) as u32).clamp(1, max_w);
    let h = ((img.height() as f32 * scale) as u32).clamp(1, max_h);
    let resized = img.resize_exact(w, h, FilterType::Lanczos3);

    let gray = is_grayscale(&resized);
    let sixel = if gray { gray_sixel(&resized.to_luma8()) } else { color_sixel(&resized) };
    Encoded {
        sixel,
        cols: w.div_ceil(fw) as u16,
        rows: h.div_ceil(fh) as u16,
        out: (w, h),
        gray,
    }
}

/// Muestrea la imagen: si casi no hay píxeles con saturación, es una página en blanco y negro.
fn is_grayscale(img: &DynamicImage) -> bool {
    let rgb = img.to_rgb8();
    let total = rgb.pixels().len().max(1);
    let colored = rgb
        .pixels()
        .step_by(7)
        .filter(|p| {
            let [r, g, b] = p.0;
            r.abs_diff(g).max(g.abs_diff(b)).max(r.abs_diff(b)) > 24
        })
        .count();
    colored * 7 * 1000 / total < 5 // menos del 0,5% de píxeles con color
}

fn gray_sixel(img: &GrayImage) -> String {
    let (w, h) = (img.width() as usize, img.height() as usize);
    let px = img.as_raw();
    let mut s = String::with_capacity(w * h / 3);
    // P2=1: los píxeles no pintados quedan transparentes (igual pintamos todos)
    let _ = write!(s, "\x1bP0;1;0q\"1;1;{w};{h}");
    for level in 0..GRAY_LEVELS {
        let pct = level * 100 / (GRAY_LEVELS - 1);
        let _ = write!(s, "#{level};2;{pct};{pct};{pct}");
    }

    let quant: Vec<u8> = (0..=255u32).map(|v| ((v * (GRAY_LEVELS as u32 - 1) + 127) / 255) as u8).collect();
    let mut masks = vec![0u8; GRAY_LEVELS * w];
    let mut used = [false; GRAY_LEVELS];
    for band in (0..h).step_by(6) {
        masks.fill(0);
        used.fill(false);
        for dy in 0..6.min(h - band) {
            let row = &px[(band + dy) * w..(band + dy + 1) * w];
            for (x, &v) in row.iter().enumerate() {
                let c = quant[v as usize] as usize;
                masks[c * w + x] |= 1 << dy;
                used[c] = true;
            }
        }
        let mut first = true;
        for c in (0..GRAY_LEVELS).filter(|&c| used[c]) {
            if !first {
                s.push('$'); // volver al inicio de la banda para el siguiente color
            }
            first = false;
            let _ = write!(s, "#{c}");
            push_rle(&mut s, &masks[c * w..(c + 1) * w]);
        }
        s.push('-');
    }
    s.push_str("\x1b\\");
    s
}

/// Una fila de sixels con run-length; los ceros del final no hace falta emitirlos.
fn push_rle(s: &mut String, row: &[u8]) {
    let end = row.iter().rposition(|&m| m != 0).map_or(0, |i| i + 1);
    let mut x = 0;
    while x < end {
        let m = row[x];
        let run = row[x..end].iter().take_while(|&&v| v == m).count();
        let ch = (63 + m) as char;
        if run > 3 {
            let _ = write!(s, "!{run}{ch}");
        } else {
            for _ in 0..run {
                s.push(ch);
            }
        }
        x += run;
    }
}

fn color_sixel(img: &DynamicImage) -> String {
    let rgba = img.to_rgba8();
    let opts = icy_sixel::EncodeOptions { max_colors: 256, diffusion: 0.3, ..Default::default() };
    icy_sixel::sixel_encode(rgba.as_raw(), img.width() as usize, img.height() as usize, &opts)
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Compara nuestro render con el de ratatui-image (Nearest + dithering fuerte), decodificando
    /// ambos Sixel de vuelta a PNG. Uso:
    /// MANGATERM_TEST_PAGE=<imagen> MANGATERM_TEST_OUT=<dir> cargo test --release render_compare -- --nocapture
    #[test]
    fn render_compare() {
        let (Ok(page), Ok(out)) = (std::env::var("MANGATERM_TEST_PAGE"), std::env::var("MANGATERM_TEST_OUT")) else {
            return;
        };
        let out = std::path::PathBuf::from(out);
        let img = image::ImageReader::open(&page).unwrap().with_guessed_format().unwrap().decode().unwrap();
        // Ventana típica: 60 filas útiles con celdas de 10x20
        let (cols, rows, font) = (120u16, 48u16, (10u16, 20u16));

        let t = std::time::Instant::now();
        let enc = encode(&img, cols, rows, font);
        println!("nuevo: {:?} gris={} {} bytes en {:?}", enc.out, enc.gray, enc.sixel.len(), t.elapsed());
        save_decoded(&enc.sixel, &out.join("nuevo.png"));

        // Lo que hacía ratatui-image: resize Nearest + sixel con opciones por defecto
        let (w, h) = enc.out;
        let old = img.resize_exact(w, h, FilterType::Nearest).to_rgba8();
        let t = std::time::Instant::now();
        let sixel = icy_sixel::sixel_encode(old.as_raw(), w as usize, h as usize, &icy_sixel::EncodeOptions::default()).unwrap();
        println!("viejo: {} bytes en {:?}", sixel.len(), t.elapsed());
        save_decoded(&sixel, &out.join("viejo.png"));
    }

    fn save_decoded(sixel: &str, path: &std::path::Path) {
        let img = icy_sixel::SixelImage::decode(sixel.as_bytes()).unwrap();
        image::RgbaImage::from_raw(img.width as u32, img.height as u32, img.pixels).unwrap().save(path).unwrap();
    }
}
