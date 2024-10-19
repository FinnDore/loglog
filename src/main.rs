use std::{
    sync::{Arc, RwLock},
    time::Duration,
};

use color_eyre::{eyre::Context, owo_colors::OwoColorize, Result, Section};
use crossterm::style::Stylize;
use futures::StreamExt;

use fuzzy_matcher::{skim::SkimMatcherV2, FuzzyMatcher};
use ratatui::{
    buffer::Buffer,
    crossterm::event::{Event, EventStream, KeyCode, KeyEventKind},
    layout::{Constraint, Layout, Rect},
    style::{Color, Style},
    text::Line,
    widgets::{Block, HighlightSpacing, Row, StatefulWidget, Table, TableState, Widget},
    DefaultTerminal, Frame,
};

#[tokio::main]
async fn main() -> Result<()> {
    color_eyre::install()?;
    let config = aws_config::load_from_env().await;
    let mut terminal = ratatui::init();

    let app = App::default();
    let app_result = app.run(terminal).await;
    ratatui::restore();
    app_result
}

fn init_aws() -> Result<()> {
    Ok(())
}

#[derive(Debug, Default)]
struct App {
    should_quit: bool,
    log_groups_component: LogGroupListComponent,
}

impl App {
    const FRAMES_PER_SECOND: f32 = 60.0;

    pub async fn run(mut self, mut terminal: DefaultTerminal) -> Result<()> {
        self.log_groups_component.run();

        let period = Duration::from_secs_f32(1.0 / Self::FRAMES_PER_SECOND);
        let mut interval = tokio::time::interval(period);
        let mut events = EventStream::new();

        while !self.should_quit {
            tokio::select! {
                _ = interval.tick() => { terminal.draw(|frame| self.draw(frame))?; },
                Some(Ok(event)) = events.next() => self.handle_event(&event),
            }
        }
        Ok(())
    }

    fn draw(&self, frame: &mut Frame) {
        let vertical = Layout::vertical([Constraint::Length(1), Constraint::Fill(1)]);
        let [title_area, body_area] = vertical.areas(frame.area());
        frame.render_widget(&self.log_groups_component, body_area);
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

/// A widget that displays a list of pull requests.
///
/// This is an async widget that fetches the list of pull requests from the GitHub API. It contains
/// an inner `Arc<RwLock<PullRequestListState>>` that holds the state of the widget. Cloning the
/// widget will clone the Arc, so you can pass it around to other threads, and this is used to spawn
/// a background task to fetch the pull requests.
#[derive(Debug, Clone, Default)]
struct LogGroupListComponent {
    state: Arc<RwLock<LogGroupListState>>,
    sorted_log_groups: Vec<String>,
    search_term: String,
    is_searching: bool,
}

#[derive(Debug, Default)]
struct LogGroupListState {
    log_groups: Vec<String>,
    loading_state: LoadingState,
    table_state: TableState,
}

#[derive(Debug, Clone)]
struct PullRequest {
    id: String,
    title: String,
    url: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
enum LoadingState {
    #[default]
    Idle,
    Loading,
    Loaded,
    Error(String),
}

impl LogGroupListComponent {
    fn run(&self) {
        let this = self.clone(); // clone the widget to pass to the background task
        tokio::spawn(this.fetch_pulls());
    }

    async fn fetch_pulls(self) {
        // this runs once, but you could also run this in a loop, using a channel that accepts
        // messages to refresh on demand, or with an interval timer to refresh every N seconds

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
                state.log_groups.extend(groups);
                if !state.log_groups.is_empty() {
                    state.table_state.select_first();
                }
            }
            Err(e) => {
                state.loading_state = LoadingState::Error(e.to_string());
                state.log_groups.clear();
                return;
            }
        }
    }

    fn scroll_down(&self) {
        self.state.write().unwrap().table_state.scroll_down_by(1);
    }

    fn scroll_up(&self) {
        self.state.write().unwrap().table_state.scroll_up_by(1);
    }

    fn apply_search(&mut self) {
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

    fn handle_event(&mut self, event: &Event) -> bool {
        if let Event::Key(key) = event {
            if key.kind == KeyEventKind::Press {
                match key.code {
                    KeyCode::Down => self.scroll_down(),
                    KeyCode::Up => self.scroll_up(),
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
