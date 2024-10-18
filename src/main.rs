use std::io;

use fuzzy_matcher::{skim::SkimMatcherV2, FuzzyMatcher};
use ratatui::{
    crossterm::event::{self, KeyCode, KeyEventKind},
    style::Stylize,
    widgets::Paragraph,
    DefaultTerminal,
};

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
    let mut matches: Vec<(String, i64)> = vec![];
    let matcher = SkimMatcherV2::default();

    let config = aws_config::load_from_env().await;
    let client = aws_sdk_cloudwatchlogs::Client::new(&config);

    let mut log_groups: Vec<String> = match client.describe_log_groups().send().await {
        Ok(response) => response
            .log_groups
            .unwrap_or_default()
            .into_iter()
            .map(|group| group.log_group_name)
            .flatten()
            .collect(),
        Err(e) => panic!("Error: {:?}", e),
    };

    loop {
        let groups = if search_term.is_empty() {
            log_groups.clone()
        } else {
            matches
                .clone()
                .into_iter()
                .map(|g| format!("{} ({})", g.0, g.1))
                .collect::<Vec<String>>()
        };
        terminal.draw(|frame| {
            let greeting = Paragraph::new(format!("{} {:?}", search_term, groups));
            frame.render_widget(greeting, frame.area());
        })?;

        if let event::Event::Key(key) = event::read()? {
            if key.kind != KeyEventKind::Press {
                continue;
            } else if is_searching {
                if key.code == KeyCode::Esc {
                    is_searching = false;
                    continue;
                } else if key.code == KeyCode::Backspace {
                    search_term.pop();
                } else {
                    search_term.push(key.code.to_string().chars().next().unwrap());
                };
                matches = log_groups
                    .clone()
                    .into_iter()
                    .map(|group| (group.clone(), matcher.fuzzy_match(&group, &search_term)))
                    .filter(|(_, score)| match score {
                        Some(score) => score > &5,
                        None => false,
                    })
                    .map(|(group, score)| (group, score.unwrap_or_default()))
                    .collect::<Vec<_>>();
            } else if key.code == KeyCode::Char('/') {
                is_searching = !is_searching;
            } else if key.code == KeyCode::Char('q') {
                return Ok(());
            }
        }
    }
}
