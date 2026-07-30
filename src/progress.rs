use std::collections::BTreeMap;
use std::io::{IsTerminal, Write, stdout};
use std::sync::Mutex;

const MAX_LABEL: usize = 24;

/// Live-updating table: one column per style, one row per concept.
/// Falls back to plain event lines when stdout is not a terminal.
pub struct Progress {
    tty: bool,
    inner: Mutex<Inner>,
}

struct Inner {
    styles: Vec<String>,
    concepts: Vec<String>,
    cells: BTreeMap<(usize, usize), Cell>,
    lines_drawn: usize,
    errors: Vec<String>,
}

#[derive(Default)]
struct Cell {
    total: usize,
    done: usize,
    failed: usize,
}

impl Progress {
    /// `styles`/`concepts` are display labels; every cell starts pending with
    /// `per_cell` expected generations.
    pub fn new(styles: Vec<String>, concepts: Vec<String>, per_cell: usize) -> Self {
        let mut cells = BTreeMap::new();
        for c in 0..concepts.len() {
            for s in 0..styles.len() {
                cells.insert(
                    (c, s),
                    Cell {
                        total: per_cell,
                        ..Cell::default()
                    },
                );
            }
        }
        let progress = Self {
            tty: stdout().is_terminal(),
            inner: Mutex::new(Inner {
                styles,
                concepts,
                cells,
                lines_drawn: 0,
                errors: Vec::new(),
            }),
        };
        progress.render();
        progress
    }

    /// Record one finished generation for (concept, style).
    pub fn record(&self, concept: usize, style: usize, result: &anyhow::Result<()>) {
        let mut inner = self.inner.lock().unwrap();
        let cell = inner.cells.get_mut(&(concept, style)).unwrap();
        match result {
            Ok(()) => cell.done += 1,
            Err(e) => {
                cell.failed += 1;
                let label = format!("{} x {}: {e:#}", inner.styles[style], inner.concepts[concept]);
                inner.errors.push(label);
            }
        }
        if !self.tty {
            let cell = &inner.cells[&(concept, style)];
            let status = if result.is_ok() { "ok" } else { "error" };
            println!(
                "[{status}] {} x {} ({}/{})",
                inner.styles[style],
                inner.concepts[concept],
                cell.done,
                cell.total
            );
            return;
        }
        drop(inner);
        self.render();
    }

    pub fn errors(&self) -> Vec<String> {
        self.inner.lock().unwrap().errors.clone()
    }

    fn render(&self) {
        if !self.tty {
            return;
        }
        let mut inner = self.inner.lock().unwrap();
        let label_width = inner
            .concepts
            .iter()
            .map(|c| truncate(c, MAX_LABEL).chars().count())
            .max()
            .unwrap_or(0);
        let col_width = inner
            .styles
            .iter()
            .map(|s| truncate(s, MAX_LABEL).chars().count())
            .max()
            .unwrap_or(0)
            .max(7);

        let mut lines = Vec::with_capacity(inner.concepts.len() + 1);
        let mut header = format!("{:width$}", "", width = label_width);
        for style in &inner.styles {
            header.push_str(&format!("  {:^width$}", truncate(style, MAX_LABEL), width = col_width));
        }
        lines.push(header);
        for c in 0..inner.concepts.len() {
            let mut row = format!(
                "{:<width$}",
                truncate(&inner.concepts[c], MAX_LABEL),
                width = label_width
            );
            for s in 0..inner.styles.len() {
                row.push_str(&format!(
                    "  {:^width$}",
                    cell_display(&inner.cells[&(c, s)]),
                    width = col_width
                ));
            }
            lines.push(row);
        }

        let mut out = String::new();
        if inner.lines_drawn > 0 {
            out.push_str(&format!("\x1b[{}F", inner.lines_drawn));
        }
        for line in &lines {
            out.push_str("\x1b[2K");
            out.push_str(line);
            out.push('\n');
        }
        inner.lines_drawn = lines.len();
        drop(inner);
        print!("{out}");
        let _ = stdout().flush();
    }
}

fn cell_display(cell: &Cell) -> String {
    let finished = cell.done + cell.failed;
    if finished == 0 {
        "…".into()
    } else if finished < cell.total {
        format!("{}/{}", cell.done, cell.total)
    } else if cell.failed == 0 {
        if cell.total == 1 {
            "✓".into()
        } else {
            format!("✓ {}", cell.done)
        }
    } else {
        format!("✗ {}/{}", cell.done, cell.total)
    }
}

fn truncate(s: &str, max: usize) -> String {
    let count = s.chars().count();
    if count <= max {
        s.to_string()
    } else {
        let mut truncated: String = s.chars().take(max - 1).collect();
        truncated.push('…');
        truncated
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cell(total: usize, done: usize, failed: usize) -> Cell {
        Cell { total, done, failed }
    }

    #[test]
    fn cell_states() {
        assert_eq!(cell_display(&cell(1, 0, 0)), "…");
        assert_eq!(cell_display(&cell(3, 1, 0)), "1/3");
        assert_eq!(cell_display(&cell(1, 1, 0)), "✓");
        assert_eq!(cell_display(&cell(3, 3, 0)), "✓ 3");
        assert_eq!(cell_display(&cell(3, 2, 1)), "✗ 2/3");
        assert_eq!(cell_display(&cell(1, 0, 1)), "✗ 0/1");
    }

    #[test]
    fn truncation() {
        assert_eq!(truncate("short", 10), "short");
        assert_eq!(truncate("averylongconceptname", 10), "averylong…");
    }
}
