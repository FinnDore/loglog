use std::sync::{Arc, RwLock};

use aws_sdk_cloudwatchlogs::types::QueryStatus;
use crossterm::event::{Event, KeyCode, KeyModifiers};
use ratatui::{
    buffer::Buffer,
    layout::{Constraint, Rect},
    style::{Color, Style},
    text::Line,
    widgets::{Block, HighlightSpacing, Row, StatefulWidget, Table, TableState, Widget},
};
use tokio::sync::mpsc;

use crate::shared::LoadingState;

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

pub enum LogViewerOutboundMessage {
    ReRender,
    UnselectLogGroup,
}

impl LogVieweromponent {
    pub fn new(group_selection_tx: mpsc::UnboundedSender<LogViewerOutboundMessage>) -> Self {
        Self {
            state: Arc::new(RwLock::new(LogViewerState {
                log_messsages: vec![],
                loading_state: LoadingState::Loading,
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
                        .rev()
                        .collect::<Vec<String>>();

                    match response.status {
                        Some(QueryStatus::Complete) => {
                            state.loading_state = LoadingState::Loaded;
                            if !state.log_messsages.is_empty() {
                                let num_of_messages = state.log_messsages.len() - 1;
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
    }

    fn scroll_down(&self, amount: Option<u16>) {
        self.state
            .write()
            .unwrap()
            .table_state
            .scroll_down_by(amount.unwrap_or(1));
    }

    fn scroll_up(&self, amount: Option<u16>) {
        self.state
            .write()
            .unwrap()
            .table_state
            .scroll_up_by(amount.unwrap_or(1));
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
        match (key.code, key.modifiers) {
            (KeyCode::Esc, _) => {
                let _ = self
                    .state
                    .write()
                    .unwrap()
                    .group_selection_tx
                    .send(LogViewerOutboundMessage::UnselectLogGroup);
                return true;
            }
            (KeyCode::Char('r'), _) => self.run(),
            (KeyCode::Char('k') | KeyCode::Up, _) => self.scroll_up(None),
            (KeyCode::Char('j') | KeyCode::Down, _) => self.scroll_down(None),
            (KeyCode::Char('u'), KeyModifiers::CONTROL) => self.scroll_up(Some(10)),
            (KeyCode::Char('d'), KeyModifiers::CONTROL) => self.scroll_down(Some(10)),
            _ => (),
        };
        false
    }
}

impl Widget for &LogVieweromponent {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let mut state = self.state.write().unwrap();
        let loading_state = Line::from(format!("{:?}", state.loading_state)).right_aligned();

        let block = Block::bordered()
            .title(self.log_group_name.to_string())
            .title(loading_state)
            .title_bottom(Line::from("q to quit").right_aligned());

        let rows = self
            .displayed_messages
            .iter()
            .map(|log_group| Row::new(vec![log_group.to_string()]))
            .collect();
        let widths = [Constraint::Fill(1)];

        let table = Table::new(
            if self.displayed_messages.is_empty() && state.loading_state != LoadingState::Loading {
                vec![Row::new(vec!["No logs in this tree".to_string()])]
            } else {
                rows
            },
            widths,
        )
        .block(block)
        .highlight_spacing(HighlightSpacing::Always)
        .highlight_style(Style::new().bg(Color::LightRed));

        StatefulWidget::render(table, area, buf, &mut state.table_state);
    }
}
