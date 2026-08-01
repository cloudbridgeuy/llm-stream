//! Incremental rendering of a streamed markdown answer.
//!
//! The renderer this replaces re-highlighted the whole accumulated answer on
//! every chunk and then printed `output.lines().skip(previous_line_count - 1)`
//! after a `MoveToColumn(0)`. It rested on two assumptions, and neither holds.
//!
//! The first is that re-rendering only ever appends lines. It does not:
//! `printer::wrap_text_to_terminal_width` re-wraps, so the last word on a row
//! moves down to the next row the moment it grows past the margin, and every
//! row after it shifts. The text already on screen is then wrong, and nothing
//! goes back to correct it.
//!
//! The second is that a rendered line occupies one physical row, which makes
//! `MoveToColumn(0)` the start of it. A line long enough to wrap occupies
//! several, so the reprint lands on the last of them and the rows above keep
//! whatever they held. Worse, when a render came back *shorter* than the last
//! one — which a line dense in `|` and `+---+` does, once the highlighter reads
//! it as a table and then stops — `skip(length - 1)` skipped past the end and
//! printed nothing at all. An answer could stream and cache in full while
//! leaving no trace on screen.
//!
//! This renderer never rewrites a row it has printed. It holds the current
//! logical line, re-renders only that line as text arrives, and treats a row as
//! finished the moment content exists beyond its break point, which greedy
//! wrapping makes permanent. Finished rows are printed and terminated; the row
//! still being written is repainted in place, and being at most one terminal
//! width it always occupies exactly one physical row, so repainting it is a
//! carriage return and a clear.
//!
//! The cost of never rewriting is that a row's colours freeze when it is
//! printed: an emphasis span that closes on the next row leaves the first half
//! unemphasised. Any streaming renderer pays this, and it is cheap next to
//! losing the text.

use std::io::Write;

use crossterm::{
    cursor::MoveToColumn,
    terminal::{Clear, ClearType},
};
use syntect::{easy::HighlightLines, highlighting::Style, parsing::SyntaxReference};
use unicode_width::UnicodeWidthChar;

use crate::printer::{as_24_bit_terminal_escaped, MARKDOWN_SYNTAX, SYNTAX_SET, THEME};

/// Ends every painted row, so that a style left open by the highlighter cannot
/// bleed into the shell prompt or the next row.
const RESET: &str = "\x1b[0m";

/// Which syntax the lines arriving now are highlighted with.
///
/// A fenced block is a state, not a property of a line, and the line that opens
/// one is the only evidence of it. Tracking the state here is what lets the
/// renderer highlight a line the moment it arrives instead of re-parsing the
/// answer from the top.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Mode {
    /// Outside a fenced block. Lines are highlighted as Markdown and wrapped at
    /// word boundaries.
    Prose,
    /// Inside a fenced block opened with this language token. Lines are
    /// highlighted as that language and broken at the margin rather than at a
    /// word, since a break inside code should fall where the terminal would
    /// have put it anyway.
    Fence { language: String },
}

/// What one chunk of arriving text adds to the screen.
///
/// `finished` rows are permanent: each is printed and terminated, and no later
/// frame touches it. `partial` is the row still being written, repainted whole
/// on every frame.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Frame {
    pub finished: Vec<String>,
    pub partial: String,
}

/// Pure. The width of `s` in terminal columns.
///
/// The renderer measures raw text and slices the styled spans afterwards, so
/// this never sees an escape sequence and does not have to skip one.
#[must_use]
pub fn display_width(s: &str) -> usize {
    s.chars().map(|c| UnicodeWidthChar::width(c).unwrap_or(0)).sum()
}

/// Pure. The byte offset in `s` of the first character that would carry the row
/// past `width` columns.
///
/// At least one character is always taken, so a character wider than the whole
/// row still makes progress rather than wrapping forever.
fn hard_cut(s: &str, width: usize) -> usize {
    let mut used = 0;
    let mut cut = 0;
    for (i, c) in s.char_indices() {
        let w = UnicodeWidthChar::width(c).unwrap_or(0);
        if used + w > width && i > 0 {
            return i;
        }
        used += w;
        cut = i + c.len_utf8();
    }
    cut
}

/// Pure. The interior byte offsets at which `line` breaks into rows of at most
/// `width` columns. The offsets exclude 0 and `line.len()`, so a line that fits
/// yields none.
///
/// With `word_wrap` the break falls after the last space that fits, and with it
/// off the break falls at the margin. A word wider than the whole row is broken
/// at the margin either way.
///
/// The offsets are monotonic under growth, which is the property the renderer
/// is built on: a break point is produced only once text exists beyond it, and
/// greedy filling decides it from the text before it, so appending can never
/// move one. That is what makes a finished row safe to print and forget.
#[must_use]
pub fn wrap_points(line: &str, width: usize, word_wrap: bool) -> Vec<usize> {
    let width = width.max(1);
    let mut points = Vec::new();
    let mut start = 0;

    while display_width(&line[start..]) > width {
        let hard = start + hard_cut(&line[start..], width);
        let mut cut = hard;
        if word_wrap {
            if let Some(i) = line[start..hard].rfind(' ') {
                cut = start + i + 1;
            }
        }
        if cut <= start {
            cut = hard;
        }
        if cut <= start {
            break;
        }
        points.push(cut);
        start = cut;
    }

    points
}

/// Pure. Cuts `spans` at `points`, byte offsets into the text the spans cover,
/// and returns one row of spans per segment.
///
/// A point falling inside a span splits it and keeps the style on both halves,
/// which is what keeps the colouring aligned with the text: the wrap is decided
/// on the raw line and applied to the highlighted one, so the two can never
/// disagree about where a row ends.
#[must_use]
pub fn slice_spans<'a>(
    spans: &[(Style, &'a str)],
    points: &[usize],
) -> Vec<Vec<(Style, &'a str)>> {
    let mut rows = Vec::new();
    let mut current: Vec<(Style, &'a str)> = Vec::new();
    let mut offset = 0;
    let mut next = 0;

    for &(style, text) in spans {
        let mut rest = text;
        while !rest.is_empty() {
            while points.get(next).is_some_and(|&p| p == offset) {
                next += 1;
                rows.push(std::mem::take(&mut current));
            }
            let end = offset + rest.len();
            match points.get(next).copied().filter(|&p| p < end) {
                Some(p) => {
                    let (head, tail) = rest.split_at(p - offset);
                    current.push((style, head));
                    offset = p;
                    rest = tail;
                }
                None => {
                    current.push((style, rest));
                    offset = end;
                    rest = "";
                }
            }
        }
    }

    while points.get(next).is_some_and(|&p| p == offset) {
        next += 1;
        rows.push(std::mem::take(&mut current));
    }
    rows.push(current);
    rows
}

/// Pure. The mode `line` leaves behind, and whether it is a fence delimiter.
///
/// A delimiter is not printed, which matches `printer::highlight_markdown`:
/// that parser consumes the fences and emits only the code between them, so
/// printing them while streaming would make the live answer differ from the one
/// `--show` prints back.
#[must_use]
pub fn step_mode(mode: &Mode, line: &str) -> (Mode, bool) {
    let trimmed = line.trim_start();
    match mode {
        Mode::Prose if trimmed.starts_with("```") => {
            let language = trimmed.trim_start_matches('`').trim();
            let language = if language.is_empty() {
                "txt".to_string()
            } else {
                language.to_string()
            };
            (Mode::Fence { language }, true)
        }
        Mode::Fence { .. } if trimmed.starts_with("```") => (Mode::Prose, true),
        other => (other.clone(), false),
    }
}

/// Pure. Whether a line, as far as it has arrived, is or may yet become a fence
/// delimiter.
///
/// Such a line is held off the screen until it ends. A delimiter is never
/// printed, and a line that has produced only a backtick or two could still
/// turn into one, so showing it and taking it back would flicker on every
/// fenced block. Nothing is lost by waiting: a delimiter is one short line.
#[must_use]
pub fn holds_back(line: &str) -> bool {
    let trimmed = line.trim_start();
    trimmed.starts_with("```") || (trimmed.len() < 3 && "```".starts_with(trimmed))
}

/// The renderer's state: everything it needs to turn the next chunk into a
/// frame, and nothing else.
#[derive(Debug, Clone)]
pub struct StreamRenderer {
    width: usize,
    mode: Mode,
    /// The raw text of the logical line being written, without its newline.
    line: String,
    /// How many of that line's rows have already been reported finished.
    emitted: usize,
}

impl StreamRenderer {
    #[must_use]
    pub fn new(width: usize) -> Self {
        Self {
            width: width.max(1),
            mode: Mode::Prose,
            line: String::new(),
            emitted: 0,
        }
    }

    /// Pure. Renders the current line into rows, highlighted and escaped.
    fn rows(&self) -> Vec<String> {
        let syntax: &SyntaxReference = match &self.mode {
            Mode::Prose => *MARKDOWN_SYNTAX,
            Mode::Fence { language } => SYNTAX_SET
                .find_syntax_by_token(language)
                .unwrap_or_else(|| SYNTAX_SET.find_syntax_plain_text()),
        };

        let mut highlighter = HighlightLines::new(syntax, &THEME);
        let spans = highlighter
            .highlight_line(&self.line, &SYNTAX_SET)
            .unwrap_or_else(|_| vec![(Style::default(), self.line.as_str())]);

        let points = wrap_points(&self.line, self.width, self.mode == Mode::Prose);

        slice_spans(&spans, &points)
            .iter()
            .map(|row| as_24_bit_terminal_escaped(row, false))
            .collect()
    }

    /// Pure. Retires the current line, whose newline has arrived.
    fn end_line(&mut self, frame: &mut Frame) {
        let (next, delimiter) = step_mode(&self.mode, &self.line);
        if !delimiter {
            frame.finished.extend(self.rows().into_iter().skip(self.emitted));
        }
        self.mode = next;
        self.line.clear();
        self.emitted = 0;
    }

    /// Pure. Reports the rows of the unfinished line that content has now moved
    /// past, and leaves the rest as the row to repaint.
    fn open_line(&mut self, frame: &mut Frame) {
        if holds_back(&self.line) {
            frame.partial.clear();
            return;
        }

        let rows = self.rows();
        let settled = rows.len().saturating_sub(1);
        frame
            .finished
            .extend(rows.iter().take(settled).skip(self.emitted).cloned());
        self.emitted = settled;
        frame.partial = rows.last().cloned().unwrap_or_default();
    }

    /// Pure. Folds a chunk of arriving text into the state and returns what it
    /// adds to the screen.
    pub fn push(&mut self, text: &str) -> Frame {
        let mut frame = Frame::default();
        for (i, segment) in text.split('\n').enumerate() {
            if i > 0 {
                self.end_line(&mut frame);
            }
            self.line.push_str(segment);
        }
        self.open_line(&mut frame);
        frame
    }

    /// Pure. Closes the answer, retiring a last line that arrived without a
    /// newline. The cursor is left on a fresh row, which is why no trailing
    /// newline is needed and why the shell prompt no longer lands on a `%`.
    pub fn finish(&mut self) -> Frame {
        let mut frame = Frame::default();
        if !self.line.is_empty() {
            self.end_line(&mut frame);
        }
        frame
    }
}

/// Shell. The terminal's width in columns, or 80 when it cannot be read.
#[must_use]
pub fn terminal_width() -> usize {
    crossterm::terminal::size().map_or(80, |(w, _)| usize::from(w).max(1))
}

/// Shell. Clears the cursor's row and returns to its start. Used to wipe the
/// spinner, which the old code overwrote with a fixed run of spaces wide enough
/// for the spinner it happened to be using.
pub fn clear_row() -> std::io::Result<()> {
    let mut out = std::io::stdout();
    crossterm::queue!(out, MoveToColumn(0), Clear(ClearType::UntilNewLine))?;
    out.flush()
}

/// Shell. Paints a frame, starting on the row the last one left the cursor on.
///
/// Every row is written from column 0 over a cleared line, so a shorter row
/// cannot leave the tail of a longer one behind it.
pub fn print_frame(frame: &Frame) -> std::io::Result<()> {
    let mut out = std::io::stdout();

    for row in &frame.finished {
        crossterm::queue!(out, MoveToColumn(0), Clear(ClearType::UntilNewLine))?;
        writeln!(out, "{row}{RESET}")?;
    }

    crossterm::queue!(out, MoveToColumn(0), Clear(ClearType::UntilNewLine))?;
    write!(out, "{}{RESET}", frame.partial)?;
    out.flush()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Strips the escapes a row is painted with, leaving the text a terminal
    /// would show. Every assertion below is about text and where it breaks,
    /// which is what the old renderer got wrong.
    fn plain(row: &str) -> String {
        let mut out = String::new();
        let mut chars = row.chars();
        while let Some(c) = chars.next() {
            if c == '\x1b' {
                for c in chars.by_ref() {
                    if c == 'm' {
                        break;
                    }
                }
            } else {
                out.push(c);
            }
        }
        out
    }

    fn rows_of(frame: &Frame) -> Vec<String> {
        frame.finished.iter().map(|r| plain(r)).collect()
    }

    // ── display_width ────────────────────────────────────────────────────────

    #[test]
    fn width_counts_columns_not_bytes() {
        // The old wrapper used `str::len`, so an accented word claimed more
        // columns than it occupies and wrapped early.
        assert_eq!(display_width("Código"), 6);
        assert_eq!("Código".len(), 7);
    }

    #[test]
    fn width_counts_an_arrow_as_one_column() {
        assert_eq!(display_width("a → b"), 5);
    }

    #[test]
    fn width_counts_a_wide_character_as_two() {
        assert_eq!(display_width("日本"), 4);
    }

    // ── wrap_points ──────────────────────────────────────────────────────────

    #[test]
    fn a_line_that_fits_has_no_break() {
        assert_eq!(wrap_points("hello", 10, true), Vec::<usize>::new());
    }

    #[test]
    fn a_line_breaks_after_the_last_space_that_fits() {
        // "aaa bbb", width 5: "aaa " fits, "aaa bbb" does not.
        assert_eq!(wrap_points("aaa bbb", 5, true), vec![4]);
    }

    #[test]
    fn a_word_wider_than_the_row_breaks_at_the_margin() {
        assert_eq!(wrap_points("aaaaaaaa", 3, true), vec![3, 6]);
    }

    #[test]
    fn code_breaks_at_the_margin_rather_than_at_a_space() {
        assert_eq!(wrap_points("aaa bbb", 5, false), vec![5]);
    }

    #[test]
    fn rows_reassemble_into_the_line() {
        // Nothing may be dropped at a break, which the old wrapper could not
        // promise: it rebuilt lines from `split_whitespace`, so a run of spaces
        // or an indent came back as one space.
        let line = "  indented   text with   runs of spaces and a verylongunbreakableword here";
        let points = wrap_points(line, 12, true);
        let mut joined = String::new();
        let mut start = 0;
        for p in &points {
            joined.push_str(&line[start..*p]);
            start = *p;
        }
        joined.push_str(&line[start..]);
        assert_eq!(joined, line);
    }

    #[test]
    fn no_row_is_wider_than_the_margin() {
        let line = "The quick brown fox jumps over the lazy dog and keeps on running";
        let points = wrap_points(line, 12, true);
        let mut start = 0;
        for p in points.iter().chain(std::iter::once(&line.len())) {
            assert!(
                display_width(&line[start..*p]) <= 12,
                "row {:?} is too wide",
                &line[start..*p]
            );
            start = *p;
        }
    }

    #[test]
    fn breaks_never_move_as_the_line_grows() {
        // The property the renderer rests on. The old one had no such
        // guarantee, which is exactly how rows already on screen went stale.
        let full = "one, two, three, four, five, six, seven, eight, nine, ten, eleven";
        let mut previous: Vec<usize> = Vec::new();
        for end in 1..=full.len() {
            if !full.is_char_boundary(end) {
                continue;
            }
            let points = wrap_points(&full[..end], 16, true);
            assert!(
                points.starts_with(&previous),
                "breaks moved at {end}: {previous:?} then {points:?}"
            );
            previous = points;
        }
    }

    #[test]
    fn a_zero_width_margin_still_makes_progress() {
        assert_eq!(wrap_points("abc", 0, true), vec![1, 2]);
    }

    // ── slice_spans ──────────────────────────────────────────────────────────

    #[test]
    fn spans_split_at_a_point_inside_one() {
        let style = Style::default();
        let rows = slice_spans(&[(style, "abcdef")], &[2, 4]);
        let text: Vec<Vec<&str>> = rows
            .iter()
            .map(|r| r.iter().map(|(_, t)| *t).collect())
            .collect();
        assert_eq!(text, vec![vec!["ab"], vec!["cd"], vec!["ef"]]);
    }

    #[test]
    fn a_point_on_a_span_boundary_starts_a_row() {
        let style = Style::default();
        let rows = slice_spans(&[(style, "abc"), (style, "def")], &[3]);
        let text: Vec<Vec<&str>> = rows
            .iter()
            .map(|r| r.iter().map(|(_, t)| *t).collect())
            .collect();
        assert_eq!(text, vec![vec!["abc"], vec!["def"]]);
    }

    #[test]
    fn no_points_yields_one_row() {
        let style = Style::default();
        let rows = slice_spans(&[(style, "abc")], &[]);
        assert_eq!(rows.len(), 1);
    }

    // ── step_mode and holds_back ─────────────────────────────────────────────

    #[test]
    fn a_fence_opens_with_its_language() {
        let (mode, delimiter) = step_mode(&Mode::Prose, "```rust");
        assert_eq!(
            mode,
            Mode::Fence {
                language: "rust".to_string()
            }
        );
        assert!(delimiter);
    }

    #[test]
    fn a_bare_fence_opens_as_plain_text() {
        let (mode, _) = step_mode(&Mode::Prose, "```");
        assert_eq!(
            mode,
            Mode::Fence {
                language: "txt".to_string()
            }
        );
    }

    #[test]
    fn a_fence_closes_back_to_prose() {
        let inside = Mode::Fence {
            language: "rust".to_string(),
        };
        assert_eq!(step_mode(&inside, "```"), (Mode::Prose, true));
    }

    #[test]
    fn an_ordinary_line_leaves_the_mode_alone() {
        let (mode, delimiter) = step_mode(&Mode::Prose, "let x = 1;");
        assert_eq!(mode, Mode::Prose);
        assert!(!delimiter);
    }

    #[test]
    fn a_partial_fence_is_held_back() {
        assert!(holds_back("`"));
        assert!(holds_back("``"));
        assert!(holds_back("```js"));
    }

    #[test]
    fn inline_code_is_not_held_back() {
        assert!(!holds_back("use `foo` here"));
        assert!(!holds_back("``x"));
    }

    // ── StreamRenderer ───────────────────────────────────────────────────────

    #[test]
    fn a_short_line_stays_partial_until_its_newline() {
        let mut r = StreamRenderer::new(40);
        let frame = r.push("hello");
        assert!(frame.finished.is_empty());
        assert_eq!(plain(&frame.partial), "hello");

        let frame = r.push("\n");
        assert_eq!(rows_of(&frame), vec!["hello"]);
        assert_eq!(frame.partial, "");
    }

    #[test]
    fn a_row_is_finished_once_content_passes_its_break() {
        let mut r = StreamRenderer::new(10);
        let frame = r.push("aaa bbb ccc ddd");
        assert_eq!(rows_of(&frame), vec!["aaa bbb "]);
        assert_eq!(plain(&frame.partial), "ccc ddd");
    }

    #[test]
    fn a_finished_row_is_never_reported_twice() {
        // The heart of the fix. Whatever order the text arrives in, each row is
        // handed to the terminal exactly once, so printing it and forgetting it
        // is safe.
        let full = "one, two, three, four, five, six, seven, eight, nine, ten, eleven, twelve";
        let mut r = StreamRenderer::new(20);
        let mut printed: Vec<String> = Vec::new();
        for chunk in full.as_bytes().chunks(3) {
            let text = String::from_utf8_lossy(chunk).to_string();
            printed.extend(rows_of(&r.push(&text)));
        }
        printed.extend(rows_of(&r.finish()));
        assert_eq!(printed.concat(), full);
    }

    #[test]
    fn the_answer_that_vanished_survives_one_character_at_a_time() {
        // The shape that produced the bug report: one long line, dense in the
        // table characters that made the old highlighter's output shrink.
        let full = concat!(
            "printf '%s\\n' '+--------+-------+------+' ",
            "'| Código | Build | Test |' '+--------+-------+------+'"
        );
        let mut r = StreamRenderer::new(30);
        let mut printed: Vec<String> = Vec::new();
        for c in full.chars() {
            printed.extend(rows_of(&r.push(&c.to_string())));
        }
        printed.extend(rows_of(&r.finish()));
        assert_eq!(printed.concat(), full);
        assert!(printed.len() > 1, "a line this long must occupy rows");
        for row in &printed {
            assert!(display_width(row) <= 30, "row {row:?} is too wide");
        }
    }

    #[test]
    fn a_fenced_block_switches_syntax_and_drops_its_delimiters() {
        let mut r = StreamRenderer::new(40);
        let mut printed: Vec<String> = Vec::new();
        printed.extend(rows_of(&r.push("text\n```rust\nlet x = 1;\n```\nafter\n")));
        printed.extend(rows_of(&r.finish()));
        assert_eq!(printed, vec!["text", "let x = 1;", "after"]);
    }

    #[test]
    fn a_line_arriving_without_a_newline_is_still_printed() {
        let mut r = StreamRenderer::new(40);
        r.push("trailing");
        let frame = r.finish();
        assert_eq!(rows_of(&frame), vec!["trailing"]);
    }

    #[test]
    fn a_blank_line_survives() {
        let mut r = StreamRenderer::new(40);
        let frame = r.push("a\n\nb\n");
        assert_eq!(rows_of(&frame), vec!["a", "", "b"]);
    }

    #[test]
    fn a_two_part_reasoning_summary_lands_on_separate_rows() {
        // The CLI half of the `**One****Two**` bug: the library now sends the
        // blank line between two summary parts, and this renderer is what turns
        // it into rows instead of leaving the headings glued together.
        let mut r = StreamRenderer::new(40);
        let mut printed: Vec<String> = Vec::new();
        printed.extend(rows_of(&r.push("**One**\n\n**Two**")));
        printed.extend(rows_of(&r.finish()));
        assert_eq!(printed, vec!["**One**", "", "**Two**"]);
    }

    #[test]
    fn a_multibyte_answer_is_never_split_inside_a_character() {
        let full = "Código → Build → Test → Deploy → Monitor, repetido varias veces seguidas";
        let mut r = StreamRenderer::new(16);
        let mut printed: Vec<String> = Vec::new();
        for c in full.chars() {
            printed.extend(rows_of(&r.push(&c.to_string())));
        }
        printed.extend(rows_of(&r.finish()));
        assert_eq!(printed.concat(), full);
        for row in &printed {
            assert!(display_width(row) <= 16, "row {row:?} is too wide");
        }
    }
}
