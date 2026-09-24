//! Draws the dashboard. Everything stays readable without colour (`NO_COLOR`): states
//! have their own symbols, and the selection is reversed, not coloured.

use ratatui::{
    Frame,
    layout::{Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Clear, Paragraph, Row, Sparkline, Table, TableState},
};

use super::model::{Dashboard, Health, Mode, Origin, Pane};

/// Below this, only the focused pane is shown.
const COMPACT_WIDTH: u16 = 60;
const COMPACT_HEIGHT: u16 = 16;

fn style(d: &Dashboard, color: Color) -> Style {
    if d.color {
        Style::new().fg(color)
    } else {
        Style::new()
    }
}

fn dim(d: &Dashboard) -> Style {
    if d.color {
        Style::new().fg(Color::DarkGray)
    } else {
        Style::new()
    }
}

fn selected() -> Style {
    Style::new().add_modifier(Modifier::REVERSED)
}

/// `12s`, `5m`, `2h`, `3d`.
pub(crate) fn age(from_ms: u64, now_ms: u64) -> String {
    let seconds = now_ms.saturating_sub(from_ms) / 1000;
    match seconds {
        0..60 => format!("{seconds}s"),
        60..3600 => format!("{}m", seconds / 60),
        3600..86_400 => format!("{}h", seconds / 3600),
        _ => format!("{}d", seconds / 86_400),
    }
}

fn dot(d: &Dashboard, health: Health) -> Span<'static> {
    let (symbol, color) = match health {
        Health::Up => ("●", Color::Green),
        Health::Pending => ("◐", Color::Yellow),
        Health::Down => ("✕", Color::Red),
        Health::Unknown => ("○", Color::DarkGray),
    };
    Span::styled(symbol, style(d, color))
}

fn percent(value: Option<f64>) -> String {
    match value {
        None => "–".into(),
        Some(v) if v >= 0.9995 => "100%".into(),
        Some(v) => format!("{:.1}%", v * 100.0),
    }
}

fn rate(value: Option<f64>) -> String {
    match value {
        None => "–".into(),
        Some(v) if v >= 10.0 => format!("{v:.0}/s"),
        Some(v) => format!("{v:.1}/s"),
    }
}

fn block<'a>(d: &Dashboard, pane: Pane, title: &str) -> Block<'a> {
    let focused = d.focus == pane;
    let title = if focused {
        Span::styled(
            format!(" {title} "),
            Style::new().add_modifier(Modifier::BOLD),
        )
    } else {
        Span::raw(format!(" {title} "))
    };
    let border = if focused {
        style(d, Color::Cyan)
    } else {
        dim(d)
    };
    Block::bordered().title(title).border_style(border)
}

fn state(d: &Dashboard, pane: Pane) -> TableState {
    let mut state = TableState::default();
    if d.focus == pane {
        state.select(Some(d.selection(pane)));
    }
    state
}

fn shares(frame: &mut Frame<'_>, d: &Dashboard, area: Rect) {
    let visible = d.visible_shares();
    let rows: Vec<Row<'_>> = visible
        .iter()
        .map(|s| {
            let url = s.url.clone().unwrap_or_else(|| format!("({})", s.status));
            Row::new(vec![
                url,
                s.origin.clone(),
                s.by.to_owned(),
                age(s.started_at, d.now_ms),
                rate(s.rate),
            ])
        })
        .collect();
    let empty = rows.is_empty();
    let table = Table::new(
        rows,
        [
            Constraint::Fill(3),
            Constraint::Fill(2),
            Constraint::Length(8),
            Constraint::Length(5),
            Constraint::Length(7),
        ],
    )
    .header(Row::new(["URL", "ORIGIN", "BY", "AGE", "REQ/S"]).style(dim(d)))
    .row_highlight_style(selected())
    .block(block(
        d,
        Pane::Shares,
        &format!("Shares ({})", visible.len()),
    ));
    frame.render_stateful_widget(table, area, &mut state(d, Pane::Shares));
    if empty {
        hint(frame, area, "No shares. Press s to share a port.");
    }
}

fn routes(frame: &mut Frame<'_>, d: &Dashboard, area: Rect) {
    let visible = d.visible_routes();
    let rows: Vec<Row<'_>> = visible
        .iter()
        .map(|r| {
            Row::new(vec![
                Line::from(vec![
                    dot(d, r.health),
                    Span::raw(format!(" {}", r.hostname)),
                ]),
                Line::from(format!("→ {}", r.origin)),
                Line::from(percent(r.uptime)),
            ])
        })
        .collect();
    let empty = rows.is_empty();
    let table = Table::new(
        rows,
        [
            Constraint::Fill(3),
            Constraint::Fill(2),
            Constraint::Length(8),
        ],
    )
    .header(Row::new(["ROUTE", "ORIGIN", "UP 24H"]).style(dim(d)))
    .row_highlight_style(selected())
    .block(block(
        d,
        Pane::Routes,
        &format!("Routes ({})", visible.len()),
    ));
    frame.render_stateful_widget(table, area, &mut state(d, Pane::Routes));
    if empty {
        hint(frame, area, "No routes on this machine.");
    }
}

fn requests(frame: &mut Frame<'_>, d: &Dashboard, area: Rect) {
    let visible = d.visible_requests();
    let rows: Vec<Row<'_>> = visible
        .iter()
        .map(|r| {
            let status = r.status.map_or_else(|| "…".to_owned(), |s| s.to_string());
            let color = match r.status {
                Some(500..) => Color::Red,
                Some(400..500) => Color::Yellow,
                _ => Color::Reset,
            };
            Row::new(vec![
                Line::from(r.method.clone()),
                Line::from(r.path.clone()),
                Line::styled(status, style(d, color)),
                Line::from(
                    r.duration_ms
                        .map_or_else(String::new, |ms| format!("{ms} ms")),
                ),
            ])
        })
        .collect();
    let table = Table::new(
        rows,
        [
            Constraint::Length(7),
            Constraint::Fill(1),
            Constraint::Length(4),
            Constraint::Length(8),
        ],
    )
    .row_highlight_style(selected())
    .block(block(d, Pane::Requests, "Requests"));
    frame.render_stateful_widget(table, area, &mut state(d, Pane::Requests));
}

fn traffic(frame: &mut Frame<'_>, d: &Dashboard, area: Rect) {
    let title = if d.traffic_per_minute {
        "Traffic (requests per minute, last hour)"
    } else {
        "Traffic (share requests per refresh)"
    };
    let data: Vec<u64> = d.traffic.iter().copied().collect();
    let peak = data.iter().copied().max().unwrap_or(0);
    let sparkline = Sparkline::default()
        .data(data)
        .style(style(d, Color::Cyan))
        .block(
            Block::bordered()
                .title(format!(" {title} · peak {peak} "))
                .border_style(dim(d)),
        );
    frame.render_widget(sparkline, area);
}

/// A line inside an empty pane.
fn hint(frame: &mut Frame<'_>, area: Rect, text: &str) {
    if area.height < 4 || area.width < 4 {
        return;
    }
    let inner = Rect {
        x: area.x + 2,
        y: area.y + 2,
        width: area.width - 4,
        height: 1,
    };
    frame.render_widget(Paragraph::new(text.to_owned()), inner);
}

fn header(d: &Dashboard) -> Line<'static> {
    let from = match &d.origin {
        Origin::App { version } => format!("the Teitunnel app {version}"),
        Origin::Local => "this machine's records (the app isn't running)".to_owned(),
    };
    let mut spans = vec![
        Span::styled("teitunnel top", Style::new().add_modifier(Modifier::BOLD)),
        Span::raw(format!(" · from {from}")),
    ];
    if !d.filter.is_empty() {
        spans.push(Span::raw(format!(" · filter: {}", d.filter)));
    }
    Line::from(spans)
}

fn footer(d: &Dashboard) -> Line<'static> {
    match d.mode {
        Mode::Filter => Line::from(format!(
            "Filter: {}▏ (Enter to keep, Esc to clear)",
            d.input
        )),
        Mode::Share => Line::from(format!(
            "Share a port or URL: {}▏ (Enter to share, Esc to cancel)",
            d.input
        )),
        _ => match &d.message {
            Some(message) => Line::from(message.clone()),
            None => Line::styled(
                "q quit · tab switch · ↑↓ select · s share · x stop · c copy URL · / filter · ? keys",
                dim(d),
            ),
        },
    }
}

fn help(frame: &mut Frame<'_>, area: Rect) {
    let lines = [
        "q, Esc      quit",
        "Tab         next pane",
        "↑ ↓, j k    select",
        "s           share a port",
        "x x         stop the selected share",
        "c           copy the selected URL",
        "/           filter",
        "?           these keys",
    ];
    let width = 42.min(area.width);
    let height = u16::try_from(lines.len() + 2)
        .unwrap_or(10)
        .min(area.height);
    let rect = Rect {
        x: area.x + (area.width - width) / 2,
        y: area.y + (area.height - height) / 2,
        width,
        height,
    };
    frame.render_widget(Clear, rect);
    frame.render_widget(
        Paragraph::new(lines.map(Line::from).to_vec()).block(Block::bordered().title(" Keys ")),
        rect,
    );
}

/// Draws the whole screen.
pub(crate) fn draw(frame: &mut Frame<'_>, d: &Dashboard) {
    let area = frame.area();
    let [top, body, bottom] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Min(0),
        Constraint::Length(1),
    ])
    .areas(area);
    frame.render_widget(Paragraph::new(header(d)), top);
    frame.render_widget(Paragraph::new(footer(d)), bottom);
    if area.width < COMPACT_WIDTH || area.height < COMPACT_HEIGHT {
        match d.focus {
            Pane::Shares => shares(frame, d, body),
            Pane::Routes => routes(frame, d, body),
            Pane::Requests => requests(frame, d, body),
        }
    } else {
        let [share_area, route_area, lower] = Layout::vertical([
            Constraint::Fill(1),
            Constraint::Fill(1),
            Constraint::Length(if d.requests.is_some() { 10 } else { 5 }),
        ])
        .areas(body);
        shares(frame, d, share_area);
        routes(frame, d, route_area);
        if d.requests.is_some() {
            let [traffic_area, request_area] =
                Layout::horizontal([Constraint::Fill(1), Constraint::Fill(1)]).areas(lower);
            traffic(frame, d, traffic_area);
            requests(frame, d, request_area);
        } else {
            traffic(frame, d, lower);
        }
    }
    if d.mode == Mode::Help {
        help(frame, body);
    }
}

#[cfg(test)]
mod tests {
    use ratatui::{Terminal, backend::TestBackend, buffer::Buffer};

    use super::*;
    use crate::top::model::{
        Key, RequestRow, Snapshot,
        tests::{route, share},
    };

    fn text(buffer: &Buffer) -> Vec<String> {
        (0..buffer.area.height)
            .map(|y| {
                (0..buffer.area.width)
                    .map(|x| buffer[(x, y)].symbol())
                    .collect::<String>()
            })
            .collect()
    }

    fn render(d: &Dashboard, width: u16, height: u16) -> Terminal<TestBackend> {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        terminal.draw(|frame| draw(frame, d)).unwrap();
        terminal
    }

    fn sample(color: bool) -> Dashboard {
        let mut d = Dashboard::new(
            Origin::App {
                version: "0.2.0".into(),
            },
            color,
        );
        let first = Snapshot {
            shares: vec![share("quiet-river", 10)],
            routes: Some(vec![
                route("app.example.com", Health::Up),
                route("api.example.com", Health::Down),
            ]),
            ..Snapshot::default()
        };
        d.update(first, 60_000, 1.0);
        d.update(
            Snapshot {
                shares: vec![share("quiet-river", 40)],
                ..Snapshot::default()
            },
            125_000,
            2.0,
        );
        d
    }

    #[test]
    fn draws_every_pane() {
        let d = sample(true);
        let terminal = render(&d, 100, 30);
        let lines = text(terminal.backend().buffer());
        assert!(lines[0].starts_with("teitunnel top · from the Teitunnel app 0.2.0"));
        let screen = lines.join("\n");
        assert!(screen.contains(" Shares (1) "));
        assert!(screen.contains("https://quiet-river.trycloudflare.com"));
        assert!(screen.contains("15/s"), "{screen}");
        assert!(screen.contains("2m"), "age");
        assert!(screen.contains("● app.example.com"));
        assert!(screen.contains("✕ api.example.com"));
        assert!(screen.contains("99.8%"));
        assert!(screen.contains("Traffic (share requests per refresh) · peak 30"));
        assert!(!screen.contains("Requests"), "no inspector, no pane");
        assert!(lines[29].starts_with("q quit · tab switch"));
        // The live dot is green, the selected share reversed.
        let buffer = terminal.backend().buffer();
        let dot = lines
            .iter()
            .enumerate()
            .find_map(|(y, l)| l.find("● app").map(|x| (l[..x].chars().count(), y)))
            .unwrap();
        let cell = &buffer[(u16::try_from(dot.0).unwrap(), u16::try_from(dot.1).unwrap())];
        assert_eq!(cell.fg, Color::Green);
    }

    #[test]
    fn respects_no_color() {
        let d = sample(false);
        let terminal = render(&d, 100, 30);
        let buffer = terminal.backend().buffer();
        assert!(
            buffer.content.iter().all(|cell| cell.fg == Color::Reset),
            "no colours"
        );
        assert!(
            text(buffer).join("\n").contains("✕ api.example.com"),
            "symbols stay"
        );
    }

    #[test]
    fn fits_small_terminals() {
        let mut d = sample(true);
        let terminal = render(&d, 40, 10);
        let screen = text(terminal.backend().buffer()).join("\n");
        assert!(screen.contains("Shares (1)"));
        assert!(!screen.contains("Routes"), "only the focused pane");
        d.key(Key::Tab);
        let terminal = render(&d, 40, 10);
        assert!(
            text(terminal.backend().buffer())
                .join("\n")
                .contains("Routes (2)")
        );
        // Tiny, and it still draws.
        render(&d, 10, 3);
    }

    #[test]
    fn shows_prompts_help_and_requests() {
        let mut d = sample(true);
        d.key(Key::Char('s'));
        d.key(Key::Char('3'));
        let terminal = render(&d, 100, 30);
        let lines = text(terminal.backend().buffer());
        assert!(lines[29].starts_with("Share a port or URL: 3"));
        d.key(Key::Esc);
        d.key(Key::Char('?'));
        let screen = text(render(&d, 100, 30).backend().buffer()).join("\n");
        assert!(screen.contains("x x         stop the selected share"));
        d.key(Key::Esc);
        d.update(
            Snapshot {
                shares: d.shares.clone(),
                requests: vec![RequestRow {
                    share: "quiet-river".into(),
                    method: "POST".into(),
                    path: "/api/login".into(),
                    status: Some(502),
                    duration_ms: Some(31),
                }],
                ..Snapshot::default()
            },
            126_000,
            1.0,
        );
        let screen = text(render(&d, 100, 30).backend().buffer()).join("\n");
        assert!(screen.contains(" Requests "));
        assert!(screen.contains("POST") && screen.contains("/api/login") && screen.contains("502"));
    }

    #[test]
    fn formats_ages() {
        assert_eq!(age(0, 59_000), "59s");
        assert_eq!(age(0, 61_000), "1m");
        assert_eq!(age(0, 7_200_000), "2h");
        assert_eq!(age(0, 200_000_000), "2d");
        assert_eq!(age(5_000, 0), "0s");
    }
}
