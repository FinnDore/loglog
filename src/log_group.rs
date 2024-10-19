use std::{
    sync::{Arc, RwLock},
    time::Duration,
};

use aws_sdk_cloudwatchlogs::types::QueryStatus;
use color_eyre::owo_colors::OwoColorize;
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
pub struct LogVieweromponent {
    pub state: Arc<RwLock<LogViewerState>>,
    pub log_group_name: String,
    displayed_messages: Vec<String>,
}

#[derive(Debug)]
pub struct LogViewerState {
    log_messsages: Vec<String>,
    loading_state: LoadingState,
    table_state: TableState,
    group_selection_tx: mpsc::UnboundedSender<LogViewerOutboundMessage>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum LoadingState {
    #[default]
    Idle,
    Loading,
    Loaded,
    Error(String),
}

pub enum LogViewerOutboundMessage {
    ReRender,
    UnselectLogGroup,
}

impl LogVieweromponent {
    pub fn new(group_selection_tx: mpsc::UnboundedSender<LogViewerOutboundMessage>) -> Self {
        Self {
            state: Arc::new(RwLock::new(LogViewerState {
                log_messsages: vec![],
                loading_state: LoadingState::Idle,
                table_state: TableState::default(),
                group_selection_tx,
            })),
            log_group_name: String::new(),
            displayed_messages: vec![],
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
        let query_id = match client
            .start_query()
            .set_start_time(Some(
                chrono::Utc::now().timestamp_millis() - (24 * (3600 * 1000)),
            ))
            .set_end_time(Some(chrono::Utc::now().timestamp_millis()))
            .set_query_string(Some("fields @message".into()))
            .set_log_group_name(self.log_group_name.clone().into())
            .send()
            .await
        {
            Ok(response) => response.query_id,
            Err(e) => panic!("Error: {:?}", e),
        };

        loop {
            tokio::time::sleep(std::time::Duration::from_millis(250)).await;

            match client
                .get_query_results()
                .set_query_id(query_id.clone())
                .send()
                .await
            {
                Ok(response) => {
                    let mut state: std::sync::RwLockWriteGuard<'_, LogViewerState> =
                        self.state.write().unwrap();
                    state.log_messsages = response
                        .results
                        .unwrap_or_default()
                        .into_iter()
                        .flatten()
                        .filter(|result| result.field == Some("@message".to_string()))
                        .map(|result| result.value.unwrap_or_default())
                        .collect::<Vec<String>>();

                    match response.status {
                        Some(QueryStatus::Complete) => {
                            if !state.log_messsages.is_empty() {
                                let num_of_messages = state.log_messsages.len() - 1;
                                state.loading_state = LoadingState::Loaded;
                                state.table_state.select(Some(num_of_messages));
                            }
                            let _ = state
                                .group_selection_tx
                                .send(LogViewerOutboundMessage::ReRender);
                            break;
                        }
                        Some(QueryStatus::Running) => {
                            state.loading_state = LoadingState::Loading;
                        }
                        _ => {
                            state.loading_state = LoadingState::Idle;
                        }
                    }
                }
                Err(e) => panic!("Error: {:?}", e),
            };
        }

        // let mut state = self.state.write().unwrap();
        // match log_groups {
        //     Ok(groups) => {
        //         state.loading_state = LoadingState::Loaded;
        //         state.log_groups = groups;
        //         if !state.log_groups.is_empty() {
        //             state.table_state.select_first();
        //         }
        //     }
        //     Err(e) => {
        //         state.loading_state = LoadingState::Error(e.to_string());
        //         state.log_groups.clear();
        //     }
        // }
    }

    fn scroll_down(&self) {
        self.state.write().unwrap().table_state.scroll_down_by(1);
    }

    fn scroll_up(&self) {
        self.state.write().unwrap().table_state.scroll_up_by(1);
    }

    pub fn set_logs(&mut self) {
        let state = self.state.read().unwrap();
        self.displayed_messages = state.log_messsages.clone()
    }
    pub fn clear_logs(&mut self) {
        let mut state = self.state.write().unwrap();
        state.log_messsages = vec![];
        self.displayed_messages = vec![];
    }

    pub fn handle_event(&mut self, event: &Event) -> bool {
        let key = match event {
            Event::Key(key) => key,
            _ => return false,
        };
        match key.code {
            KeyCode::Esc => {
                let _ = self
                    .state
                    .write()
                    .unwrap()
                    .group_selection_tx
                    .send(LogViewerOutboundMessage::UnselectLogGroup);
                return true;
            }
            KeyCode::Char('k') | KeyCode::Down => self.scroll_up(),
            KeyCode::Char('j') | KeyCode::Up => self.scroll_down(),
            _ => (),
        };
        false
        // false
    }
}

impl Widget for &LogVieweromponent {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let mut state = self.state.write().unwrap();

        // a block with a right aligned title with the loading state on the right
        let loading_state = Line::from(format!("{:?}", state.loading_state)).right_aligned();
        // let title = if self.is_searching {
        //     Line::styled(
        //         format!("/{}", self.search_term),
        //         Style::new().fg(Color::Red),
        //     )
        // } else {
        //     Line::from("")
        // };

        let block = Block::bordered()
            .title(self.log_group_name.to_string())
            .title(loading_state)
            .title_bottom(Line::from("q to quit").right_aligned());

        // a table with the list of pull requests
        let rows = self
            .displayed_messages
            .iter()
            .map(|log_group| Row::new(vec![log_group.to_string()]));
        let widths = [Constraint::Max(49)];
        let table = Table::new(rows, widths)
            .block(block)
            .highlight_spacing(HighlightSpacing::Always)
            .highlight_style(Style::new().bg(Color::LightRed));

        StatefulWidget::render(table, area, buf, &mut state.table_state);
    }
}
