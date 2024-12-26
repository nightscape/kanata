use eframe::egui;
use std::sync::{Arc, Mutex};
use log::{debug, error, info};

#[derive(Default)]
pub struct OverlayState {
    pub svg_string: Option<String>,
}

pub struct OverlayApp {
    state: Arc<Mutex<OverlayState>>,
}

impl OverlayApp {
    pub fn new(state: Arc<Mutex<OverlayState>>) -> Self {
        info!("Creating new OverlayApp");
        Self { state }
    }
}

impl eframe::App for OverlayApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        info!("OverlayApp update called");
        let state = self.state.lock().unwrap();
        
        info!("Current SVG string: {:?}", state.svg_string);
        
        if let Some(svg) = &state.svg_string {
            info!("Rendering SVG overlay");
            egui::CentralPanel::default().show(ctx, |ui| {
                match egui_extras::load_svg(svg) {
                    Some(image) => {
                        info!("SVG loaded successfully");
                        ui.image(image);
                    }
                    None => {
                        error!("Failed to load SVG");
                    }
                }
            });
        } else {
            info!("No SVG to render");
        }
    }
}

pub fn start_overlay(state: Arc<Mutex<OverlayState>>) {
    info!("Starting overlay window");
    let options = eframe::NativeOptions {
        always_on_top: true,
        decorated: false,
        transparent: true,
        ..Default::default()
    };
    
    info!("Attempting to create native window");
    match eframe::run_native(
        "Kanata Overlay",
        options,
        Box::new(|cc| {
            info!("Creating native window");
            Box::new(OverlayApp::new(state))
        }),
    ) {
        Ok(_) => info!("Overlay window closed"),
        Err(e) => error!("Failed to create overlay window: {}", e),
    }
}
