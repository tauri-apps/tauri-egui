// Copyright 2019-2021 Tauri Programme within The Commons Conservancy
// SPDX-License-Identifier: Apache-2.0
// SPDX-License-Identifier: MIT

#![cfg_attr(
  all(not(debug_assertions), target_os = "windows"),
  windows_subsystem = "windows"
)]

use egui::{FontData, FontDefinitions, FontFamily};
use tauri::{RunEvent, State};
use tauri_egui::{eframe, egui};

use std::sync::mpsc::{channel, Receiver, Sender};

#[tauri::command]
async fn open_native_window(
  egui_handle: State<'_, tauri_egui::EguiPluginHandle>,
) -> Result<String, ()> {
  let (egui_app, rx) = Layout::new();
  let native_options = eframe::NativeOptions {
    resizable: false,
    ..Default::default()
  };

  let _window = egui_handle
    .create_window(
      "native-window".to_string(),
      Box::new(|cc| {
        setup(cc);
        Box::new(egui_app)
      }),
      "Login".into(),
      native_options,
    )
    .unwrap();

  Ok(rx.recv().unwrap_or_else(|_| String::new()))
}

fn setup(cc: &eframe::CreationContext) {
  let mut font = FontDefinitions::default();
  let font_name = "SourceSansPro Regular";
  font.font_data.insert(
    font_name.into(),
    FontData::from_static(include_bytes!("SourceSansPro-Regular.ttf")),
  );
  font
    .families
    .get_mut(&FontFamily::Monospace)
    .unwrap()
    .insert(0, font_name.into());
  font
    .families
    .get_mut(&FontFamily::Proportional)
    .unwrap()
    .insert(0, font_name.into());
  cc.egui_ctx.set_fonts(font);
}

struct Layout {
  input: String,
  tx: Sender<String>,
}

impl Layout {
  pub fn new() -> (Self, Receiver<String>) {
    let (tx, rx) = channel();
    (
      Self {
        input: "".into(),
        tx,
      },
      rx,
    )
  }
}

impl eframe::App for Layout {
  fn update(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
    let Self { input, tx, .. } = self;

    let size = egui::Vec2 { x: 340., y: 100. };

    frame.set_window_size(size);
    egui::CentralPanel::default().show(ctx, |ui| {
      ui.heading("Tauri example");

      let (valid, textfield) = ui
        .horizontal(|ui| {
          let field = ui.add(egui::TextEdit::singleline(input).hint_text("Input"));
          (!input.is_empty(), field)
        })
        .inner;

      let mut button = ui.add_enabled(valid, egui::Button::new("Submit"));
      button.rect.min.x = 100.;
      button.rect.max.x = 100.;
      if (textfield.lost_focus() && ui.input().key_pressed(egui::Key::Enter)) || button.clicked() {
        let _ = tx.send(input.clone());
        input.clear();
        frame.close();
      }
    });
  }
}

fn main() {
  tauri::Builder::default()
    .invoke_handler(tauri::generate_handler![open_native_window])
    .setup(|app| {
      app.wry_plugin(tauri_egui::EguiPluginBuilder::new(app.handle()));
      Ok(())
    })
    .build(tauri::generate_context!("examples/basic/tauri.conf.json"))
    .expect("error while building tauri application")
    .run(|_app, event| {
      if let RunEvent::WindowEvent { label, event, .. } = event {
        println!("{} {:?}", label, event);
      }
    });
}
