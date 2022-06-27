// Copyright 2019-2021 Tauri Programme within The Commons Conservancy
// SPDX-License-Identifier: Apache-2.0
// SPDX-License-Identifier: MIT

use std::sync::mpsc::sync_channel;

use tauri::{AppHandle, Manager, Runtime};
use tauri_runtime::UserEvent;
use tauri_runtime_wry::{Context, PluginBuilder};

pub use egui;
pub use epi;

mod plugin;
use plugin::EguiPlugin;
pub use plugin::EguiPluginHandle;

pub struct EguiPluginBuilder<R: Runtime> {
  app: AppHandle<R>,
}

impl<R: Runtime> EguiPluginBuilder<R> {
  pub fn new(app: AppHandle<R>) -> Self {
    Self { app }
  }
}

impl<T: UserEvent, R: Runtime> PluginBuilder<T> for EguiPluginBuilder<R> {
  type Plugin = EguiPlugin<T>;

  fn build(self, context: Context<T>) -> Self::Plugin {
    let plugin = EguiPlugin {
      context: plugin::Context {
        inner: context,
        main_thread: plugin::MainThreadContext {
          windows: Default::default(),
        },
      },
      create_window_channel: sync_channel(1),
      is_focused: false,
    };
    self.app.manage(plugin.handle());
    plugin
  }
}
