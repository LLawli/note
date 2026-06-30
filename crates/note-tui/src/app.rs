//! The Elm-style application model (Model / Msg / update / view).
//!
//! `update` is driven by semantic [`Msg`]s (not raw key events) so the state
//! machine is unit-testable without a terminal; [`App::map_key`] is the only
//! place that knows about crossterm.

use note_core::{Note, NoteId};
use note_store::Store;
use ratatui::Frame;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::layout::{Constraint, Layout};
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Paragraph, Wrap};
use ratatui_bubbletea_theme::BubbleTheme;
use ratatui_tea::{Cmd, Model};
use tui_markdown::{Options, StyleSheet};

const LIST_LIMIT: usize = 500;

/// What the TUI wants the caller to do after it exits.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// Quit the application.
    Quit,
    /// The user asked to edit this note (the CLI runs `$EDITOR`, then re-enters).
    Edit(NoteId),
    /// The user asked to create a note with this (possibly empty) title; the CLI
    /// opens `$EDITOR` seeded with the title as an H1, then re-enters.
    New { title: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mode {
    List,
    View,
    Search,
    Create,
}

/// Semantic messages. The event loop translates key presses into these.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Msg {
    Up,
    Down,
    Open,
    Back,
    Quit,
    Edit,
    StartSearch,
    SearchChar(char),
    SearchBackspace,
    SearchSubmit,
    SearchCancel,
    StartCreate,
    TitleChar(char),
    TitleBackspace,
    CreateSubmit,
    CreateCancel,
    Reload,
}

/// The note browser model.
#[derive(Debug)]
pub struct App<'a> {
    store: &'a Store,
    theme: BubbleTheme,
    mode: Mode,
    notes: Vec<Note>,
    selected: usize,
    search: String,
    title: String,
    scroll: u16,
    status: String,
    running: bool,
    outcome: Outcome,
}

impl<'a> App<'a> {
    #[must_use]
    pub fn new(store: &'a Store) -> Self {
        Self {
            store,
            theme: BubbleTheme::default(),
            mode: Mode::List,
            notes: Vec::new(),
            selected: 0,
            search: String::new(),
            title: String::new(),
            scroll: 0,
            status: String::new(),
            running: true,
            outcome: Outcome::Quit,
        }
    }

    #[must_use]
    pub fn is_running(&self) -> bool {
        self.running
    }

    #[must_use]
    pub fn outcome(&self) -> Outcome {
        self.outcome.clone()
    }

    /// Translate a key press into a semantic message for the current mode.
    #[must_use]
    pub fn map_key(&self, key: KeyEvent) -> Option<Msg> {
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
            return Some(Msg::Quit);
        }
        match self.mode {
            Mode::List => match key.code {
                KeyCode::Char('q') | KeyCode::Esc => Some(Msg::Quit),
                KeyCode::Up | KeyCode::Char('k') => Some(Msg::Up),
                KeyCode::Down | KeyCode::Char('j') => Some(Msg::Down),
                KeyCode::Enter | KeyCode::Char('l') => Some(Msg::Open),
                KeyCode::Char('/') => Some(Msg::StartSearch),
                KeyCode::Char('e') => Some(Msg::Edit),
                KeyCode::Char('n') => Some(Msg::StartCreate),
                _ => None,
            },
            Mode::View => match key.code {
                KeyCode::Char('q') => Some(Msg::Quit),
                KeyCode::Esc | KeyCode::Enter | KeyCode::Char('h') => Some(Msg::Back),
                KeyCode::Up | KeyCode::Char('k') => Some(Msg::Up),
                KeyCode::Down | KeyCode::Char('j') => Some(Msg::Down),
                KeyCode::Char('e') => Some(Msg::Edit),
                _ => None,
            },
            Mode::Search => match key.code {
                KeyCode::Esc => Some(Msg::SearchCancel),
                KeyCode::Enter => Some(Msg::SearchSubmit),
                KeyCode::Backspace => Some(Msg::SearchBackspace),
                KeyCode::Char(c) => Some(Msg::SearchChar(c)),
                _ => None,
            },
            Mode::Create => match key.code {
                KeyCode::Esc => Some(Msg::CreateCancel),
                KeyCode::Enter => Some(Msg::CreateSubmit),
                KeyCode::Backspace => Some(Msg::TitleBackspace),
                KeyCode::Char(c) => Some(Msg::TitleChar(c)),
                _ => None,
            },
        }
    }

    fn current(&self) -> Option<&Note> {
        self.notes.get(self.selected)
    }

    fn reload_all(&mut self) {
        match self.store.readers().list_notes(LIST_LIMIT, 0) {
            Ok(notes) => {
                self.notes = notes;
                self.clamp_selection();
                self.status.clear();
            }
            Err(e) => self.status = format!("error: {e}"),
        }
    }

    fn run_search(&mut self) {
        // Prefix-aware, like `note search`: "mensag" must find "mensagem".
        let result = if self.search.trim().is_empty() {
            self.store.readers().list_notes(LIST_LIMIT, 0)
        } else {
            self.store.readers().search_prefix(&self.search, LIST_LIMIT)
        };
        match result {
            Ok(notes) => {
                self.notes = notes;
                self.selected = 0;
                self.status.clear();
            }
            Err(e) => self.status = format!("error: {e}"),
        }
    }

    fn clamp_selection(&mut self) {
        let max = self.notes.len().saturating_sub(1);
        if self.selected > max {
            self.selected = max;
        }
    }

    /// Move the list cursor down one, staying within bounds via the single
    /// selection-clamp (so the upper bound lives in exactly one place).
    fn select_next(&mut self) {
        self.selected += 1;
        self.clamp_selection();
    }

    /// Upper bound for the View-mode scroll offset: the body's line count, so
    /// scrolling cannot run off into blank space past the note. Approximate
    /// (ignores wrapping) but bounded, which is the point.
    fn max_scroll(&self) -> u16 {
        self.current().map_or(0, |n| {
            u16::try_from(n.body.lines().count()).unwrap_or(u16::MAX)
        })
    }
}

impl Model for App<'_> {
    type Msg = Msg;

    fn init(&mut self) -> Cmd<Self::Msg> {
        Cmd::message(Msg::Reload)
    }

    fn update(&mut self, msg: Self::Msg) -> Cmd<Self::Msg> {
        match msg {
            Msg::Reload => self.reload_all(),
            Msg::Quit => {
                self.outcome = Outcome::Quit;
                self.running = false;
            }
            Msg::Edit => {
                if let Some(note) = self.current() {
                    self.outcome = Outcome::Edit(note.id);
                    self.running = false;
                }
            }
            Msg::Up => match self.mode {
                Mode::View => self.scroll = self.scroll.saturating_sub(1),
                _ => self.selected = self.selected.saturating_sub(1),
            },
            Msg::Down => match self.mode {
                Mode::View => self.scroll = (self.scroll + 1).min(self.max_scroll()),
                _ => self.select_next(),
            },
            Msg::Open => {
                if self.mode == Mode::List && !self.notes.is_empty() {
                    self.mode = Mode::View;
                    self.scroll = 0;
                }
            }
            // Back from a view, and applying a search, both land on the list.
            Msg::Back | Msg::SearchSubmit => self.mode = Mode::List,
            Msg::StartSearch => {
                self.mode = Mode::Search;
                self.search.clear();
                self.run_search(); // empty query -> show the full list, not stale results
            }
            Msg::SearchChar(c) => {
                self.search.push(c);
                self.run_search();
            }
            Msg::SearchBackspace => {
                self.search.pop();
                self.run_search();
            }
            Msg::SearchCancel => {
                self.mode = Mode::List;
                self.search.clear();
                self.reload_all();
            }
            // Create is not gated on `current()`, so `n` works on an empty list
            // (creating the first note) — unlike Edit, which needs a selection.
            Msg::StartCreate => {
                self.mode = Mode::Create;
                self.title.clear();
            }
            Msg::TitleChar(c) => self.title.push(c),
            Msg::TitleBackspace => {
                self.title.pop();
            }
            Msg::CreateSubmit => {
                self.outcome = Outcome::New {
                    title: self.title.trim().to_owned(),
                };
                self.running = false;
            }
            Msg::CreateCancel => {
                self.mode = Mode::List;
                self.title.clear();
            }
        }
        Cmd::none()
    }

    fn view(&self, frame: &mut Frame<'_>) {
        let [main, footer] =
            Layout::vertical([Constraint::Min(0), Constraint::Length(1)]).areas(frame.area());

        match self.mode {
            Mode::List => self.render_list(frame, main),
            Mode::View => self.render_view(frame, main),
            Mode::Search => {
                let [input, results] =
                    Layout::vertical([Constraint::Length(3), Constraint::Min(0)]).areas(main);
                let bar = self
                    .theme
                    .paragraph(self.search.clone())
                    .block(self.theme.titled_block("search"));
                frame.render_widget(bar, input);
                self.render_list(frame, results);
            }
            Mode::Create => {
                let [input, results] =
                    Layout::vertical([Constraint::Length(3), Constraint::Min(0)]).areas(main);
                let bar = self
                    .theme
                    .paragraph(self.title.clone())
                    .block(self.theme.titled_block("new note"));
                frame.render_widget(bar, input);
                self.render_list(frame, results);
            }
        }

        frame.render_widget(Paragraph::new(self.footer_help()), footer);
    }
}

impl App<'_> {
    fn footer_help(&self) -> Line<'_> {
        match self.mode {
            Mode::List => self.theme.help_line([
                ("up/down", "move"),
                ("enter", "open"),
                ("/", "search"),
                ("n", "new"),
                ("e", "edit"),
                ("q", "quit"),
            ]),
            Mode::View => self.theme.help_line([
                ("up/down", "scroll"),
                ("e", "edit"),
                ("esc", "back"),
                ("q", "quit"),
            ]),
            Mode::Search => {
                self.theme
                    .help_line([("type", "filter"), ("enter", "apply"), ("esc", "cancel")])
            }
            Mode::Create => self.theme.help_line([
                ("type", "title"),
                ("enter", "open editor"),
                ("esc", "cancel"),
            ]),
        }
    }

    fn render_list(&self, frame: &mut Frame<'_>, area: ratatui::layout::Rect) {
        let title = format!("notes ({})", self.notes.len());
        let lines: Vec<Line<'_>> = if self.notes.is_empty() {
            vec![Line::from(self.theme.muted("(no notes)"))]
        } else {
            self.notes
                .iter()
                .enumerate()
                .map(|(i, note)| {
                    let title = note.display_title();
                    if i == self.selected {
                        Line::from(self.theme.accent(format!("> {title}")))
                    } else {
                        Line::from(self.theme.span(format!("  {title}")))
                    }
                })
                .collect()
        };
        let para = Paragraph::new(lines).block(self.theme.titled_block(title));
        frame.render_widget(para, area);
    }

    fn render_view(&self, frame: &mut Frame<'_>, area: ratatui::layout::Rect) {
        let Some(note) = self.current() else {
            frame.render_widget(self.theme.paragraph(self.status.clone()), area);
            return;
        };
        // Markdown notes are rendered (headings/bold/lists styled); plain notes
        // are shown verbatim. termimad can't draw into a ratatui frame, so the
        // TUI uses tui-markdown, then strips the `#`/``` markers it emits raw.
        let text: Text<'_> = if note.content_kind.is_markdown() {
            render_markdown(&note.body)
        } else {
            Text::raw(note.body.as_str())
        };
        let para = Paragraph::new(text)
            .block(self.theme.titled_block(note.display_title()))
            .wrap(Wrap { trim: false })
            .scroll((self.scroll, 0));
        frame.render_widget(para, area);
    }
}

/// Markers that redraw a fenced code block once its raw ``` fences are removed:
/// a top rule that carries the language label, a left gutter, and a bottom rule.
const CODE_TOP: &str = "┌─ ";
const CODE_GUTTER: &str = "│ ";
const CODE_BOTTOM: &str = "└─";

/// Markdown theme for the in-frame renderer. A terminal can't change font size,
/// so heading levels are differentiated by color/weight/decoration: H1 is a
/// reversed banner so it can't be mistaken for the plain-bold H2, then each
/// lower level drops a degree of emphasis.
#[derive(Clone, Copy, Debug)]
struct MarkdownStyle;

impl StyleSheet for MarkdownStyle {
    fn heading(&self, level: u8) -> Style {
        match level {
            1 => Style::new().fg(Color::Black).bg(Color::Cyan).bold(),
            2 => Style::new().fg(Color::Cyan).bold(),
            3 => Style::new().fg(Color::Cyan).italic(),
            4 => Style::new().fg(Color::Cyan).dim(),
            _ => Style::new().fg(Color::Cyan).dim().italic(),
        }
    }

    fn code(&self) -> Style {
        Style::new().fg(Color::Gray)
    }

    fn link(&self) -> Style {
        Style::new().fg(Color::Blue).underlined()
    }

    fn blockquote(&self) -> Style {
        Style::new().fg(Color::Green).italic()
    }

    fn heading_meta(&self) -> Style {
        Style::new().dim()
    }

    fn metadata_block(&self) -> Style {
        Style::new().fg(Color::LightYellow)
    }
}

/// Render a note's markdown to themed ratatui `Text`, then clean up the raw
/// markers tui-markdown emits verbatim (heading `#`s and code-fence lines).
fn render_markdown(body: &str) -> Text<'_> {
    let options = Options::new(MarkdownStyle);
    clean_markdown(tui_markdown::from_str_with_options(body, &options))
}

/// Rewrite the raw markers tui-markdown emits verbatim: strip the `#…` heading
/// prefixes, and replace a fenced code block's ``` fences with a light box — a
/// top rule labelled with the language, a left gutter, and a bottom rule — so it
/// reads as a code block (and still shows e.g. that it is `json`).
fn clean_markdown(text: Text<'_>) -> Text<'_> {
    let rule = Style::new().fg(Color::DarkGray);
    let lang_style = Style::new().fg(Color::Gray);
    let mut lines: Vec<Line<'_>> = Vec::with_capacity(text.lines.len());
    let mut in_code = false;
    for mut line in text.lines {
        // A fence is a single span: ```` ```lang ```` opens, ```` ``` ```` closes.
        let fence_lang = line
            .spans
            .first()
            .and_then(|s| s.content.strip_prefix("```"))
            .map(|lang| lang.trim().to_owned());
        if let Some(lang) = fence_lang {
            if in_code {
                in_code = false;
                lines.push(Line::from(Span::styled(CODE_BOTTOM, rule)));
            } else {
                in_code = true;
                let top = if lang.is_empty() {
                    Line::from(Span::styled(CODE_TOP.trim_end(), rule))
                } else {
                    Line::from(vec![
                        Span::styled(CODE_TOP, rule),
                        Span::styled(lang, lang_style),
                    ])
                };
                lines.push(top);
            }
            continue;
        }
        if in_code {
            line.spans.insert(0, Span::styled(CODE_GUTTER, rule));
        } else if line
            .spans
            .first()
            .is_some_and(|s| is_hash_prefix(&s.content))
        {
            line.spans.remove(0);
        }
        lines.push(line);
    }
    Text::from(lines)
}

/// True for a tui-markdown heading prefix span: 1–6 `#` then a single space.
fn is_hash_prefix(s: &str) -> bool {
    s.strip_suffix(' ')
        .is_some_and(|hashes| (1..=6).contains(&hashes.len()) && hashes.bytes().all(|b| b == b'#'))
}

#[cfg(test)]
mod tests {
    use super::*;
    use note_core::ContentKind;
    use note_store::{NewNote, Store};
    use std::collections::BTreeSet;

    fn store_with(titles: &[&str]) -> (Store, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path().join("notes.sqlite")).unwrap();
        for t in titles {
            store
                .writer()
                .create_note(NewNote {
                    title: Some((*t).to_owned()),
                    body: format!("body of {t}"),
                    content_kind: ContentKind::Markdown,
                    tags: BTreeSet::new(),
                    links: Vec::new(),
                })
                .unwrap();
        }
        (store, dir)
    }

    fn loaded(store: &Store) -> App<'_> {
        let mut app = App::new(store);
        app.update(Msg::Reload);
        app
    }

    #[test]
    fn reload_populates_notes() {
        let (store, _d) = store_with(&["a", "b", "c"]);
        let app = loaded(&store);
        assert_eq!(app.notes.len(), 3);
    }

    #[test]
    fn down_and_up_move_selection_within_bounds() {
        let (store, _d) = store_with(&["a", "b"]);
        let mut app = loaded(&store);
        assert_eq!(app.selected, 0);
        app.update(Msg::Down);
        assert_eq!(app.selected, 1);
        app.update(Msg::Down); // clamped at last
        assert_eq!(app.selected, 1);
        app.update(Msg::Up);
        assert_eq!(app.selected, 0);
        app.update(Msg::Up); // clamped at first
        assert_eq!(app.selected, 0);
    }

    #[test]
    fn open_and_back_switch_modes() {
        let (store, _d) = store_with(&["a"]);
        let mut app = loaded(&store);
        assert_eq!(app.mode, Mode::List);
        app.update(Msg::Open);
        assert_eq!(app.mode, Mode::View);
        app.update(Msg::Back);
        assert_eq!(app.mode, Mode::List);
    }

    #[test]
    fn quit_stops_with_quit_outcome() {
        let (store, _d) = store_with(&["a"]);
        let mut app = loaded(&store);
        app.update(Msg::Quit);
        assert!(!app.is_running());
        assert_eq!(app.outcome(), Outcome::Quit);
    }

    #[test]
    fn edit_returns_edit_outcome_for_selected() {
        let (store, _d) = store_with(&["a", "b"]);
        let mut app = loaded(&store);
        app.update(Msg::Down);
        let expected = app.notes[1].id;
        app.update(Msg::Edit);
        assert!(!app.is_running());
        assert_eq!(app.outcome(), Outcome::Edit(expected));
    }

    #[test]
    fn search_filters_then_cancel_restores() {
        let (store, _d) = store_with(&["alpha", "beta", "gamma"]);
        let mut app = loaded(&store);
        app.update(Msg::StartSearch);
        assert_eq!(app.mode, Mode::Search);
        for c in "alpha".chars() {
            app.update(Msg::SearchChar(c));
        }
        assert_eq!(app.notes.len(), 1);
        assert_eq!(app.notes[0].display_title(), "alpha");
        app.update(Msg::SearchCancel);
        assert_eq!(app.mode, Mode::List);
        assert_eq!(app.notes.len(), 3);
    }

    #[test]
    fn search_matches_word_prefixes() {
        let (store, _d) = store_with(&["mensagem", "outra"]);
        // store_with sets body to "body of <title>", so search the titles' words.
        let mut app = loaded(&store);
        app.update(Msg::StartSearch);
        for c in "mensag".chars() {
            app.update(Msg::SearchChar(c));
        }
        assert_eq!(app.notes.len(), 1);
        assert_eq!(app.notes[0].display_title(), "mensagem");
    }

    #[test]
    fn start_search_resets_to_full_list() {
        let (store, _d) = store_with(&["alpha", "beta", "gamma"]);
        let mut app = loaded(&store);
        app.update(Msg::StartSearch);
        for c in "alpha".chars() {
            app.update(Msg::SearchChar(c));
        }
        assert_eq!(app.notes.len(), 1);
        // re-opening search must clear the stale filtered list, not keep it
        app.update(Msg::SearchCancel);
        app.update(Msg::StartSearch);
        assert_eq!(app.notes.len(), 3);
    }

    #[test]
    fn scroll_changes_in_view_mode_only() {
        let (store, _d) = store_with(&["a"]);
        let mut app = loaded(&store);
        app.update(Msg::Down); // list mode: moves selection (clamped), scroll stays 0
        assert_eq!(app.scroll, 0);
        app.update(Msg::Open);
        app.update(Msg::Down); // view mode: scrolls
        assert_eq!(app.scroll, 1);
        app.update(Msg::Up);
        assert_eq!(app.scroll, 0);
    }

    #[test]
    fn map_key_is_mode_aware() {
        let (store, _d) = store_with(&["a"]);
        let mut app = loaded(&store);
        let slash = KeyEvent::from(KeyCode::Char('/'));
        assert_eq!(app.map_key(slash), Some(Msg::StartSearch));
        app.update(Msg::StartSearch);
        // in search mode, '/' is just a character
        assert_eq!(app.map_key(slash), Some(Msg::SearchChar('/')));
    }

    #[test]
    fn start_create_enters_create_mode() {
        let (store, _d) = store_with(&["a"]);
        let mut app = loaded(&store);
        app.update(Msg::StartCreate);
        assert_eq!(app.mode, Mode::Create);
        assert_eq!(app.title, "");
    }

    #[test]
    fn typing_title_builds_buffer() {
        let (store, _d) = store_with(&["a"]);
        let mut app = loaded(&store);
        app.update(Msg::StartCreate);
        for c in "idea".chars() {
            app.update(Msg::TitleChar(c));
        }
        assert_eq!(app.title, "idea");
        app.update(Msg::TitleBackspace);
        assert_eq!(app.title, "ide");
    }

    #[test]
    fn create_submit_returns_new_outcome_trimmed() {
        let (store, _d) = store_with(&["a"]);
        let mut app = loaded(&store);
        app.update(Msg::StartCreate);
        for c in "  ideas  ".chars() {
            app.update(Msg::TitleChar(c));
        }
        app.update(Msg::CreateSubmit);
        assert!(!app.is_running());
        assert_eq!(
            app.outcome(),
            Outcome::New {
                title: "ideas".to_owned()
            }
        );
    }

    #[test]
    fn create_cancel_returns_to_list() {
        let (store, _d) = store_with(&["a"]);
        let mut app = loaded(&store);
        app.update(Msg::StartCreate);
        app.update(Msg::TitleChar('x'));
        app.update(Msg::CreateCancel);
        assert_eq!(app.mode, Mode::List);
        assert_eq!(app.title, "");
        assert!(app.is_running());
    }

    #[test]
    fn create_works_on_empty_store() {
        // `n` must work with zero notes (create the first one), unlike `e`.
        let (store, _d) = store_with(&[]);
        let mut app = loaded(&store);
        assert!(app.notes.is_empty());
        app.update(Msg::StartCreate);
        app.update(Msg::CreateSubmit);
        assert!(!app.is_running());
        assert_eq!(
            app.outcome(),
            Outcome::New {
                title: String::new()
            }
        );
    }

    #[test]
    fn markdown_strips_hash_and_boxes_code_block() {
        let text = render_markdown("# Title\n\nbody\n\n```json\n{\"a\":1}\n```\n");
        let spans: Vec<String> = text
            .lines
            .iter()
            .flat_map(|l| l.spans.iter().map(|s| s.content.to_string()))
            .collect();
        // heading marker gone, heading text kept
        assert!(
            !spans.iter().any(|s| s == "# "),
            "the '# ' marker must be stripped"
        );
        assert!(spans.iter().any(|s| s == "Title"), "heading text stays");
        // no raw fence line survives
        for line in &text.lines {
            let joined: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
            assert!(
                !joined.starts_with("```"),
                "fence line must be stripped: {joined:?}"
            );
        }
        // the language label is shown instead of the raw fence
        assert!(
            spans.iter().any(|s| s == "json"),
            "code block should label its language"
        );
        // the block is boxed: a top rule and a left gutter
        let starts_with = |ch: char| {
            text.lines
                .iter()
                .any(|l| l.spans.first().is_some_and(|s| s.content.starts_with(ch)))
        };
        assert!(starts_with('┌'), "code block should have a top rule");
        assert!(starts_with('│'), "code interior should have a gutter");
    }

    #[test]
    fn map_key_create_mode() {
        let (store, _d) = store_with(&["a"]);
        let mut app = loaded(&store);
        assert_eq!(
            app.map_key(KeyEvent::from(KeyCode::Char('n'))),
            Some(Msg::StartCreate)
        );
        app.update(Msg::StartCreate);
        // in create mode, 'n' is literal text; Enter/Esc submit/cancel.
        assert_eq!(
            app.map_key(KeyEvent::from(KeyCode::Char('n'))),
            Some(Msg::TitleChar('n'))
        );
        assert_eq!(
            app.map_key(KeyEvent::from(KeyCode::Enter)),
            Some(Msg::CreateSubmit)
        );
        assert_eq!(
            app.map_key(KeyEvent::from(KeyCode::Esc)),
            Some(Msg::CreateCancel)
        );
        assert_eq!(
            app.map_key(KeyEvent::from(KeyCode::Backspace)),
            Some(Msg::TitleBackspace)
        );
    }
}
