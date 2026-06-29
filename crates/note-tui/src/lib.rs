//! note-tui: the ratatui-tea interactive UI for `note`.
//!
//! Elm-style Model/Msg/Cmd app over `note-store`. The terminal is restored on
//! every exit path including panic (via `ratatui::init`'s panic hook). The TUI
//! never writes logs to the terminal it draws on (it does not log at all).

mod app;

pub use app::{App, Msg, Outcome};

use note_store::Store;
use ratatui::Terminal;
use ratatui::backend::Backend;
use ratatui::crossterm::event::{self, Event, KeyEventKind};
use ratatui_tea::Program;
use std::io;
use std::time::Duration;

const POLL: Duration = Duration::from_millis(250);

/// Launch the interactive browser over `store`. Sets up the terminal (raw mode +
/// alternate screen + panic-restoring hook), runs the event loop, and always
/// restores the terminal before returning. The returned [`Outcome`] tells the
/// caller whether the user quit or asked to edit a note.
pub fn run(store: &Store) -> io::Result<Outcome> {
    let mut terminal = ratatui::try_init()?;
    let outcome = event_loop(store, &mut terminal);
    let _ = ratatui::try_restore();
    outcome
}

fn event_loop<B: Backend>(store: &Store, terminal: &mut Terminal<B>) -> io::Result<Outcome>
where
    B::Error: Into<io::Error>,
{
    let mut program = Program::new(App::new(store));
    program.init();

    while program.model().is_running() {
        program.draw(terminal).map_err(Into::into)?;

        if event::poll(POLL)?
            && let Event::Key(key) = event::read()?
            && key.kind == KeyEventKind::Press
            && let Some(msg) = program.model().map_key(key)
        {
            program.send(msg);
        }
    }
    Ok(program.model().outcome())
}

#[cfg(test)]
mod tests {
    use super::*;
    use note_core::ContentKind;
    use note_store::{NewNote, Store};
    use ratatui::backend::TestBackend;
    use ratatui_tea::Model;
    use std::collections::BTreeSet;

    #[test]
    fn renders_the_note_list_to_a_test_backend() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path().join("notes.sqlite")).unwrap();
        store
            .writer()
            .create_note(NewNote {
                title: Some("Hello TUI".to_owned()),
                body: "body".to_owned(),
                content_kind: ContentKind::Markdown,
                tags: BTreeSet::new(),
                links: Vec::new(),
            })
            .unwrap();

        let mut app = App::new(&store);
        let _ = app.update(Msg::Reload);
        let program = Program::new(app);

        let backend = TestBackend::new(40, 10);
        let mut terminal = Terminal::new(backend).unwrap();
        program.draw(&mut terminal).unwrap();

        let rendered = terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(ratatui::buffer::Cell::symbol)
            .collect::<String>();
        assert!(
            rendered.contains("Hello TUI"),
            "list should render the note title"
        );
        assert!(rendered.contains("notes (1)"), "list should show the count");
    }

    #[test]
    fn renders_markdown_in_view_mode() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path().join("notes.sqlite")).unwrap();
        store
            .writer()
            .create_note(NewNote {
                title: None,
                body: "# Heading One\n\nsome body text".to_owned(),
                content_kind: ContentKind::Markdown,
                tags: BTreeSet::new(),
                links: Vec::new(),
            })
            .unwrap();

        let mut app = App::new(&store);
        let _ = app.update(Msg::Reload);
        let _ = app.update(Msg::Open); // enter View mode
        let program = Program::new(app);

        let backend = TestBackend::new(40, 12);
        let mut terminal = Terminal::new(backend).unwrap();
        program.draw(&mut terminal).unwrap();

        let rendered = terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(ratatui::buffer::Cell::symbol)
            .collect::<String>();
        // markdown is rendered (tui-markdown styles the heading; it keeps the
        // '#' marker, unlike the CLI's termimad which strips it).
        assert!(
            rendered.contains("Heading One"),
            "heading text should render"
        );
        assert!(rendered.contains("some body text"), "body should render");
    }
}
