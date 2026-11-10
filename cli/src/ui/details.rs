use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Paragraph, Wrap};

use doer_core::Todo;
use doer_core::todo::{Priority, parse_ymd, ymd_of};

use crate::ui::theme::Theme;

pub struct DetailsView<'a> {
    pub todo: Option<&'a Todo>,
    pub project: Option<&'a str>,
    pub today: i64,
}

pub fn render(frame: &mut Frame, area: Rect, view: &DetailsView<'_>, theme: &Theme) {
    let inner = Rect {
        x: area.x + 2,
        y: area.y + 1,
        width: area.width.saturating_sub(3),
        height: area.height.saturating_sub(2),
    };
    let lines = match view.todo {
        Some(todo) => lines_for(todo, view, theme),
        None => vec![Line::styled("Nothing selected", theme.dim)],
    };
    frame.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), inner);
}

fn lines_for(todo: &Todo, view: &DetailsView<'_>, theme: &Theme) -> Vec<Line<'static>> {
    let m = &todo.meta;
    let mut out = vec![Line::styled(
        todo.text.clone(),
        if todo.done {
            theme.done_text
        } else {
            theme.cursor_text
        },
    )];

    let mut facts: Vec<Span<'static>> = Vec::new();
    let mut fact = |span: Span<'static>| {
        if !facts.is_empty() {
            facts.push(Span::styled("  ·  ", theme.dim));
        }
        facts.push(span);
    };
    if let Some(project) = view.project {
        fact(Span::styled(format!("#{project}"), theme.meta));
    }
    if let Some(p) = m.priority() {
        let style = match p {
            Priority::Now => theme.urgent,
            Priority::Soon => theme.soon,
            Priority::Someday => theme.dim,
        };
        fact(Span::styled(p.as_str().to_string(), style));
    }
    if let Some(due) = &m.due {
        let (label, style) = due_label(due, view.today, theme);
        fact(Span::styled(label, style));
    }
    if !facts.is_empty() {
        out.push(Line::from(facts));
    }
    if let Some(remind) = &m.remind_at {
        out.push(Line::styled(format!("⏰ {remind}"), theme.meta));
    }

    if let Some(next) = &m.next_step {
        section(&mut out, "NEXT", theme);
        out.push(Line::styled(format!("👉 {next}"), theme.text));
    }
    if !m.links.is_empty() {
        section(&mut out, "LINKS", theme);
        for (i, link) in m.links.iter().enumerate() {
            let label = if link.label.is_empty() {
                short_url(&link.url)
            } else {
                link.label.clone()
            };
            out.push(Line::from(vec![
                Span::styled(format!("{} ", i + 1), theme.dim),
                Span::styled(label, theme.link),
            ]));
        }
    }
    if !m.context.is_empty() {
        section(&mut out, "CONTEXT", theme);
        for c in &m.context {
            out.push(Line::styled(format!("• {c}"), theme.text));
        }
    }
    if let Some(session) = &m.session {
        section(&mut out, "SESSION", theme);
        out.push(Line::styled(session.clone(), theme.meta));
    }
    if let Some(notes) = &m.notes {
        section(&mut out, "NOTES", theme);
        for line in notes.lines() {
            out.push(Line::styled(line.to_string(), theme.text));
        }
    }
    if m.is_empty() {
        out.push(Line::default());
        out.push(Line::styled(
            "No details. Agents add them with `doer update`.",
            theme.dim,
        ));
    }

    out.push(Line::default());
    out.push(Line::styled(hint(todo), theme.dim));
    out
}

fn hint(todo: &Todo) -> String {
    let mut keys = vec!["p priority"];
    if todo.meta.session.is_some() {
        keys.push("y copy resume");
    } else if todo.meta.next_step.is_some() {
        keys.push("y copy next");
    }
    if !todo.meta.links.is_empty() {
        keys.push("1-9 open link");
    }
    keys.push("o hide");
    keys.join(" · ")
}

fn section(out: &mut Vec<Line<'static>>, title: &str, theme: &Theme) {
    out.push(Line::default());
    out.push(Line::styled(title.to_string(), theme.heading));
}

fn short_url(url: &str) -> String {
    let trimmed = url
        .trim_start_matches("https://")
        .trim_start_matches("http://")
        .trim_start_matches("www.");
    if trimmed.chars().count() > 40 {
        let cut: String = trimmed.chars().take(39).collect();
        format!("{cut}…")
    } else {
        trimmed.to_string()
    }
}

#[must_use]
pub fn due_label(due: &str, today: i64, theme: &Theme) -> (String, Style) {
    let Some(day) = parse_ymd(due) else {
        return (format!("due {due}"), theme.meta);
    };
    let (_, m, d) = ymd_of(day);
    let date = format!("{d} {}", month(m));
    match day - today {
        n if n < 0 => (format!("overdue ({date})"), theme.urgent),
        0 => ("due today".to_string(), theme.soon),
        1 => (format!("due tomorrow ({date})"), theme.meta),
        _ => (format!("due {} {date}", weekday(day)), theme.meta),
    }
}

#[must_use]
pub fn mark_style(todo: &Todo, today: i64, theme: &Theme) -> Option<Style> {
    if todo.done {
        return None;
    }
    let due = todo.meta.due_day().map(|day| day - today);
    match (todo.meta.priority(), due) {
        (_, Some(n)) if n < 0 => Some(theme.urgent),
        (Some(Priority::Now), _) => Some(theme.urgent),
        (_, Some(0)) | (Some(Priority::Soon), _) => Some(theme.soon),
        (Some(Priority::Someday), _) => Some(theme.dim),
        _ => None,
    }
}

fn month(m: i64) -> &'static str {
    const NAMES: [&str; 12] = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];
    usize::try_from(m - 1)
        .ok()
        .and_then(|i| NAMES.get(i))
        .copied()
        .unwrap_or("?")
}

fn weekday(day: i64) -> &'static str {
    const NAMES: [&str; 7] = ["Thu", "Fri", "Sat", "Sun", "Mon", "Tue", "Wed"];
    usize::try_from(day.rem_euclid(7))
        .ok()
        .and_then(|i| NAMES.get(i))
        .copied()
        .unwrap_or("?")
}
