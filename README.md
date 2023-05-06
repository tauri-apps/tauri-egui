# tauri-egui

[![status](https://img.shields.io/badge/status-stable-blue.svg)](https://github.com/tauri-apps/tauri-egui/tree/dev)
[![License](https://img.shields.io/badge/License-MIT%20or%20Apache%202-green.svg)](https://opencollective.com/tauri)
[![test library](https://img.shields.io/github/workflow/status/tauri-apps/tauri-egui/test%20library?label=test%20library)](https://github.com/tauri-apps/tauri/actions?query=workflow%3A%22test+library%22)
[![Chat Server](https://img.shields.io/badge/chat-discord-7289da.svg)](https://discord.gg/SpmNs4S)
[![website](https://img.shields.io/badge/website-tauri.app-purple.svg)](https://tauri.app)
[![https://good-labs.github.io/greater-good-affirmation/assets/images/badge.svg](https://good-labs.github.io/greater-good-affirmation/assets/images/badge.svg)](https://good-labs.github.io/greater-good-affirmation)
[![support](https://img.shields.io/badge/sponsor-Open%20Collective-blue.svg)](https://opencollective.com/tauri)

## Dependency

| Component                                                                                    | Description                               | Version                                                                                                  | Lin | Win | Mac |
| -------------------------------------------------------------------------------------------- | ----------------------------------------- | -------------------------------------------------------------------------------------------------------- | --- | --- | --- |
| [**tauri**](https://github.com/tauri-apps/tauri/tree/dev/core/tauri)                         | runtime core                              | [![](https://img.shields.io/badge/crate.io-V2.0.0-alpha.4-orange)](https://crates.io/crates/tauri)                         | ✅  | ✅  | ✅  |
| [**tauri-runtime**](https://github.com/tauri-apps/tauri/tree/dev/core/tauri-runtime)         | layer between Tauri and webview libraries | [![](https://img.shields.io/crates/v/tauri-runtime.svg)](https://crates.io/crates/tauri-runtime)         | ✅  | ✅  | ✅  |
| [**tauri-runtime-wry**](https://github.com/tauri-apps/tauri/tree/dev/core/tauri-runtime-wry) | enables system-level interaction via WRY  | [![](https://img.shields.io/crates/v/tauri-runtime-wry.svg)](https://crates.io/crates/tauri-runtime-wry) | ✅  | ✅  | ✅  |

## About tauri-egui

`tauri-egui` is a Tauri plugin for using the [`egui library`](https://github.com/emilk/egui) in a Tauri application via [`glutin`](https://github.com/tauri-apps/glutin). `egui` is a pure Rust GUI library that runs natively, recommended by the Tauri team for secure contexts such as password and secret interfaces.

## Example

```rust
use tauri::Manager;
use tauri_egui::{eframe, egui, EguiPluginBuilder, EguiPluginHandle};

struct LoginApp {
  password: String,
  on_submit: Box<dyn Fn(&str) -> bool + Send>,
}

impl LoginApp {
  fn new<F: Fn(&str) -> bool + Send + 'static>(
    ctx: &eframe::CreationContext,
    on_submit: F,
  ) -> Self {
    Self {
      password: "".into(),
      on_submit: Box::new(on_submit),
    }
  }
}

impl eframe::App for LoginApp {
  fn update(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
    let LoginApp {
      password,
      on_submit,
    } = self;
    egui::CentralPanel::default().show(ctx, |ui| {
      ui.label("Enter your password");
      let textfield = ui.add_sized(
        [ui.available_width(), 24.],
        egui::TextEdit::singleline(password).password(true),
      );
      let button = ui.button("Submit");
      if (textfield.lost_focus() && ui.input().key_pressed(egui::Key::Enter)) || button.clicked() {
        if on_submit(password) {
          frame.close();
        }
      }
    });
  }
}

fn main() {
  tauri::Builder::default()
    .setup(|app| {
      app.wry_plugin(EguiPluginBuilder::new(app.handle()));
      let egui_handle = app.state::<EguiPluginHandle>();

      let native_options = eframe::NativeOptions {
        drag_and_drop_support: true,
        initial_window_size: Some([1280.0, 1024.0].into()),
        ..Default::default()
      };

      let _window = egui_handle
        .create_window(
          "native-window".to_string(),
          Box::new(|cc| Box::new(LoginApp::new(cc, |pwd| pwd == "tauriisawesome"))),
          "Login".into(),
          native_options,
        )
        .unwrap();

      Ok(())
    })
    .run(tauri::generate_context!("examples/demo/tauri.conf.json"))
    .expect("error while building tauri application");
}
```

## Semver

**tauri-egui** is following [Semantic Versioning 2.0](https://semver.org/).

## Licenses

Code: (c) 2019 - 2022 - The Tauri Programme within The Commons Conservancy.

MIT or MIT/Apache 2.0 where applicable.
