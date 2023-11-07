use ratatui::backend::Backend;
use ratatui::prelude::{Alignment, Constraint, Direction, Layout, Rect, Stylize};
use ratatui::widgets::{Block, Borders, Clear, Paragraph, Wrap};
use ratatui::Frame;

pub struct Popup {
    title: String,
    text: String,
    pub shown: bool,
}

impl Popup {
    pub fn new() -> Self {
        Self {
            shown: false,
            text: Default::default(),
            title: Default::default(),
        }
    }

    pub fn set_title(&mut self, title: impl AsRef<str>) {
        self.title = title.as_ref().to_string();
    }

    pub fn set_text(&mut self, text: impl AsRef<str>) {
        self.text = text.as_ref().to_string();
    }

    pub fn draw<B: Backend>(&self, f: &mut Frame<B>) {
        let size = f.size();
        let chunks = Layout::default()
            .constraints([Constraint::Percentage(20), Constraint::Percentage(80)])
            .split(size);

        let block = Block::default()
            .title(self.title.as_str())
            .borders(Borders::ALL);
        let area = centered_rect(70, 70, size);

        f.render_widget(Clear, area); //this clears out the background
        f.render_widget(block, area);

        let rect = Layout::default()
            .margin(2)
            .constraints([Constraint::Percentage(100)])
            .direction(Direction::Horizontal)
            .split(area)[0];

        let paragraph = Paragraph::new(self.text.as_str())
            .alignment(Alignment::Left)
            .wrap(Wrap { trim: true });

        f.render_widget(paragraph, rect);
    }
}

/// helper function to create a centered rect using up certain percentage of the available rect `r`
fn centered_rect(percent_x: u16, percent_y: u16, r: Rect) -> Rect {
    let popup_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(r);

    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(popup_layout[1])[1]
}
