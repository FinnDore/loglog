use color_eyre::Result;
use futures::StreamExt;

use log_groups::{LogGroupListComponent, LogGroupSelectionOutboundMessage};
use log_viewer::{LogViewerComponent, LogViewerOutboundMessage};
use ratatui::{
    crossterm::event::{Event, EventStream, KeyCode, KeyEventKind},
    widgets::Widget,
    DefaultTerminal, Frame,
};
use tokio::sync::mpsc;

mod log_groups;
mod log_viewer;
mod shared;

#[tokio::main]
async fn main() -> Result<()> {
    color_eyre::install()?;
    let terminal = ratatui::init();

    let mut app = App::new();
    let app_result = app.run(terminal).await;
    ratatui::restore();
    app_result
}

#[derive(Debug)]
struct App<'a> {
    should_quit: bool,
    selected_group: Option<String>,
    log_groups_component: LogGroupListComponent,
    log_group_selection_rx: mpsc::UnboundedReceiver<LogGroupSelectionOutboundMessage>,
    log_viewer_component: LogViewerComponent<'a>,
    log_viewer_rx: mpsc::UnboundedReceiver<LogViewerOutboundMessage>,
}

impl<'a> App<'a> {
    pub async fn run(&mut self, mut terminal: DefaultTerminal) -> Result<()> {
        self.log_groups_component.run();

        let mut events = EventStream::new();

        while !self.should_quit {
            terminal.draw(|frame| self.draw(frame))?;
            tokio::select! {
                event = self.log_group_selection_rx.recv() => {
                    match event {
                        Some(LogGroupSelectionOutboundMessage::SelectedGroup(group)) => {
                            self.selected_group = Some(group.clone());
                            self.log_viewer_component.log_group_name = group;
                            // TODO handle reselecvtion and stuff
                            self.log_viewer_component.run()
                        },
                        None => (),
                        Some(LogGroupSelectionOutboundMessage::ApplySearch) => {
                            self.log_groups_component.apply_search();
                        }

                    }
                },
                 event = self.log_viewer_rx.recv() => {
                    match event {
                        None => (),
                        Some(LogViewerOutboundMessage::ReRender) => {
                            self.log_viewer_component.set_logs();
                            terminal.draw(|frame| self.draw(frame))?;
                        }
                        Some(LogViewerOutboundMessage::UnselectLogGroup) => {
                            self.selected_group = None;
                            self.log_viewer_component.clear_logs();
                            self.log_viewer_component.log_group_name.clear();
                        }
                    }
                },
                Some(Ok(event)) = events.next() => self.handle_event(&event),
            }
        }
        Ok(())
    }

    fn draw(&mut self, frame: &mut Frame) {
        if self.selected_group.is_some() {
            &self
                .log_viewer_component
                .render(frame.area(), frame.buffer_mut());
        } else {
            frame.render_widget(&self.log_groups_component, frame.area());
        }
    }

    fn handle_event(&mut self, event: &Event) {
        let prevent_exit = if self.selected_group.is_some() {
            self.log_viewer_component.handle_event(event)
        } else {
            self.log_groups_component.handle_event(event)
        };
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

impl<'a> App<'a> {
    fn new() -> Self {
        let (tx, rx) = mpsc::unbounded_channel::<LogGroupSelectionOutboundMessage>();
        let (log_viewer_tx, log_viewer_rx) = mpsc::unbounded_channel::<LogViewerOutboundMessage>();
        Self {
            should_quit: false,
            selected_group: None,
            log_groups_component: LogGroupListComponent::new(tx),
            log_viewer_component: LogViewerComponent::new(log_viewer_tx),
            log_viewer_rx,
            log_group_selection_rx: rx,
        }
    }
}
