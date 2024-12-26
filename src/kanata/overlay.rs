use eframe::egui;
use std::sync::Arc;
use parking_lot::Mutex;
use log::error;
use std::sync::mpsc::{channel, Sender, Receiver};

#[derive(Default)]
pub struct OverlayState {
    pub svg_string: Option<String>,
}

pub struct OverlayApp {
    state: Arc<Mutex<OverlayState>>,
}

impl OverlayApp {
    pub fn new(state: Arc<Mutex<OverlayState>>) -> Self {
        Self { state }
    }
}

impl eframe::App for OverlayApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let state = self.state.lock();

        if let Some(svg) = &state.svg_string {
            egui::CentralPanel::default().show(ctx, |ui| {
                if let Ok(image) = egui_extras::image::load_svg_bytes(svg.as_bytes()) {
                    let texture = ctx.load_texture("svg_image", image, Default::default());
                    ui.image(&texture);
                }
            });
        }
    }
}

pub enum OverlayMessage {
    UpdateSvg(String),
    Close,
}

pub fn initialize_overlay() -> Sender<OverlayMessage> {
    let (tx, rx) = channel();
    let tx_clone = tx.clone();

    // Store the channel sender somewhere accessible to your application

    // Start the UI on the main thread
    std::thread::spawn(move || {
        let state = Arc::new(Mutex::new(OverlayState::default()));
        let state_clone = state.clone();

        // Create a thread to handle messages
        std::thread::spawn(move || {
            while let Ok(msg) = rx.recv() {
                match msg {
                    OverlayMessage::UpdateSvg(svg) => {
                        let mut state = state_clone.lock();
                        state.svg_string = Some(svg);
                    }
                    OverlayMessage::Close => break,
                }
            }
        });

        let options = eframe::NativeOptions {
            viewport: egui::ViewportBuilder::default()
                .with_always_on_top()
                .with_decorations(false)
                .with_transparent(true),
            renderer: eframe::Renderer::default(),
            vsync: true,
            ..Default::default()
        };

        if let Err(e) = eframe::run_native(
            "Kanata Overlay",
            options,
            Box::new(|_cc| Ok(Box::new(OverlayApp::new(state)))),
        ) {
            error!("Overlay error: {}", e);
        }
    });

    tx_clone
}
