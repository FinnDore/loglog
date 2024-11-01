use std::{
    fs,
    sync::{Arc, RwLock},
};

use aws_sdk_cloudwatchlogs::types::QueryStatus;
use crossterm::event::{Event, KeyCode, KeyModifiers};
use ratatui::{
    buffer::Buffer,
    layout::{Constraint, Rect},
    style::{Color, Style},
    text::Line,
    widgets::{Block, HighlightSpacing, Row, StatefulWidget, TableState, Widget},
};
use tokio::sync::mpsc;

use crate::table::Table;
use crate::{aws, shared::LoadingState};

#[derive(Debug, Clone)]
pub struct LogVieweromponent {
    pub state: Arc<RwLock<LogViewerState>>,
    pub log_group_name: String,
    displayed_messages: Vec<String>,
    table: Table,
}

#[derive(Debug)]
pub struct LogViewerState {
    log_messsages: Vec<String>,
    loading_state: LoadingState,
    group_selection_tx: mpsc::UnboundedSender<LogViewerOutboundMessage>,
}

pub enum LogViewerOutboundMessage {
    ReRender,
    UnselectLogGroup,
    SetLogs(Vec<String>),
}

impl LogVieweromponent {
    pub fn new(log_viewer_tx: mpsc::UnboundedSender<LogViewerOutboundMessage>) -> Self {
        Self {
            state: Arc::new(RwLock::new(LogViewerState {
                log_messsages: vec![],
                loading_state: LoadingState::Loading,
                group_selection_tx: log_viewer_tx,
            })),
            log_group_name: String::new(),
            displayed_messages: vec![],
            table: Table::new(vec![]),
        }
    }
    pub fn run(&self) {
        let this = self.clone(); // clone the widget to pass to the background task
        tokio::spawn(this.fetch_logs());
    }

    async fn fetch_logs(self) {
        self.state.write().unwrap().loading_state = LoadingState::Loading;

        let (outbound_message, loading_state) = match aws::fetch_logs(
            self.log_group_name.clone(),
            chrono::Utc::now().timestamp_millis() - (24 * (3600 * 1000)),
            chrono::Utc::now().timestamp_millis(),
        )
        .await
        {
            Ok(log_messages) => (
                LogViewerOutboundMessage::SetLogs(log_messages),
                LoadingState::Loaded,
            ),
            Err(e) => (LogViewerOutboundMessage::ReRender, LoadingState::Error(e)),
        };

        let mut state = self.state.write().unwrap();
        state.loading_state = loading_state;

        // let lines: Vec<String> = fs::read_to_string("logs")
        //     .unwrap()
        //     .lines()
        //     .map(|line| line.to_string())
        //     .collect();
        state.group_selection_tx.send(outbound_message).unwrap();
    }

    pub fn set_logs(&mut self, log_messages: Vec<String>) {
        self.table.data = log_messages;
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
            (KeyCode::Char('k') | KeyCode::Up, _) => self.table.scroll_up(None),
            (KeyCode::Char('j') | KeyCode::Down, _) => self.table.scroll_down(None),
            (KeyCode::Char('u'), KeyModifiers::CONTROL) => self.table.scroll_up(Some(2000)),
            (KeyCode::Char('d'), KeyModifiers::CONTROL) => self.table.scroll_down(Some(10)),
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

        self.table.render(area, buf);
    }
}
