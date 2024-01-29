use ratatui::prelude::{Alignment, Constraint, Direction, Layout, Margin, Rect};
use ratatui::text::Text;
use ratatui::widgets::{
    Block, Borders, Clear, Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState,
};
use ratatui::Frame;
use tui_textarea::{Input, Key};

pub struct Popup {
    title: String,
    text: Text<'static>,
    scroll_vert: usize,
    scroll_horiz: usize,
    content_length_vert: usize,
    content_length_horiz: usize,
    pub shown: bool,
}

impl Popup {
    pub fn new() -> Self {
        Self {
            shown: false,
            scroll_vert: 0,
            scroll_horiz: 0,
            content_length_vert: 0,
            content_length_horiz: 0,
            text: Default::default(),
            title: Default::default(),
        }
    }

    pub fn set_title(&mut self, title: impl AsRef<str>) {
        self.title = title.as_ref().to_string();
    }

    pub fn set_text(&mut self, text: Text<'static>) {
        self.text = text;
        self.content_length_horiz = self.text.width();
        self.content_length_vert = self.text.height();
        self.scroll_horiz = 0;
        self.scroll_vert = 0;
    }

    pub fn draw(&mut self, f: &mut Frame) {
        let size = f.size();
        // let chunks = Layout::default()
        //     .constraints([Constraint::Percentage(20), Constraint::Percentage(80)])
        //     .split(size);

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

        let paragraph = Paragraph::new(self.text.clone())
            .alignment(Alignment::Left)
            .scroll((self.scroll_vert as u16, self.scroll_horiz as u16));

        f.render_widget(paragraph, rect);
        if self.content_length_vert as u16 > rect.height {
            let scrollbar_vert = Scrollbar::default()
                .orientation(ScrollbarOrientation::VerticalRight)
                .symbols(ratatui::symbols::scrollbar::VERTICAL);

            let mut state_vert = ScrollbarState::default()
                .content_length(self.content_length_vert)
                .position(self.scroll_vert);

            f.render_stateful_widget(
                scrollbar_vert,
                area.inner(&Margin {
                    horizontal: 0,
                    vertical: 1,
                }),
                &mut state_vert,
            );
        } else {
            self.scroll_vert = 0;
        }

        if self.content_length_horiz as u16 > rect.width {
            let scrollbar_horiz = Scrollbar::default()
                .orientation(ScrollbarOrientation::HorizontalBottom)
                .symbols(ratatui::symbols::scrollbar::HORIZONTAL)
                .thumb_symbol("■");

            let mut state_horiz = ScrollbarState::default()
                .content_length(self.content_length_horiz)
                .position(self.scroll_horiz);

            f.render_stateful_widget(
                scrollbar_horiz,
                area.inner(&Margin {
                    horizontal: 1,
                    vertical: 0,
                }),
                &mut state_horiz,
            );
        } else {
            self.scroll_horiz = 0;
        }
    }

    pub fn on_input(&mut self, input: Input) {
        match input.key {
            Key::Up => self.scroll_vert = self.scroll_vert.saturating_sub(1),
            Key::PageDown => self.scroll_vert = self.content_length_vert.saturating_sub(1),
            Key::PageUp => self.scroll_vert = 0,
            Key::Left => self.scroll_horiz = self.scroll_horiz.saturating_sub(1),

            Key::Right => {
                self.scroll_horiz = self
                    .scroll_horiz
                    .saturating_add(1)
                    .clamp(0, self.content_length_horiz.saturating_sub(1));
            }

            Key::Down => {
                self.scroll_vert = self
                    .scroll_vert
                    .saturating_add(1)
                    .clamp(0, self.content_length_vert.saturating_sub(1))
            }

            Key::Char('q') | Key::Enter => {
                self.shown = false;
            }

            _ => {}
        }
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
