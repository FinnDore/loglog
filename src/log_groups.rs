use std::sync::{Arc, RwLock};

use crossterm::event::{Event, KeyCode, KeyEventKind};
use fuzzy_matcher::{skim::SkimMatcherV2, FuzzyMatcher};
use ratatui::{
    buffer::Buffer,
    layout::{Constraint, Rect},
    style::{Color, Style},
    text::Line,
    widgets::{Block, HighlightSpacing, Row, StatefulWidget, Table, TableState, Widget},
};
use tokio::sync::mpsc;

#[derive(Debug, Clone)]
pub struct LogGroupListComponent {
    pub(crate) state: Arc<RwLock<LogGroupListState>>,
    sorted_log_groups: Vec<String>,
    search_term: String,
    is_searching: bool,
}

#[derive(Debug)]
pub struct LogGroupListState {
    log_groups: Vec<String>,
    loading_state: LoadingState,
    table_state: TableState,
    group_selection_tx: mpsc::UnboundedSender<LogGroupSelectionOutboundMessage>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum LoadingState {
    #[default]
    Idle,
    Loading,
    Loaded,
    Error(String),
}

pub enum LogGroupSelectionOutboundMessage {
    SelectedGroup(String),
    ApplySearch,
}

impl LogGroupListComponent {
    pub fn new(
        group_selection_tx: mpsc::UnboundedSender<LogGroupSelectionOutboundMessage>,
    ) -> Self {
        Self {
            state: Arc::new(RwLock::new(LogGroupListState {
                log_groups: vec![],
                loading_state: LoadingState::Idle,
                table_state: TableState::default(),
                group_selection_tx,
            })),
            search_term: String::new(),
            is_searching: false,
            sorted_log_groups: vec![],
        }
    }
    pub fn run(&self) {
        let this = self.clone(); // clone the widget to pass to the background task
        tokio::spawn(this.fetch_log_groups());
    }

    async fn fetch_log_groups(self) {
        self.state.write().unwrap().loading_state = LoadingState::Loading;

        let config = aws_config::load_from_env().await;
        let client = aws_sdk_cloudwatchlogs::Client::new(&config);
        let log_groups = match client.describe_log_groups().send().await {
            Ok(response) => Ok(response
                .log_groups
                .unwrap_or_default()
                .into_iter()
                .map(|group| group.log_group_name)
                .flatten()
                .collect::<Vec<String>>()),
            Err(e) => Err(e),
        };

        let mut state = self.state.write().unwrap();
        match log_groups {
            Ok(groups) => {
                state.loading_state = LoadingState::Loaded;
                state.log_groups = groups;
                if !state.log_groups.is_empty() {
                    state.table_state.select_first();
                }
            }
            Err(e) => {
                state.loading_state = LoadingState::Error(e.to_string());
                state.log_groups.clear();
            }
        }
        state
            .group_selection_tx
            .send(LogGroupSelectionOutboundMessage::ApplySearch)
            .unwrap();
    }

    fn scroll_down(&self) {
        self.state.write().unwrap().table_state.scroll_down_by(1);
    }

    fn scroll_up(&self) {
        self.state.write().unwrap().table_state.scroll_up_by(1);
    }

    pub fn apply_search(&mut self) {
        if self.search_term.is_empty() {
            self.sorted_log_groups = self.state.read().unwrap().log_groups.clone();
            return;
        }
        let groups = self.state.read().unwrap().log_groups.clone();
        let matcher = SkimMatcherV2::default();
        self.sorted_log_groups = groups
            .into_iter()
            .map(|group| {
                (
                    group.clone(),
                    matcher.fuzzy_match(&group, &self.search_term),
                )
            })
            .filter(|(_, score)| match score {
                Some(score) => score > &5,
                None => false,
            })
            .map(|(group, _)| group)
            .collect();
    }

    pub fn handle_event(&mut self, event: &Event) -> bool {
        if let Event::Key(key) = event {
            if key.kind == KeyEventKind::Press {
                match key.code {
                    KeyCode::Down => self.scroll_down(),
                    KeyCode::Up => self.scroll_up(),
                    KeyCode::Enter => {
                        let state = self.state.write().unwrap();
                        if let Some(selected) = self
                            .sorted_log_groups
                            .get(state.table_state.selected().unwrap_or(0))
                        {
                            state
                                .group_selection_tx
                                .send(LogGroupSelectionOutboundMessage::SelectedGroup(
                                    selected.clone(),
                                ))
                                .unwrap();
                        }
                    }
                    _ => (),
                };
            }
            if self.is_searching {
                match key.code {
                    KeyCode::Esc => {
                        self.is_searching = false;
                        self.search_term.clear();
                        self.sorted_log_groups = self.state.read().unwrap().log_groups.clone();
                    }
                    KeyCode::Backspace => {
                        self.search_term.pop();
                    }
                    KeyCode::Char(c) => self.search_term.push(c),
                    _ => (),
                }
                self.apply_search();
                return key.code == KeyCode::Esc;
            }
            if key.kind == KeyEventKind::Press {
                match key.code {
                    KeyCode::Char('/') => self.is_searching = !self.is_searching,
                    KeyCode::Char('j') => self.scroll_down(),
                    KeyCode::Char('k') => self.scroll_up(),
                    KeyCode::Char('r') => {
                        if self.state.read().unwrap().loading_state != LoadingState::Loading {
                            let this = self.clone();
                            tokio::spawn(this.fetch_log_groups());
                        }
                    }
                    _ => (),
                };
            }
        }
        false
    }
}

impl Widget for &LogGroupListComponent {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let mut state = self.state.write().unwrap();

        // a block with a right aligned title with the loading state on the right
        let loading_state = Line::from(format!("{:?}", state.loading_state)).right_aligned();
        let title = if self.is_searching {
            Line::styled(
                format!("/{}", self.search_term),
                Style::new().fg(Color::Red),
            )
        } else {
            Line::from("")
        };

        if self.is_searching {}
        let block = Block::bordered()
            .title("Log Groups".to_string())
            .title_bottom(title)
            .title(loading_state)
            .title_bottom(Line::from("q to quit").right_aligned());

        // a table with the list of pull requests
        let rows = self
            .sorted_log_groups
            .iter()
            .map(|log_group| Row::new(vec![log_group.to_string()]));
        let widths = [Constraint::Max(49)];
        let table = Table::new(rows, widths)
            .block(block)
            .highlight_spacing(HighlightSpacing::Always)
            .highlight_symbol("🪵")
            .highlight_style(Style::new().fg(Color::Red));

        StatefulWidget::render(table, area, buf, &mut state.table_state);
    }
}