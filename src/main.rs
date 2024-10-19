use std::{
    io,
    sync::{Arc, RwLock},
    time::Duration,
};

use clap::builder::Str;
use crossterm::event::{Event, EventStream, KeyCode, KeyEventKind};
use futures::StreamExt;
use fuzzy_matcher::{skim::SkimMatcherV2, FuzzyMatcher};
use ratatui::{
    crossterm::event::{self},
    style::{Color, Style, Stylize},
    text::{Text, ToText},
    widgets::{List, Paragraph},
    DefaultTerminal,
};
use tokio::time::sleep;

#[tokio::main]
async fn main() {
    let mut terminal = ratatui::init();
    terminal.clear().unwrap();
    let app_result = run(terminal).await.unwrap();
    ratatui::restore();
}

async fn run(mut terminal: DefaultTerminal) -> io::Result<()> {
    let mut is_searching = false;
    let mut search_term = String::new();
    let mut selected_index = 0;
    let mut matches: Vec<(String, i64)> = vec![];
    let matcher = SkimMatcherV2::default();
    let mut logs: Vec<String> = vec![];

    let config = aws_config::load_from_env().await;
    let client = aws_sdk_cloudwatchlogs::Client::new(&config);

    let mut log_selection = LogSelection::new(config).await;

    log_selection
        .run(terminal)
        .await
        .expect("Error running log selection component");
    Ok(())
}

#[derive(Clone)]
struct LogGroupState {
    client: aws_sdk_cloudwatchlogs::Client,
    logs: Vec<String>,
    rest_call_state: RestCallState,
}

struct LogSelection {
    log_group_state: Arc<RwLock<LogGroupState>>,
    search_term: String,
    is_searching: bool,
    selected_index: usize,
    matcher: SkimMatcherV2,
    should_quit: bool,
}

impl LogSelection {
    async fn new(config: aws_config::SdkConfig) -> Self {
        let client = aws_sdk_cloudwatchlogs::Client::new(&config);

        Self {
            search_term: String::new(),
            matcher: SkimMatcherV2::default(),
            should_quit: false,
            log_group_state: Arc::new(RwLock::new(LogGroupState {
                client,
                logs: vec![],
                rest_call_state: RestCallState::LOADING,
            })),
            is_searching: false,
            selected_index: 0,
        }
    }

    async fn run(&mut self, mut terminal: DefaultTerminal) -> io::Result<()> {
        let period = Duration::from_secs_f32(1.0 / 120.);
        let mut interval = tokio::time::interval(period);
        let mut events = EventStream::new();
        tokio::spawn(fetch_log_groups(self.log_group_state.clone()));
        while !self.should_quit {
            tokio::select! {
                _ = interval.tick() => { terminal.draw(|frame| self.draw(frame))?; },
                Some(Ok(event)) = events.next() => self.handle_event(&event).await,
            }
        }

        Ok(())
    }

    async fn draw(&mut self, frame: &mut ratatui::Frame<'_>) {
        let state = self.log_group_state.read().unwrap();
        if state.rest_call_state == RestCallState::LOADING {
            let greeting = Paragraph::new("Loading...");
            frame.render_widget(greeting, frame.area());
        } else if state.rest_call_state == RestCallState::ERROR {
            let greeting = Paragraph::new("Error loading groups");
            frame.render_widget(greeting, frame.area());
        } else {
            let groups = List::new(
                self.log_group_state
                    .read()
                    .unwrap()
                    .logs
                    .clone()
                    .into_iter()
                    .map(|g| Text::raw(g))
                    .collect::<Vec<Text>>(),
            );
            frame.render_widget(groups, frame.area());
        }
    }

    async fn handle_event(&mut self, event: &Event) {
        if let Event::Key(key) = event {
            if key.kind != KeyEventKind::Press {
                return;
            }
            if key.code == KeyCode::Char('/') {
                self.is_searching = !self.is_searching;
            } else if key.code == KeyCode::Char('q') {
                self.should_quit = true;
            }
            //  else if key.code == KeyCode::Down {
            //     self.selected_index = (self.selected_index + 1).min(self.log_groups.len() - 1);
            // } else if key.code == KeyCode::Up {
            //     self.selected_index = (self.selected_index.max(1) - 1).max(0);
            // }
        }
    }
}

async fn fetch_log_groups(state: Arc<RwLock<LogGroupState>>) {
    let mut state = state.write().expect("Failed to lock state");
    let mut log_groups = match state.client.describe_log_groups().send().await {
        Ok(response) => Ok(response
            .log_groups
            .unwrap_or_default()
            .into_iter()
            .map(|group| group.log_group_name)
            .flatten()
            .collect::<Vec<String>>()),
        Err(e) => Err(e),
    };

    match log_groups {
        Ok(groups) => {
            state.logs = groups;
            state.rest_call_state = RestCallState::OK;
        }
        Err(_) => {
            state.logs = vec![];
            state.rest_call_state = RestCallState::ERROR;
        }
    }
}

#[derive(PartialEq, Clone)]
enum RestCallState {
    OK,
    ERROR,
    LOADING,
}
