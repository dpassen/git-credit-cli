use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{Terminal, backend::TestBackend};

use super::{Action, Picker, View};
use crate::git::{CommitInfo, Contributor};

fn contributors() -> Vec<Contributor> {
    vec![
        Contributor {
            name: "Alice Example".to_owned(),
            email: "alice@example.com".to_owned(),
            commits: 10,
        },
        Contributor {
            name: "Bob Smith".to_owned(),
            email: "bob@example.com".to_owned(),
            commits: 5,
        },
        Contributor {
            name: "Carol Jones".to_owned(),
            email: "carol@work.test".to_owned(),
            commits: 2,
        },
    ]
}

fn commit_info() -> CommitInfo {
    CommitInfo {
        author_name: "Current Author".to_owned(),
        author_email: "current@example.com".to_owned(),
        message:
            "Improve contributor selection\n\nShow commit context before choosing co-authors.\n"
                .to_owned(),
    }
}

fn render(picker: &Picker<'_>) -> String {
    let backend = TestBackend::new(80, 24);
    let mut terminal = Terminal::new(backend).unwrap();
    let commit = commit_info();
    terminal
        .draw(|frame| {
            picker.render(frame, "0123456789abcdef0123456789abcdef01234567", &commit);
        })
        .unwrap();
    terminal.backend().to_string()
}

fn type_character(picker: &mut Picker<'_>, character: char) {
    let action = picker.handle_key(KeyEvent::new(KeyCode::Char(character), KeyModifiers::NONE));
    assert_eq!(action, Action::Continue);
}

#[test]
fn renders_the_default_picker() {
    let contributors = contributors();
    let picker = Picker::new(&contributors);

    insta::assert_snapshot!(render(&picker));
}

#[test]
fn renders_confirmation() {
    let contributors = contributors();
    let mut picker = Picker::new(&contributors);
    picker.toggle_selected();
    picker.move_down();
    picker.toggle_selected();
    picker.view = View::Confirmation;

    insta::assert_snapshot!(render(&picker));
}

#[test]
fn fuzzy_filters_names_and_emails() {
    let contributors = contributors();
    let mut picker = Picker::new(&contributors);

    for character in "work".chars() {
        type_character(&mut picker, character);
    }

    assert_eq!(picker.matches, vec![2]);
}

#[test]
fn clearing_the_query_restores_default_order() {
    let contributors = contributors();
    let mut picker = Picker::new(&contributors);
    type_character(&mut picker, 'b');

    picker.query.clear();
    picker.refilter();

    assert_eq!(picker.matches, vec![0, 1, 2]);
}

#[test]
fn selections_survive_filter_changes() {
    let contributors = contributors();
    let mut picker = Picker::new(&contributors);
    picker.toggle_selected();

    for character in "work".chars() {
        type_character(&mut picker, character);
    }
    picker.toggle_selected();

    assert_eq!(picker.selected.into_iter().collect::<Vec<_>>(), vec![0, 2]);
}

#[test]
fn enter_with_selections_opens_confirmation() {
    let contributors = contributors();
    let mut picker = Picker::new(&contributors);
    picker.toggle_selected();

    let action = picker.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

    assert_eq!(action, Action::Continue);
    assert_eq!(picker.view, View::Confirmation);
}

#[test]
fn escape_from_confirmation_returns_to_picker() {
    let contributors = contributors();
    let mut picker = Picker::new(&contributors);
    picker.view = View::Confirmation;

    let action = picker.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));

    assert_eq!(action, Action::Continue);
    assert_eq!(picker.view, View::Picker);
}

#[test]
fn enter_on_confirmation_confirms_the_selection() {
    let contributors = contributors();
    let mut picker = Picker::new(&contributors);
    picker.toggle_selected();
    picker.view = View::Confirmation;

    let action = picker.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

    assert_eq!(action, Action::Confirm);
    assert_eq!(picker.selected_contributors(), vec![&contributors[0]]);
}

#[test]
fn escape_cancels() {
    let contributors = contributors();
    let mut picker = Picker::new(&contributors);

    let action = picker.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));

    assert_eq!(action, Action::Cancel);
}

#[test]
fn cursor_is_clamped_and_resets_after_filtering() {
    let contributors = contributors();
    let mut picker = Picker::new(&contributors);

    picker.move_down();
    picker.move_down();
    picker.move_down();
    assert_eq!(picker.cursor, 2);

    picker.move_up();
    assert_eq!(picker.cursor, 1);

    type_character(&mut picker, 'a');
    assert_eq!(picker.cursor, 0);
}
