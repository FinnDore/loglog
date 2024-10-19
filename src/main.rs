use std::{
    sync::{Arc, RwLock},
    time::Duration,
};

use color_eyre::{eyre::Context, owo_colors::OwoColorize, Result, Section};
use crossterm::style::Stylize;
use futures::StreamExt;

use fuzzy_matcher::{skim::SkimMatcherV2, FuzzyMatcher};
use log_groups::{LogGroupListComponent, LogGroupListState, LogGroupSelectionOutboundMessage};
use ratatui::{
    buffer::Buffer,
    crossterm::event::{Event, EventStream, KeyCode, KeyEventKind},
    layout::{Constraint, Layout, Rect},
    style::{Color, Style},
    text::Line,
    widgets::{Block, HighlightSpacing, Row, StatefulWidget, Table, TableState, Widget},
    DefaultTerminal, Frame,
};
use tokio::sync::mpsc;

mod log_groups;

#[tokio::main]
async fn main() -> Result<()> {
    color_eyre::install()?;
    let terminal = ratatui::init();

    let app = App::new();
    let app_result = app.run(terminal).await;
    ratatui::restore();
    app_result
}

#[derive(Debug)]
struct App {
    should_quit: bool,
    selected_group: Option<String>,
    log_groups_component: LogGroupListComponent,
    log_group_selection_rx: mpsc::UnboundedReceiver<LogGroupSelectionOutboundMessage>,
}

impl App {
    pub async fn run(mut self, mut terminal: DefaultTerminal) -> Result<()> {
        self.log_groups_component.run();

        let mut events = EventStream::new();

        while !self.should_quit {
            terminal.draw(|frame| self.draw(frame))?;
            tokio::select! {
                event = self.log_group_selection_rx.recv() => {
                    match event {
                        Some(LogGroupSelectionOutboundMessage::SelectedGroup(group)) => {
                            self.selected_group = Some(group);
                        },
                        None => (),
                        Some(LogGroupSelectionOutboundMessage::ApplySearch) => {
                            self.log_groups_component.apply_search();
                        }

                    }
                },
                Some(Ok(event)) = events.next() => self.handle_event(&event),
            }
        }
        Ok(())
    }

    fn draw(&self, frame: &mut Frame) {
        frame.render_widget(&self.log_groups_component, frame.area());
    }

    fn handle_event(&mut self, event: &Event) {
        let prevent_exit = self.log_groups_component.handle_event(event);
        if let Event::Key(key) = event {
            if key.kind == KeyEventKind::Press {
                match key.code {
                    KeyCode::Char('q') | KeyCode::Esc => {
                        if !prevent_exit {
                            self.should_quit = true
                        }
                    }
                    _ => {}
                }
            }
        }
    }
}

impl App {
    fn new() -> Self {
        let (tx, rx) = mpsc::unbounded_channel::<LogGroupSelectionOutboundMessage>();
        Self {
            log_group_selection_rx: rx,
            should_quit: false,
            selected_group: None,
            log_groups_component: LogGroupListComponent::new(tx),
        }
    }
}
