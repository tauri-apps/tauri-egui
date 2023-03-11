// Copyright 2019-2021 Tauri Programme within The Commons Conservancy
// SPDX-License-Identifier: Apache-2.0
// SPDX-License-Identifier: MIT

#![cfg_attr(
  all(not(debug_assertions), target_os = "windows"),
  windows_subsystem = "windows"
)]

use tauri::{RunEvent, State};
use tauri_egui::eframe;

#[tauri::command]
async fn open_native_window(
  egui_handle: State<'_, tauri_egui::EguiPluginHandle>,
) -> Result<(), ()> {
  //let (egui_app, rx) = Layout::new();
  let native_options = eframe::NativeOptions {
    drag_and_drop_support: true,
    initial_window_size: Some([1280.0, 1024.0].into()),
    ..Default::default()
  };

  let _window = egui_handle
    .create_window(
      "native-window".to_string(),
      Box::new(|cc| Box::new(egui_demo_app::WrapApp::new(cc))),
      "Login".into(),
      native_options,
    )
    .unwrap();

  Ok(())
}

fn main() {
  let mut app = tauri::Builder::default()
    .invoke_handler(tauri::generate_handler![open_native_window])
    .build(tauri::generate_context!("examples/demo/tauri.conf.json"))
    .expect("error while building tauri application");
  app.wry_plugin(tauri_egui::EguiPluginBuilder::new(app.handle()));

  app.run(|_app, event| {
    if let RunEvent::WindowEvent { label, event, .. } = event {
      println!("{} {:?}", label, event);
    }
  });
}
