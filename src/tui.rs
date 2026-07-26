use std::io::{IsTerminal, stdin, stdout};

use anyhow::{Context, bail};
use crossterm::event::{self, KeyCode, KeyEvent, KeyModifiers};
use nucleo_matcher::{
    Config, Matcher, Utf32Str,
    pattern::{AtomKind, CaseMatching, Normalization, Pattern},
};
use ratatui::{
    Frame,
    layout::{Constraint, Layout},
    style::{Modifier, Style},
    widgets::{Block, List, ListItem, ListState, Paragraph},
};

use crate::git::Contributor;

pub fn run(contributors: &[Contributor]) -> anyhow::Result<()> {
    if !stdin().is_terminal() || !stdout().is_terminal() {
        bail!("git-credit requires an interactive terminal");
    }

    let mut terminal = ratatui::try_init().context("failed to initialize terminal")?;
    let result = run_list(&mut terminal, contributors);
    ratatui::try_restore().context("failed to restore terminal")?;
    result
}

fn run_list(
    terminal: &mut ratatui::DefaultTerminal,
    contributors: &[Contributor],
) -> anyhow::Result<()> {
    let mut picker = Picker::new(contributors);

    loop {
        terminal.draw(|frame| picker.render(frame, contributors))?;

        let event = event::read().context("failed to read terminal input")?;
        let Some(key) = event.as_key_press_event() else {
            continue;
        };

        if picker.handle_key(key, contributors) {
            return Ok(());
        }
    }
}

struct Picker {
    query: String,
    matches: Vec<usize>,
    cursor: usize,
    matcher: Matcher,
}

impl Picker {
    fn new(contributors: &[Contributor]) -> Self {
        let mut picker = Self {
            query: String::new(),
            matches: Vec::new(),
            cursor: 0,
            matcher: Matcher::new(Config::DEFAULT),
        };
        picker.refilter(contributors);
        picker
    }

    fn handle_key(&mut self, key: KeyEvent, contributors: &[Contributor]) -> bool {
        match key.code {
            KeyCode::Esc => return true,
            KeyCode::Up => self.move_up(),
            KeyCode::Down => self.move_down(),
            KeyCode::Backspace => {
                self.query.pop();
                self.refilter(contributors);
            }
            KeyCode::Char(character)
                if !key.modifiers.intersects(
                    KeyModifiers::CONTROL | KeyModifiers::ALT | KeyModifiers::SUPER,
                ) =>
            {
                self.query.push(character);
                self.refilter(contributors);
            }
            _ => {}
        }

        false
    }

    fn move_up(&mut self) {
        self.cursor = self.cursor.saturating_sub(1);
    }

    fn move_down(&mut self) {
        if self.cursor + 1 < self.matches.len() {
            self.cursor += 1;
        }
    }

    fn refilter(&mut self, contributors: &[Contributor]) {
        self.cursor = 0;

        if self.query.is_empty() {
            self.matches = (0..contributors.len()).collect();
            return;
        }

        let pattern = Pattern::new(
            &self.query,
            CaseMatching::Smart,
            Normalization::Smart,
            AtomKind::Fuzzy,
        );
        let mut utf32_buf = Vec::new();
        let mut matches: Vec<_> = contributors
            .iter()
            .enumerate()
            .filter_map(|(index, contributor)| {
                let identity = format!("{} <{}>", contributor.name, contributor.email);
                pattern
                    .score(Utf32Str::new(&identity, &mut utf32_buf), &mut self.matcher)
                    .map(|score| (index, score))
            })
            .collect();

        matches.sort_by(|(left_index, left_score), (right_index, right_score)| {
            right_score
                .cmp(left_score)
                .then_with(|| left_index.cmp(right_index))
        });
        self.matches = matches.into_iter().map(|(index, _)| index).collect();
    }

    fn render(&self, frame: &mut Frame, contributors: &[Contributor]) {
        let [search_area, list_area, help_area] = Layout::vertical([
            Constraint::Length(3),
            Constraint::Min(1),
            Constraint::Length(1),
        ])
        .areas(frame.area());

        let search = Paragraph::new(format!("{}▏", self.query))
            .block(Block::bordered().title(" Search contributors "));
        frame.render_widget(search, search_area);

        let items = self.matches.iter().map(|index| {
            let contributor = &contributors[*index];
            ListItem::new(format!(
                "{} <{}>  {} commits",
                contributor.name, contributor.email, contributor.commits
            ))
        });
        let list = List::new(items)
            .block(Block::bordered().title(format!(
                " Contributors ({}/{}) ",
                self.matches.len(),
                contributors.len()
            )))
            .highlight_symbol("› ")
            .highlight_style(Style::new().add_modifier(Modifier::REVERSED));
        let mut list_state =
            ListState::default().with_selected((!self.matches.is_empty()).then_some(self.cursor));
        frame.render_stateful_widget(list, list_area, &mut list_state);

        frame.render_widget(Paragraph::new("↑/↓ move  Esc close"), help_area);
    }
}

#[cfg(test)]
mod tests {
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    use super::Picker;
    use crate::git::Contributor;

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

    fn type_character(picker: &mut Picker, character: char, contributors: &[Contributor]) {
        let should_exit = picker.handle_key(
            KeyEvent::new(KeyCode::Char(character), KeyModifiers::NONE),
            contributors,
        );
        assert!(!should_exit);
    }

    #[test]
    fn fuzzy_filters_names_and_emails() {
        let contributors = contributors();
        let mut picker = Picker::new(&contributors);

        for character in "work".chars() {
            type_character(&mut picker, character, &contributors);
        }

        assert_eq!(picker.matches, vec![2]);
    }

    #[test]
    fn clearing_the_query_restores_default_order() {
        let contributors = contributors();
        let mut picker = Picker::new(&contributors);
        type_character(&mut picker, 'b', &contributors);

        picker.query.clear();
        picker.refilter(&contributors);

        assert_eq!(picker.matches, vec![0, 1, 2]);
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

        type_character(&mut picker, 'a', &contributors);
        assert_eq!(picker.cursor, 0);
    }
}
