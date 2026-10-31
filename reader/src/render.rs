//! Render Sixel pensado para manga.
//!
//! - Escala con Lanczos3 al tamaño exacto en píxeles del área (no "nearest").
//! - Páginas en escala de grises (la mayoría): 64 niveles de gris sin dithering, nítido y sin ruido.
//! - Páginas a color: 256 colores con dithering suave.

use image::imageops::FilterType;
use image::{DynamicImage, GrayImage};
use serde::{Deserialize, Serialize};
use std::fmt::Write;

/// Niveles de gris de la paleta. Sixel define colores en porcentaje (0-100), así que más de ~100 no suma.
const GRAY_LEVELS: usize = 64;
/// No agrandar más que esto las imágenes chicas (más allá solo se ve borroso).
const MAX_UPSCALE: f32 = 2.0;

/// Cómo se ajusta la página al área del lector (antes del zoom).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Fit {
    /// La página entera en pantalla.
    #[default]
    Page,
    /// Todo el ancho; se desplaza en vertical (webtoons).
    Width,
    /// Todo el alto.
    Height,
    /// Un píxel de la imagen, un píxel de pantalla.
    Original,
}

impl Fit {
    pub fn next(self) -> Self {
        match self {
            Fit::Page => Fit::Width,
            Fit::Width => Fit::Height,
            Fit::Height => Fit::Original,
            Fit::Original => Fit::Page,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Fit::Page => tr!("página entera", "whole page"),
            Fit::Width => tr!("ajustar al ancho", "fit width"),
            Fit::Height => tr!("ajustar al alto", "fit height"),
            Fit::Original => tr!("tamaño original", "original size"),
        }
    }
}

/// Niveles de zoom (en %) que recorren `+` y `-`.
pub const ZOOMS: [u16; 7] = [100, 125, 150, 200, 250, 300, 400];

/// Qué parte de la página se ve: ajuste, zoom (en %) y posición dentro de lo que sobresale
/// del área (0 = arriba/izquierda, 1000 = abajo/derecha).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct View {
    pub fit: Fit,
    pub zoom: u16,
    pub x: u16,
    pub y: u16,
}

impl Default for View {
    fn default() -> Self {
        View { fit: Fit::Page, zoom: 100, x: 0, y: 0 }
    }
}

impl View {
    /// La misma vista desde el principio de la página (para las que se precargan).
    pub fn at_start(self) -> Self {
        View { x: 0, y: 0, ..self }
    }
}

/// La parte visible de la página, ya escalada a los píxeles que se van a pintar.
pub struct Framed {
    pub img: DynamicImage,
    /// Cuántos píxeles de la página escalada quedan fuera del área (lo que se puede desplazar).
    pub overflow: (u32, u32),
}

/// Escala `img` según `view` y recorta lo que cabe en `max_w`×`max_h` píxeles.
pub fn frame(img: &DynamicImage, max_w: u32, max_h: u32, view: View) -> Framed {
    let (max_w, max_h) = (max_w.max(1), max_h.max(1));
    let (iw, ih) = (img.width().max(1) as f32, img.height().max(1) as f32);
    let base = match view.fit {
        Fit::Page => (max_w as f32 / iw).min(max_h as f32 / ih).min(MAX_UPSCALE),
        Fit::Width => max_w as f32 / iw,
        Fit::Height => max_h as f32 / ih,
        Fit::Original => 1.0,
    };
    let scale = base * view.zoom.max(10) as f32 / 100.0;
    let (full_w, full_h) = (((iw * scale) as u32).max(1), ((ih * scale) as u32).max(1));
    let (w, h) = (full_w.min(max_w), full_h.min(max_h));
    let overflow = (full_w - w, full_h - h);
    // Región equivalente en la imagen original
    let ox = (overflow.0 as u64 * view.x.min(1000) as u64 / 1000) as f32;
    let oy = (overflow.1 as u64 * view.y.min(1000) as u64 / 1000) as f32;
    let sx = ((ox / scale) as u32).min(img.width().saturating_sub(1));
    let sy = ((oy / scale) as u32).min(img.height().saturating_sub(1));
    let sw = ((w as f32 / scale).round() as u32).clamp(1, img.width() - sx);
    let sh = ((h as f32 / scale).round() as u32).clamp(1, img.height() - sy);
    let visible = if (sx, sy, sw, sh) == (0, 0, img.width(), img.height()) { img.clone() } else { img.crop_imm(sx, sy, sw, sh) };
    Framed { img: visible.resize_exact(w, h, FilterType::Lanczos3), overflow }
}

pub struct Encoded {
    pub sixel: String,
    /// Tamaño final en celdas de terminal.
    pub cols: u16,
    pub rows: u16,
    pub out: (u32, u32),
    pub overflow: (u32, u32),
    pub gray: bool,
}

/// Encaja `img` en `cols`×`rows` celdas (de `font` píxeles cada una) según `view` y lo codifica.
pub fn encode(img: &DynamicImage, cols: u16, rows: u16, font: (u16, u16), view: View) -> Encoded {
    let (fw, fh) = (font.0 as u32, font.1 as u32);
    let Framed { img: resized, overflow } = frame(img, cols as u32 * fw, rows as u32 * fh, view);
    let (w, h) = (resized.width(), resized.height());

    let gray = is_grayscale(&resized);
    let sixel = if gray { gray_sixel(&resized.to_luma8()) } else { color_sixel(&resized) };
    Encoded {
        sixel,
        cols: w.div_ceil(fw) as u16,
        rows: h.div_ceil(fh) as u16,
        out: (w, h),
        overflow,
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
    /// HANTHO_TEST_PAGE=<imagen> HANTHO_TEST_OUT=<dir> cargo test --release render_compare -- --nocapture
    #[test]
    fn render_compare() {
        let (Ok(page), Ok(out)) = (std::env::var("HANTHO_TEST_PAGE"), std::env::var("HANTHO_TEST_OUT")) else {
            return;
        };
        let out = std::path::PathBuf::from(out);
        let img = image::ImageReader::open(&page).unwrap().with_guessed_format().unwrap().decode().unwrap();
        // Ventana típica: 60 filas útiles con celdas de 10x20
        let (cols, rows, font) = (120u16, 48u16, (10u16, 20u16));

        let t = std::time::Instant::now();
        let enc = encode(&img, cols, rows, font, View::default());
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

    /// Guarda cómo queda una página con cada ajuste y zoom (decodificando el Sixel). Uso:
    /// HANTHO_TEST_PAGE=<imagen> HANTHO_TEST_OUT=<dir> cargo test --release views_preview
    #[test]
    fn views_preview() {
        let (Ok(page), Ok(out)) = (std::env::var("HANTHO_TEST_PAGE"), std::env::var("HANTHO_TEST_OUT")) else {
            return;
        };
        let out = std::path::PathBuf::from(out);
        let img = image::ImageReader::open(&page).unwrap().with_guessed_format().unwrap().decode().unwrap();
        let views = [
            ("pagina", View::default()),
            ("ancho", View { fit: Fit::Width, ..View::default() }),
            ("ancho_abajo", View { fit: Fit::Width, y: 1000, ..View::default() }),
            ("zoom200_centro", View { zoom: 200, x: 500, y: 500, ..View::default() }),
            ("original", View { fit: Fit::Original, ..View::default() }),
        ];
        for (name, view) in views {
            let t = std::time::Instant::now();
            let enc = encode(&img, 120, 29, (10, 20), view);
            println!("{name}: {:?} sobra {:?} en {:?}", enc.out, enc.overflow, t.elapsed());
            save_decoded(&enc.sixel, &out.join(format!("{name}.png")));
        }
    }

    #[test]
    fn frame_views() {
        let img = DynamicImage::new_luma8(800, 1200);
        // Página entera: cabe sin desbordar
        let f = frame(&img, 1000, 600, View::default());
        assert_eq!((f.img.width(), f.img.height(), f.overflow), (400, 600, (0, 0)));
        // Al ancho: ocupa los 1000 px y sobra alto para desplazarse
        let f = frame(&img, 1000, 600, View { fit: Fit::Width, ..View::default() });
        assert_eq!((f.img.width(), f.img.height(), f.overflow), (1000, 600, (0, 900)));
        // Zoom 200% sobre página entera: sobresale en los dos ejes; abajo a la derecha también vale
        let f = frame(&img, 1000, 600, View { zoom: 200, x: 1000, y: 1000, ..View::default() });
        assert_eq!((f.img.width(), f.img.height(), f.overflow), (800, 600, (0, 600)));
        let f = frame(&img, 500, 600, View { fit: Fit::Original, x: 1000, y: 1000, ..View::default() });
        assert_eq!((f.img.width(), f.img.height(), f.overflow), (500, 600, (300, 600)));
    }

    fn save_decoded(sixel: &str, path: &std::path::Path) {
        let img = icy_sixel::SixelImage::decode(sixel.as_bytes()).unwrap();
        image::RgbaImage::from_raw(img.width as u32, img.height as u32, img.pixels).unwrap().save(path).unwrap();
    }
}
