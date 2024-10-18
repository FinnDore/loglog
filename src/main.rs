use std::io;

use clap::builder::Str;
use fuzzy_matcher::{skim::SkimMatcherV2, FuzzyMatcher};
use ratatui::{
    crossterm::event::{self, KeyCode, KeyEventKind},
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

    let selected_style = Style::default().fg(Color::LightMagenta);
    loop {
        if logs.is_empty() {
            let mut groups = if search_term.is_empty() {
                log_groups
                    .clone()
                    .into_iter()
                    .map(|g| Text::raw(g))
                    .collect::<Vec<Text>>()
            } else {
                matches
                    .clone()
                    .into_iter()
                    .map(|(g, _)| Text::raw(g.clone()))
                    .collect::<Vec<Text>>()
            };
            groups[selected_index] =
                Text::styled(groups[selected_index].to_string(), selected_style.clone());
            groups.push(Text::raw(format!("Searching for '{}'", search_term)));
            groups.rotate_right(1);
            terminal.draw(|frame| {
                let greeting = List::new(groups);
                frame.render_widget(greeting, frame.area());
            })?;
        } else {
            let logs = List::new(
                logs.clone()
                    .into_iter()
                    .map(|log| Text::raw(log))
                    .collect::<Vec<Text>>(),
            );
            terminal.draw(|frame| {
                frame.render_widget(logs, frame.area());
            });
        }
        if let event::Event::Key(key) = event::read()? {
            if key.code == KeyCode::Esc {
                is_searching = false;
                if !logs.is_empty() {
                    logs.clear();
                }
                continue;
            }
            if key.kind != KeyEventKind::Press {
                continue;
            } else if is_searching {
                selected_index = 0;
                if key.code == KeyCode::Backspace {
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
            } else if key.code == KeyCode::Down {
                selected_index = (selected_index + 1).min(log_groups.len() - 1);
            } else if key.code == KeyCode::Up {
                selected_index = (selected_index.max(1) - 1).max(0);
            } else if key.code == KeyCode::Enter {
                let r = match client
                    .start_query()
                    .set_end_time(Some(chrono::Utc::now().timestamp_millis()))
                    .set_start_time(Some(
                        (chrono::Utc::now() - chrono::Duration::days(3000)).timestamp_millis(),
                    ))
                    .set_log_group_name(log_groups[selected_index].clone().into())
                    .set_query_string(Some(
                        "fields @timestamp, @message | sort @timestamp asc".to_owned(),
                    ))
                    .limit(500)
                    .send()
                    .await
                {
                    Ok(response) => response,
                    Err(e) => panic!("Error: {:?}", e),
                };

                sleep(std::time::Duration::from_secs(15)).await;
                logs = match client
                    .get_query_results()
                    .set_query_id(r.query_id)
                    .send()
                    .await
                {
                    Ok(response) => response
                        .results
                        .unwrap_or_default()
                        .into_iter()
                        .flatten()
                        .map(|result| result.value.unwrap_or_default())
                        .collect::<Vec<String>>(),
                    Err(e) => panic!("Error: {:?}", e),
                };
                // .events
                //                     .unwrap_or_default()
                //                     .into_iter()
                //                     .map(|event| event.message.unwrap_or_default())
                //                     .collect::<Vec<String>>(),
            }
        }
    }
}
