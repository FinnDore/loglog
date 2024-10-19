//! # [Ratatui] Async example
//!
//! This example demonstrates how to use Ratatui with widgets that fetch data asynchronously. It
//! uses the `octocrab` crate to fetch a list of pull requests from the GitHub API. You will need an
//! environment variable named `GITHUB_TOKEN` with a valid GitHub personal access token. The token
//! does not need any special permissions.
//!
//! <https://docs.github.com/en/authentication/keeping-your-account-and-data-secure/managing-your-personal-access-tokens#creating-a-fine-grained-personal-access-token>
//! <https://github.com/settings/tokens/new> to create a new token (select classic, and no scopes)
//!
//! This example does not cover message passing between threads, it only demonstrates how to manage
//! shared state between the main thread and a background task, which acts mostly as a one-shot
//! fetcher. For more complex scenarios, you may need to use channels or other synchronization
//! primitives.
//!
//! A simple app might have multiple widgets that fetch data from different sources, and each widget
//! would have its own background task to fetch the data. The main thread would then render the
//! widgets with the latest data.
//!
//! The latest version of this example is available in the [examples] folder in the repository.
//!
//! Please note that the examples are designed to be run against the `main` branch of the Github
//! repository. This means that you may not be able to compile with the latest release version on
//! crates.io, or the one that you have installed locally.
//!
//! See the [examples readme] for more information on finding examples that match the version of the
//! library you are using.
//!
//! [Ratatui]: https://github.com/ratatui/ratatui
//! [examples]: https://github.com/ratatui/ratatui/blob/main/examples
//! [examples readme]: https://github.com/ratatui/ratatui/blob/main/examples/README.md
use std::{
    sync::{Arc, RwLock},
    time::Duration,
};

use color_eyre::{eyre::Context, owo_colors::OwoColorize, Result, Section};
use futures::StreamExt;

use ratatui::{
    buffer::Buffer,
    crossterm::event::{Event, EventStream, KeyCode, KeyEventKind},
    layout::{Constraint, Layout, Rect},
    style::{Style, Stylize},
    text::Line,
    widgets::{Block, HighlightSpacing, Row, StatefulWidget, Table, TableState, Widget},
    DefaultTerminal, Frame,
};

#[tokio::main]
async fn main() -> Result<()> {
    color_eyre::install()?;
    let config = aws_config::load_from_env().await;
    let mut terminal = ratatui::init();

    let app_result = App::default().run(terminal).await;
    ratatui::restore();
    app_result
}

fn init_aws() -> Result<()> {
    Ok(())
}

#[derive(Debug, Default)]
struct App {
    should_quit: bool,
    pull_requests: LogGroupListWidget,
}

impl App {
    const FRAMES_PER_SECOND: f32 = 60.0;

    pub async fn run(mut self, mut terminal: DefaultTerminal) -> Result<()> {
        self.pull_requests.run();

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
        frame.render_widget(&self.pull_requests, body_area);
    }

    fn handle_event(&mut self, event: &Event) {
        if let Event::Key(key) = event {
            if key.kind == KeyEventKind::Press {
                match key.code {
                    KeyCode::Char('q') | KeyCode::Esc => self.should_quit = true,
                    KeyCode::Char('j') | KeyCode::Down => self.pull_requests.scroll_down(),
                    KeyCode::Char('k') | KeyCode::Up => self.pull_requests.scroll_up(),
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
struct LogGroupListWidget {
    state: Arc<RwLock<LogGroupListState>>,
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

impl LogGroupListWidget {
    /// Start fetching the pull requests in the background.
    ///
    /// This method spawns a background task that fetches the pull requests from the GitHub API.
    /// The result of the fetch is then passed to the `on_load` or `on_err` methods.
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
        let mut log_groups = match client.describe_log_groups().send().await {
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
            Err(e) => {
                state.loading_state = LoadingState::Error(e.to_string());
                state.log_groups.clear();
                return;
            }
            Ok(groups) => {
                state.loading_state = LoadingState::Loaded;
                state.log_groups.extend(groups);
                if !state.log_groups.is_empty() {
                    state.table_state.select(Some(0));
                }
            }
        }
    }

    fn scroll_down(&self) {
        self.state.write().unwrap().table_state.scroll_down_by(1);
    }

    fn scroll_up(&self) {
        self.state.write().unwrap().table_state.scroll_up_by(1);
    }
}

impl Widget for &LogGroupListWidget {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let mut state = self.state.write().unwrap();

        // a block with a right aligned title with the loading state on the right
        let loading_state = Line::from(format!("{:?}", state.loading_state)).right_aligned();
        let block = Block::bordered()
            .title("Log Groups")
            .title(loading_state)
            .title_bottom(Line::from("q to quit").right_aligned());

        // a table with the list of pull requests
        let rows = state
            .log_groups
            .iter()
            .map(|log_group| Row::new(vec![log_group.to_string()]));
        let widths = [Constraint::Max(49)];
        let table = Table::new(rows, widths)
            .block(block)
            .highlight_spacing(HighlightSpacing::Always)
            .highlight_symbol(">>")
            .highlight_style(Style::new().red());

        StatefulWidget::render(table, area, buf, &mut state.table_state);
    }
}
