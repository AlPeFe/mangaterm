//! Pantalla de inicio: logo con degradé animado, pétalos de sakura cayendo y menú principal.

use crate::theme::{self, mix, FAINT, LAVENDER, MUTED, ON_PINK, PEACH, PINK, PINK_DEEP, PINK_DIM, TEXT};
use ratatui::buffer::Buffer;
use ratatui::layout::{Constraint, Flex, Layout, Rect};
use ratatui::style::{Color, Style, Stylize};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Clear, Paragraph};
use ratatui::Frame;
use std::time::Instant;

/// Letras en estilo "ANSI Shadow" (6 filas). Se arman por letra para que todas las filas midan igual.
const GLYPHS: [(char, [&str; 6]); 7] = [
    ('m', ["███╗   ███╗", "████╗ ████║", "██╔████╔██║", "██║╚██╔╝██║", "██║ ╚═╝ ██║", "╚═╝     ╚═╝"]),
    ('a', [" █████╗ ", "██╔══██╗", "███████║", "██╔══██║", "██║  ██║", "╚═╝  ╚═╝"]),
    ('n', ["███╗   ██╗", "████╗  ██║", "██╔██╗ ██║", "██║╚██╗██║", "██║ ╚████║", "╚═╝  ╚═══╝"]),
    ('g', [" ██████╗ ", "██╔════╝ ", "██║  ███╗", "██║   ██║", "╚██████╔╝", " ╚═════╝ "]),
    ('t', ["████████╗", "╚══██╔══╝", "   ██║   ", "   ██║   ", "   ██║   ", "   ╚═╝   "]),
    ('e', ["███████╗", "██╔════╝", "█████╗  ", "██╔══╝  ", "███████╗", "╚══════╝"]),
    ('r', ["██████╗ ", "██╔══██╗", "██████╔╝", "██╔══██╗", "██║  ██║", "╚═╝  ╚═╝"]),
];

fn logo_rows(word: &str) -> Vec<Vec<char>> {
    let mut rows = vec![Vec::new(); 6];
    for ch in word.chars() {
        if let Some((_, glyph)) = GLYPHS.iter().find(|(c, _)| *c == ch) {
            for (row, part) in rows.iter_mut().zip(glyph) {
                row.extend(part.chars());
            }
        }
    }
    rows
}

pub struct MenuItem {
    pub icon: &'static str,
    pub label: &'static str,
    pub detail: String,
}

struct Petal {
    x: f32,
    y: f32,
    speed: f32,
    sway: f32,
    phase: f32,
    glyph: char,
    color: Color,
}

pub struct Home {
    started: Instant,
    petals: Vec<Petal>,
    seed: u64,
    logo: Vec<Vec<char>>,
    pub selected: usize,
}

impl Home {
    pub fn new() -> Self {
        Home { started: Instant::now(), petals: Vec::new(), seed: 0x9e37_79b9_7f4a_7c15, logo: logo_rows("mangaterm"), selected: 0 }
    }

    /// Generador pseudoaleatorio mínimo (xorshift): no hace falta nada criptográfico para pétalos.
    fn rand(&mut self) -> f32 {
        self.seed ^= self.seed << 13;
        self.seed ^= self.seed >> 7;
        self.seed ^= self.seed << 17;
        (self.seed % 10_000) as f32 / 10_000.0
    }

    fn spawn_petal(&mut self, area: Rect, anywhere: bool) -> Petal {
        let glyphs = ['✿', '❀', '·', '∗', '❁', '˚'];
        let colors = [PINK, PINK_DEEP, LAVENDER, PEACH, PINK_DIM];
        let gi = (self.rand() * glyphs.len() as f32) as usize % glyphs.len();
        let ci = (self.rand() * colors.len() as f32) as usize % colors.len();
        Petal {
            x: self.rand() * area.width as f32,
            y: if anywhere { self.rand() * area.height as f32 } else { -1.0 },
            speed: 0.08 + self.rand() * 0.18,
            sway: 0.2 + self.rand() * 0.6,
            phase: self.rand() * std::f32::consts::TAU,
            glyph: glyphs[gi],
            color: colors[ci],
        }
    }

    /// Avanza la animación un paso (se llama en cada tick).
    pub fn tick(&mut self, area: Rect) {
        let target = (area.width as usize * area.height as usize / 90).clamp(8, 60);
        while self.petals.len() < target {
            let p = self.spawn_petal(area, true);
            self.petals.push(p);
        }
        self.petals.truncate(target);
        let t = self.started.elapsed().as_secs_f32();
        for i in 0..self.petals.len() {
            let p = &mut self.petals[i];
            p.y += p.speed;
            p.x += (t * 1.3 + p.phase).sin() * p.sway * 0.25;
            if p.y >= area.height as f32 || p.x < -1.0 || p.x > area.width as f32 + 1.0 {
                self.petals[i] = self.spawn_petal(area, false);
            }
        }
    }

    pub fn draw(&mut self, f: &mut Frame, items: &[MenuItem], footer: Line<'_>) {
        let area = f.area();
        let t = self.started.elapsed().as_secs_f32();
        draw_petals(f.buffer_mut(), area, &self.petals);

        let logo_w = self.logo[0].len() as u16;
        let big = area.width >= logo_w + 4 && area.height >= 26;
        let logo_h = if big { 6 } else { 1 };
        let menu_h = items.len() as u16 + 2;
        let [_, logo_area, tag_area, _, menu_area, _, footer_area] = Layout::vertical([
            Constraint::Fill(1),
            Constraint::Length(logo_h),
            Constraint::Length(2),
            Constraint::Length(1),
            Constraint::Length(menu_h),
            Constraint::Fill(1),
            Constraint::Length(1),
        ])
        .areas(area);

        // Intro: el logo se revela de izquierda a derecha en el primer segundo
        let reveal = (t / 0.9).min(1.0);
        if big {
            let [logo_area] = Layout::horizontal([Constraint::Length(logo_w)]).flex(Flex::Center).areas(logo_area);
            draw_logo(f.buffer_mut(), logo_area, &self.logo, t, reveal);
        } else {
            let title = Line::from(vec![Span::raw("✿ ").fg(PINK), Span::raw("mangaterm").fg(PINK).bold(), Span::raw(" ✿").fg(PINK)]);
            f.render_widget(Paragraph::new(title).centered(), logo_area);
        }

        let tagline = Line::from(vec![
            Span::raw("manga en tu terminal").fg(mix(theme::FAINT, MUTED, reveal)).italic(),
            Span::raw("  ♡").fg(mix(theme::FAINT, PINK, ((t * 2.0).sin() + 1.0) / 2.0)),
        ]);
        f.render_widget(Paragraph::new(tagline).centered(), tag_area);

        // El menú aparece suavemente después del logo
        let fade = ((t - 0.6) / 0.5).clamp(0.0, 1.0);
        let menu_w = 46.min(area.width.saturating_sub(4));
        let [menu_area] = Layout::horizontal([Constraint::Length(menu_w)]).flex(Flex::Center).areas(menu_area);
        f.render_widget(Clear, menu_area);
        f.render_widget(theme::block("menú").border_style(Style::new().fg(mix(theme::FAINT, PINK_DIM, fade))), menu_area);
        let inner = menu_area.inner(ratatui::layout::Margin::new(1, 1));
        for (i, item) in items.iter().enumerate() {
            let row = Rect::new(inner.x, inner.y + i as u16, inner.width, 1);
            let selected = i == self.selected;
            let pulse = ((t * 3.0).sin() + 1.0) / 2.0;
            let paragraph = if selected {
                // La fila elegida entera en rosa, "respirando" suavemente
                Paragraph::new(Line::from(vec![
                    Span::raw(" ❯ ").fg(ON_PINK).bold(),
                    Span::raw(format!("{} ", item.icon)).fg(ON_PINK),
                    Span::raw(item.label).fg(ON_PINK).bold(),
                    Span::raw(format!("  {}", item.detail)).fg(Color::Rgb(110, 60, 90)),
                ]))
                .style(Style::new().bg(mix(PINK, Color::Rgb(255, 214, 232), pulse * 0.6)))
            } else {
                Paragraph::new(Line::from(vec![
                    Span::raw(format!("   {} ", item.icon)).fg(mix(FAINT, LAVENDER, fade)),
                    Span::raw(item.label).fg(mix(FAINT, TEXT, fade)),
                    Span::raw(format!("  {}", item.detail)).fg(mix(FAINT, MUTED, fade)),
                ]))
            };
            f.render_widget(paragraph, row);
        }

        f.render_widget(Paragraph::new(footer).centered(), footer_area);
    }
}

fn draw_petals(buf: &mut Buffer, area: Rect, petals: &[Petal]) {
    for p in petals {
        let (x, y) = (p.x.round(), p.y.round());
        if x >= 0.0 && y >= 0.0 && (x as u16) < area.width && (y as u16) < area.height {
            buf[(area.x + x as u16, area.y + y as u16)].set_char(p.glyph).set_fg(p.color);
        }
    }
}

/// Bloques llenos con degradé rosa→lavanda que se desplaza, más un brillo que cruza el logo;
/// los trazos de "sombra" (╔═╝…) en rosa apagado.
fn draw_logo(buf: &mut Buffer, area: Rect, rows: &[Vec<char>], t: f32, reveal: f32) {
    let width = rows[0].len() as f32;
    let visible = (width * reveal) as usize;
    let shine = ((t * 0.35) % 1.6 - 0.3) * width; // posición del brillo
    for (y, row) in rows.iter().enumerate() {
        for (x, &ch) in row.iter().enumerate().take(visible) {
            if ch == ' ' || area.x + x as u16 >= buf.area.right() {
                continue;
            }
            let cell = &mut buf[(area.x + x as u16, area.y + y as u16)];
            let color = if ch == '█' {
                let wave = ((x as f32 / width) * std::f32::consts::TAU + t * 1.2 - y as f32 * 0.25).sin() * 0.5 + 0.5;
                let base = mix(PINK, LAVENDER, wave);
                let glow = (1.0 - ((x as f32 + y as f32 * 1.5 - shine).abs() / 4.0)).max(0.0);
                mix(base, Color::Rgb(255, 245, 250), glow * 0.85)
            } else {
                PINK_DIM
            };
            cell.set_char(ch).set_fg(color);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;

    /// Dibuja el menú en un backend de prueba y vuelca las celdas (carácter + colores) a JSON,
    /// para poder mirar el diseño sin terminal. Uso:
    /// MANGATERM_PREVIEW=<archivo.json> cargo test --release home_preview
    #[test]
    fn home_preview() {
        let Ok(out) = std::env::var("MANGATERM_PREVIEW") else { return };
        let mut terminal = Terminal::new(TestBackend::new(120, 34)).unwrap();
        let mut home = Home::new();
        home.selected = 1;
        // Dejar que termine la intro y caigan pétalos
        std::thread::sleep(std::time::Duration::from_millis(1300));
        let area = Rect::new(0, 0, 120, 34);
        for _ in 0..60 {
            home.tick(area);
        }
        let items = [
            MenuItem { icon: "♡", label: "Biblioteca", detail: "3 favoritos".into() },
            MenuItem { icon: "⌕", label: "Buscar", detail: "165 fuentes".into() },
            MenuItem { icon: "✦", label: "Extensiones", detail: "156 instaladas".into() },
            MenuItem { icon: "☁", label: "AniList", detail: "@alexp".into() },
            MenuItem { icon: "✕", label: "Salir", detail: String::new() },
        ];
        let footer = Line::from(vec![Span::raw("↑↓").fg(PINK).bold(), Span::raw(" elegir  ").fg(MUTED)]);
        terminal.draw(|f| home.draw(f, &items, footer)).unwrap();
        crate::theme::dump_buffer(terminal.backend().buffer(), &out);
    }
}
