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
use ratatui::crossterm::event::{
    self, DisableMouseCapture, EnableMouseCapture, Event, KeyEventKind,
};
use ratatui::crossterm::execute;
use ratatui_tea::Program;
use std::io::{self, stdout};

/// Launch the interactive browser over `store`. Sets up the terminal (raw mode +
/// alternate screen + panic-restoring hook), runs the event loop, and always
/// restores the terminal before returning. The returned [`Outcome`] tells the
/// caller whether the user quit or asked to edit a note.
pub fn run(store: &Store) -> io::Result<Outcome> {
    let mut terminal = ratatui::try_init()?;
    enable_mouse();
    let outcome = event_loop(store, &mut terminal);
    let _ = execute!(stdout(), DisableMouseCapture);
    let _ = ratatui::try_restore();
    outcome
}

/// Enable mouse capture and chain a panic hook that disables it. `ratatui`'s own
/// panic hook restores raw mode and the alternate screen but not mouse capture,
/// so without this a panic mid-session would leave the terminal emitting mouse
/// escape sequences. Disabling on normal exit happens in `run`.
fn enable_mouse() {
    let _ = execute!(stdout(), EnableMouseCapture);
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let _ = execute!(stdout(), DisableMouseCapture);
        previous(info);
    }));
}

fn event_loop<B: Backend>(store: &Store, terminal: &mut Terminal<B>) -> io::Result<Outcome>
where
    B::Error: Into<io::Error>,
{
    let mut program = Program::new(App::new(store));
    program.init();
    program.draw(terminal).map_err(Into::into)?; // initial frame

    // Event-driven redraw: only repaint after a handled key or a resize, so an
    // idle TUI does no work (e.g. no re-parsing the viewed note's markdown).
    while program.model().is_running() {
        let redraw = match event::read()? {
            Event::Key(key) if key.kind == KeyEventKind::Press => {
                match program.model().map_key(key) {
                    Some(msg) => {
                        program.send(msg);
                        true
                    }
                    None => false,
                }
            }
            Event::Mouse(ev) => {
                let size = terminal.size().map_err(Into::into)?;
                match program.model().map_mouse(ev, size) {
                    Some(msg) => {
                        program.send(msg);
                        true
                    }
                    None => false,
                }
            }
            Event::Resize(_, _) => true,
            _ => false,
        };
        if redraw {
            program.draw(terminal).map_err(Into::into)?;
        }
    }
    Ok(program.model().outcome())
}

#[cfg(test)]
mod tests {
    use super::*;
    use note_core::{ContentKind, WikiLink, WikiTarget};
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
        // markdown is rendered and the heading `#` marker is stripped by
        // render_markdown (the heading text and its per-level style remain).
        assert!(
            rendered.contains("Heading One"),
            "heading text should render"
        );
        assert!(
            !rendered.contains("# Heading One"),
            "the '#' marker should be stripped"
        );
        assert!(rendered.contains("some body text"), "body should render");
    }

    #[test]
    fn renders_create_prompt() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path().join("notes.sqlite")).unwrap();

        let mut app = App::new(&store);
        let _ = app.update(Msg::Reload);
        let _ = app.update(Msg::StartCreate);
        for c in "my title".chars() {
            let _ = app.update(Msg::TitleChar(c));
        }
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
            rendered.contains("new note"),
            "create prompt should show the 'new note' block title"
        );
        assert!(
            rendered.contains("my title"),
            "create prompt should echo the typed title"
        );
    }

    #[test]
    fn renders_links_panel() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path().join("notes.sqlite")).unwrap();
        // A single note with one (dangling) link, so the opened note and its
        // panel are unambiguous regardless of list ordering.
        store
            .writer()
            .create_note(NewNote {
                title: Some("Source".to_owned()),
                body: "see [[Target]]".to_owned(),
                content_kind: ContentKind::Markdown,
                tags: BTreeSet::new(),
                links: vec![WikiLink {
                    target: WikiTarget::ByTitle("Target".to_owned()),
                    display: None,
                }],
            })
            .unwrap();

        let mut app = App::new(&store);
        let _ = app.update(Msg::Reload);
        let _ = app.update(Msg::Open); // view Source (most recent)
        let _ = app.update(Msg::OpenLinks);
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
        assert!(rendered.contains("links"), "panel title should render");
        assert!(
            rendered.contains("Target"),
            "the outgoing link label should render"
        );
    }
}
