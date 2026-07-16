use image::{DynamicImage, ImageBuffer, Rgba};
use ratatui::{Frame, layout::Rect};
use ratatui_image::{Resize, StatefulImage, picker::Picker, protocol::StatefulProtocol};
pub struct ArtworkState {
    picker: Picker,
    protocol: Option<StatefulProtocol>,
}

impl ArtworkState {
    pub fn detect() -> Self {
        let picker = Picker::from_query_stdio().unwrap_or_else(|_| Picker::halfblocks());
        let mut artwork = Self {
            picker,
            protocol: None,
        };
        artwork.set_image(Self::placeholder());
        artwork
    }

    pub fn halfblocks() -> Self {
        let picker = Picker::halfblocks();
        let mut artwork = Self {
            picker,
            protocol: None,
        };
        artwork.set_image(Self::placeholder());
        artwork
    }

    pub fn protocol_name(&self) -> String {
        format!("{:?}", self.picker.protocol_type())
    }

    pub fn load(&mut self, bytes: &[u8]) -> Result<(), image::ImageError> {
        self.set_image(image::load_from_memory(bytes)?);
        Ok(())
    }

    pub fn render(&mut self, frame: &mut Frame<'_>, area: Rect) {
        if let Some(protocol) = self.protocol.as_mut() {
            frame.render_stateful_widget(
                StatefulImage::default().resize(Resize::Fit(None)),
                area,
                protocol,
            );
        }
    }

    fn set_image(&mut self, image: DynamicImage) {
        self.protocol = Some(self.picker.new_resize_protocol(image));
    }

    fn placeholder() -> DynamicImage {
        let image = ImageBuffer::from_fn(256, 256, |x, y| {
            let diagonal = ((x + y) / 2) as u8;
            let pulse = ((x.abs_diff(y) / 2).min(80)) as u8;
            Rgba([0, 255_u8.saturating_sub(pulse), diagonal, 255])
        });
        DynamicImage::ImageRgba8(image)
    }
}
