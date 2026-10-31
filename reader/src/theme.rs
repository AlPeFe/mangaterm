//! Paleta y estilos de la interfaz: rosa pastel como acento, lavanda y menta de apoyo.

use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style, Stylize};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, List, Paragraph};
use ratatui::Frame;

pub const PINK: Color = Color::Rgb(255, 182, 213);
pub const PINK_DEEP: Color = Color::Rgb(240, 128, 170);
pub const PINK_DIM: Color = Color::Rgb(150, 96, 124);
pub const LAVENDER: Color = Color::Rgb(200, 178, 255);
pub const MINT: Color = Color::Rgb(160, 236, 200);
pub const PEACH: Color = Color::Rgb(255, 204, 170);
pub const TEXT: Color = Color::Rgb(244, 232, 240);
pub const MUTED: Color = Color::Rgb(160, 140, 156);
pub const FAINT: Color = Color::Rgb(100, 86, 98);
/// Texto sobre fondo rosa (selecciones, etiquetas).
pub const ON_PINK: Color = Color::Rgb(54, 26, 44);
pub const ERROR: Color = Color::Rgb(255, 130, 140);

pub fn rgb(c: Color) -> (f32, f32, f32) {
    match c {
        Color::Rgb(r, g, b) => (r as f32, g as f32, b as f32),
        _ => (255.0, 255.0, 255.0),
    }
}

/// Interpola entre dos colores RGB (t en 0..=1).
pub fn mix(a: Color, b: Color, t: f32) -> Color {
    let ((ar, ag, ab), (br, bg, bb)) = (rgb(a), rgb(b));
    let t = t.clamp(0.0, 1.0);
    Color::Rgb((ar + (br - ar) * t) as u8, (ag + (bg - ag) * t) as u8, (ab + (bb - ab) * t) as u8)
}

/// Bloque con borde redondeado rosa y título con icono.
pub fn block(title: &str) -> Block<'_> {
    Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(Style::new().fg(PINK_DIM))
        .title(Line::from(vec![Span::raw(" "), Span::raw(title).fg(PINK).bold(), Span::raw(" ")]))
}

/// Igual pero resaltado (el panel con foco).
pub fn block_focused(title: &str) -> Block<'_> {
    block(title).border_style(Style::new().fg(PINK))
}

pub fn highlight(list: List<'_>) -> List<'_> {
    list.highlight_style(Style::new().bg(PINK).fg(ON_PINK).add_modifier(Modifier::BOLD))
        .highlight_symbol(" ✿ ")
}

/// Barra inferior: etiqueta de pantalla, mensaje y atajos (`tecla descripción · …`).
pub fn status_bar(f: &mut Frame, area: Rect, badge: &str, message: &str, keys: &[(&str, &str)]) {
    let mut hints: Vec<Span> = Vec::new();
    for (i, (key, desc)) in keys.iter().enumerate() {
        if i > 0 {
            hints.push(Span::raw("  ").fg(FAINT));
        }
        hints.push(Span::raw(*key).fg(PINK).bold());
        hints.push(Span::raw(format!(" {desc}")).fg(MUTED));
    }
    let mut hints_width: usize = hints.iter().map(|s| s.content.chars().count()).sum::<usize>() + 1;
    let badge = format!(" {badge} ");
    let badge_width = badge.chars().count() + 1;
    // En paneles estrechos (p. ej. al lado del chat de un agente) el mensaje importa más que los atajos
    if (area.width as usize) < hints_width + badge_width + 16 {
        hints.clear();
        hints_width = 0;
    }

    let msg_width = (area.width as usize).saturating_sub(hints_width + badge_width);
    let message: String = if message.chars().count() > msg_width {
        let mut cut: String = message.chars().take(msg_width.saturating_sub(2)).collect();
        cut.push('…');
        cut
    } else {
        message.to_string()
    };
    let pad = msg_width.saturating_sub(message.chars().count());

    let mut spans = vec![Span::raw(badge).bg(PINK).fg(ON_PINK).bold(), Span::raw(" "), Span::raw(message).fg(TEXT)];
    spans.push(Span::raw(" ".repeat(pad)));
    spans.extend(hints);
    f.render_widget(Paragraph::new(Line::from(spans)), area);
}

/// Spinner braille para "cargando…".
pub fn spinner(tick: u64) -> &'static str {
    const FRAMES: [&str; 10] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
    FRAMES[(tick as usize) % FRAMES.len()]
}

pub fn status_color(status: crate::library::Status) -> Color {
    use crate::library::Status::*;
    match status {
        Current => PINK,
        Planning => LAVENDER,
        Completed => MINT,
        Paused => PEACH,
        Dropped => MUTED,
        Repeating => PINK_DEEP,
    }
}

/// Vuelca un buffer de ratatui a JSON (filas de [carácter, fg, bg]) para previsualizar el diseño.
#[cfg(test)]
pub fn dump_buffer(buf: &ratatui::buffer::Buffer, path: &str) {
    let hex = |c: Color| match c {
        Color::Rgb(r, g, b) => format!("#{r:02x}{g:02x}{b:02x}"),
        _ => String::new(),
    };
    let rows: Vec<Vec<(String, String, String)>> = (0..buf.area.height)
        .map(|y| {
            (0..buf.area.width)
                .map(|x| {
                    let c = &buf[(x, y)];
                    (c.symbol().to_string(), hex(c.fg), hex(c.bg))
                })
                .collect()
        })
        .collect();
    std::fs::write(path, serde_json::to_string(&rows).unwrap()).unwrap();
}
