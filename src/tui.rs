use std::{
    collections::BTreeSet,
    io::{IsTerminal, stdin, stdout},
};

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
    widgets::{Block, List, ListItem, ListState, Paragraph, Wrap},
};

use crate::git::{CommitInfo, Contributor};

pub fn run(
    contributors: &[Contributor],
    head_oid: &str,
    commit: &CommitInfo,
) -> anyhow::Result<Option<Vec<usize>>> {
    if !stdin().is_terminal() || !stdout().is_terminal() {
        bail!("git-credit requires an interactive terminal");
    }

    let mut terminal = ratatui::try_init().context("failed to initialize terminal")?;
    let result = run_list(&mut terminal, contributors, head_oid, commit);
    ratatui::try_restore().context("failed to restore terminal")?;
    result
}

fn run_list(
    terminal: &mut ratatui::DefaultTerminal,
    contributors: &[Contributor],
    head_oid: &str,
    commit: &CommitInfo,
) -> anyhow::Result<Option<Vec<usize>>> {
    let mut picker = Picker::new(contributors);

    loop {
        terminal.draw(|frame| picker.render(frame, contributors, head_oid, commit))?;

        let event = event::read().context("failed to read terminal input")?;
        let Some(key) = event.as_key_press_event() else {
            continue;
        };

        match picker.handle_key(key, contributors) {
            Action::Continue => {}
            Action::Confirm => return Ok(Some(picker.selected_indices())),
            Action::Cancel => return Ok(None),
        }
    }
}

struct Picker {
    view: View,
    query: String,
    matches: Vec<usize>,
    cursor: usize,
    selected: BTreeSet<usize>,
    matcher: Matcher,
}

impl Picker {
    fn new(contributors: &[Contributor]) -> Self {
        let mut picker = Self {
            view: View::Picker,
            query: String::new(),
            matches: Vec::new(),
            cursor: 0,
            selected: BTreeSet::new(),
            matcher: Matcher::new(Config::DEFAULT),
        };
        picker.refilter(contributors);
        picker
    }

    fn handle_key(&mut self, key: KeyEvent, contributors: &[Contributor]) -> Action {
        match self.view {
            View::Picker => self.handle_picker_key(key, contributors),
            View::Confirmation => self.handle_confirmation_key(key),
        }
    }

    fn handle_picker_key(&mut self, key: KeyEvent, contributors: &[Contributor]) -> Action {
        match key.code {
            KeyCode::Esc => return Action::Cancel,
            KeyCode::Enter if self.selected.is_empty() => return Action::Confirm,
            KeyCode::Enter => self.view = View::Confirmation,
            KeyCode::Up => self.move_up(),
            KeyCode::Down => self.move_down(),
            KeyCode::Backspace => {
                self.query.pop();
                self.refilter(contributors);
            }
            KeyCode::Char(' ')
                if !key.modifiers.intersects(
                    KeyModifiers::CONTROL | KeyModifiers::ALT | KeyModifiers::SUPER,
                ) =>
            {
                self.toggle_selected();
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

        Action::Continue
    }

    fn handle_confirmation_key(&mut self, key: KeyEvent) -> Action {
        match key.code {
            KeyCode::Enter => Action::Confirm,
            KeyCode::Esc => {
                self.view = View::Picker;
                Action::Continue
            }
            _ => Action::Continue,
        }
    }

    fn move_up(&mut self) {
        self.cursor = self.cursor.saturating_sub(1);
    }

    fn move_down(&mut self) {
        if self.cursor + 1 < self.matches.len() {
            self.cursor += 1;
        }
    }

    fn toggle_selected(&mut self) {
        let Some(index) = self.matches.get(self.cursor).copied() else {
            return;
        };

        if !self.selected.remove(&index) {
            self.selected.insert(index);
        }
    }

    fn selected_indices(&self) -> Vec<usize> {
        self.selected.iter().copied().collect()
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

    fn render(
        &self,
        frame: &mut Frame,
        contributors: &[Contributor],
        head_oid: &str,
        commit: &CommitInfo,
    ) {
        match self.view {
            View::Picker => self.render_picker(frame, contributors, head_oid, commit),
            View::Confirmation => self.render_confirmation(frame, contributors),
        }
    }

    fn render_picker(
        &self,
        frame: &mut Frame,
        contributors: &[Contributor],
        head_oid: &str,
        commit: &CommitInfo,
    ) {
        let message_height = frame.area().height.saturating_sub(8).min(8);
        let [header_area, message_area, search_area, list_area, help_area] = Layout::vertical([
            Constraint::Length(1),
            Constraint::Length(message_height),
            Constraint::Length(3),
            Constraint::Min(1),
            Constraint::Length(1),
        ])
        .areas(frame.area());

        let abbreviated_oid = head_oid.get(..8).unwrap_or(head_oid);
        frame.render_widget(
            Paragraph::new(format!(
                "HEAD {abbreviated_oid}  Author: {} <{}>",
                commit.author_name, commit.author_email
            )),
            header_area,
        );
        frame.render_widget(
            Paragraph::new(commit.message.as_str())
                .block(Block::bordered().title(" Commit message "))
                .wrap(Wrap { trim: false }),
            message_area,
        );

        let search = Paragraph::new(format!("{}▏", self.query))
            .block(Block::bordered().title(" Search contributors "));
        frame.render_widget(search, search_area);

        let items = self.matches.iter().map(|index| {
            let contributor = &contributors[*index];
            let marker = if self.selected.contains(index) {
                "[x]"
            } else {
                "[ ]"
            };
            ListItem::new(format!(
                "{marker} {} <{}>  {} commits",
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

        frame.render_widget(
            Paragraph::new("↑/↓ move  Space select  Enter continue  Esc cancel"),
            help_area,
        );
    }

    fn render_confirmation(&self, frame: &mut Frame, contributors: &[Contributor]) {
        let [body_area, help_area] =
            Layout::vertical([Constraint::Min(1), Constraint::Length(1)]).areas(frame.area());

        let mut body = String::from("Add these co-authors to HEAD?\n\n");
        for index in &self.selected {
            let contributor = &contributors[*index];
            body.push_str(&format!(
                "Co-authored-by: {} <{}>\n",
                contributor.name, contributor.email
            ));
        }
        body.push_str("\nAmending HEAD will change its commit ID.");

        frame.render_widget(
            Paragraph::new(body).block(Block::bordered().title(" Confirm co-authors ")),
            body_area,
        );
        frame.render_widget(Paragraph::new("Enter confirm  Esc back"), help_area);
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum View {
    Picker,
    Confirmation,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Action {
    Continue,
    Confirm,
    Cancel,
}

#[cfg(test)]
mod tests;
