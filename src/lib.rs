// Copyright 2019-2021 Tauri Programme within The Commons Conservancy
// SPDX-License-Identifier: Apache-2.0
// SPDX-License-Identifier: MIT

use std::{sync::mpsc::sync_channel, time::Instant};

// use glutin::{ContextError, CreationError};
use tauri::{AppHandle, Manager, Runtime};
use tauri_runtime::UserEvent;
use tauri_runtime_wry::{Context, PluginBuilder};

pub use eframe;
pub use egui;

mod plugin;
use plugin::EguiPlugin;
pub use plugin::EguiPluginHandle;

// pub type Window = plugin::Window<tauri::EventLoopMessage>;

#[derive(Debug, thiserror::Error)]
pub enum Error {
  // #[error("failed to create window: {0}")]
  // FailedToCreateWindow(#[from] CreationError),
  #[error("glutin error: {0}")]
  Glutin(#[from] glutin::error::Error),
  #[error("Found no glutin configs matching the template")]
  NoGlutinConfigs,
  #[error("failed to create painter: {0}")]
  FailedToCreatePainter(String),
}

pub type Result<T> = std::result::Result<T, Error>;

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
      context,
      create_window_channel: sync_channel(1),
      running: None,
      is_focused: false,
      next_repaint_time: Instant::now(),
    };
    self.app.manage(plugin.handle());
    plugin
  }
}
