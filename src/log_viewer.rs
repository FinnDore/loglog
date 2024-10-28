use std::iter::Enumerate;
use std::slice::Iter;
use std::sync::{Arc, RwLock};

use aws_sdk_cloudwatchlogs::types::QueryStatus;
use clap::builder::Str;
use color_eyre::owo_colors::OwoColorize;
use crossterm::event::{Event, KeyCode, KeyModifiers};
use rat_ftable::selection::rowselection;
use rat_ftable::textdata::Row;
use rat_ftable::TableDataIter;
use rat_ftable::{
    selection::{NoSelection, RowSelection},
    Table, TableData, TableState,
};
use ratatui::text::Span;
use ratatui::widgets::Borders;
use ratatui::{
    buffer::Buffer,
    layout::{Constraint, Rect},
    style::{Color, Style},
    text::{Line, Text},
    widgets::{Block, HighlightSpacing, StatefulWidget, Widget},
};
use tokio::sync::mpsc;

use crate::shared::{LoadingState, ONE_HOUR_MS};

#[derive(Debug)]
pub struct LogViewerComponent<'a> {
    pub state: Arc<RwLock<LogViewerState>>,
    pub log_group_name: String,
    displayed_messages: Vec<String>,
    table: Table<'a, RowSelection>,
    table_state: TableState<RowSelection>,
    scrolled: bool,
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
}

impl<'a> LogViewerComponent<'a> {
    pub fn new(group_selection_tx: mpsc::UnboundedSender<LogViewerOutboundMessage>) -> Self {
        let mut table_state = TableState::default();
        table_state.set_scroll_selection(true);
        Self {
            state: Arc::new(RwLock::new(LogViewerState {
                log_messsages: vec![],
                loading_state: LoadingState::Loading,
                group_selection_tx,
            })),
            table: Table::new(),
            log_group_name: String::new(),
            displayed_messages: vec![],
            table_state,
            scrolled: false,
        }
    }

    pub fn run(&self) {
        let state = self.state.clone();
        tokio::spawn(LogViewerComponent::fetch_logs(
            state,
            self.log_group_name.clone(),
        ));
    }

    async fn fetch_logs(state: Arc<RwLock<LogViewerState>>, log_group_name: String) {
        state.write().unwrap().loading_state = LoadingState::Loading;

        let config = aws_config::load_from_env().await;
        let client = aws_sdk_cloudwatchlogs::Client::new(&config);
        let query_id = match client
            .start_query()
            .set_start_time(Some(
                chrono::Utc::now().timestamp_millis() - (ONE_HOUR_MS * 48),
            ))
            .set_end_time(Some(chrono::Utc::now().timestamp_millis()))
            .set_query_string(Some("fields @message".into()))
            .set_log_group_name(log_group_name.into())
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
                        state.write().unwrap();
                    state.log_messsages = response
                        .results
                        .unwrap_or_default()
                        .into_iter()
                        .flatten()
                        .filter(|result| result.field == Some("@message".to_string()))
                        .map(|message| message.value.unwrap_or_default())
                        .rev()
                        .collect::<Vec<String>>();

                    match response.status {
                        Some(QueryStatus::Complete) => {
                            state.loading_state = LoadingState::Loaded;
                            if !state.log_messsages.is_empty() {
                                let num_of_messages = state.log_messsages.len() - 1;
                                // TODO
                                // state.table_state.select(Some(num_of_messages));
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

    fn scroll_down(&mut self, amount: Option<usize>) {
        self.table_state.scroll_down(amount.unwrap_or(1));
    }

    fn scroll_up(&mut self, amount: Option<usize>) {
        self.table_state.scroll_up(amount.unwrap_or(1));
    }

    pub fn set_logs(&mut self) {
        let state = self.state.read().unwrap();
        self.displayed_messages = state
            .log_messsages
            .clone()
            .iter()
            .map(|message| message.clone())
            .collect();
        println!("{}", self.displayed_messages.len());
        self.table_state
            .scroll_to_row(self.displayed_messages.len().saturating_sub(1));
    }

    pub fn clear_logs(&mut self) {
        let mut state = self.state.write().unwrap();
        state.log_messsages.clear();
        self.displayed_messages = vec![];
    }

    pub fn handle_event(&mut self, event: &Event) -> bool {
        // let _ = rowselection::handle_events(&mut self.table_state, true, event);
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

impl<'a> LogViewerComponent<'a> {
    pub fn render(&mut self, area: Rect, buf: &mut Buffer) {
        let state = self.state.write().unwrap();
        let loading_state = Line::from(format!("{:?}", state.loading_state)).right_aligned();

        let block = Block::new()
            .borders(Borders::BOTTOM | Borders::TOP)
            .title(self.log_group_name.to_string())
            .title(loading_state)
            .title_bottom(Line::from("q to quit").right_aligned());

        Table::default()
            .iter(DataIter {
                size: self.displayed_messages.len(),
                iter: self.displayed_messages.iter().enumerate(),
                item: None,
            })
            .select_row_style(Some(Style::new()))
            .block(block)
            .render(area, buf, &mut self.table_state);
    }
}

struct DataIter<'a> {
    size: usize,
    iter: Enumerate<Iter<'a, String>>,
    item: Option<(usize, &'a String)>,
}

impl<'a> TableDataIter<'a> for DataIter<'a> {
    fn widths(&self) -> Vec<Constraint> {
        vec![Constraint::Percentage(100)]
    }

    fn rows(&self) -> Option<usize> {
        Some(self.size)
    }

    fn nth(&mut self, n: usize) -> bool {
        self.item = self.iter.nth(n);
        self.item.is_some()
    }

    fn render_cell(
        &self,
        ctx: &rat_ftable::TableContext,
        column: usize,
        area: Rect,
        buf: &mut Buffer,
    ) {
        let item = self.item.expect("item should be set");
        let style = match ctx.selected_row {
            true => Style::new(),
            false => Style::new(),
        };

        let text = Span::styled(item.1, style);
        text.render(area, buf);
    }
}
