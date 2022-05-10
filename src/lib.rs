// Copyright 2019-2021 Tauri Programme within The Commons Conservancy
// SPDX-License-Identifier: Apache-2.0
// SPDX-License-Identifier: MIT

use std::{
  collections::HashMap,
  sync::{
    mpsc::{sync_channel, Receiver, SyncSender},
    Arc, Mutex,
  },
};

use tauri::{AppHandle, Runtime};
use tauri_runtime::UserEvent;
use tauri_runtime_wry::{
  wry::application::{
    event::Event,
    event_loop::{ControlFlow, EventLoopProxy, EventLoopWindowTarget},
  },
  EventLoopIterationContext, Message, Plugin, WebContextStore, WebviewId,
};

pub use egui;
pub use epi;

mod egui_impl;

struct CreateWindowPayload {
  label: String,
  app: Box<dyn epi::App + Send>,
  native_options: epi::NativeOptions,
}

pub struct EguiPlugin {
  create_window_channel: (
    SyncSender<CreateWindowPayload>,
    Receiver<CreateWindowPayload>,
  ),
  windows: Arc<Mutex<HashMap<WebviewId, egui_impl::WindowWrapper>>>,
  is_focused: bool,
}

pub struct EguiPluginHandle<R: Runtime> {
  app: AppHandle<R>,
  create_window_tx: SyncSender<CreateWindowPayload>,
}

impl Default for EguiPlugin {
  fn default() -> Self {
    Self {
      create_window_channel: sync_channel(1),
      windows: Default::default(),
      is_focused: false,
    }
  }
}

impl EguiPlugin {
  pub fn handle<R: Runtime>(&self, app: AppHandle<R>) -> EguiPluginHandle<R> {
    EguiPluginHandle {
      app,
      create_window_tx: self.create_window_channel.0.clone(),
    }
  }
}

impl<R: Runtime> EguiPluginHandle<R> {
  pub fn create_window(
    &self,
    label: String,
    app: Box<dyn epi::App + Send>,
    native_options: epi::NativeOptions,
  ) -> tauri::Result<()> {
    let create_window_tx = self.create_window_tx.clone();
    self.app.run_on_main_thread(move || {
      let _ = create_window_tx.send(CreateWindowPayload {
        label,
        app,
        native_options,
      });
    })
  }
}

impl<T: UserEvent> Plugin<T> for EguiPlugin {
  #[allow(dead_code)]
  fn on_event(
    &mut self,
    event: &Event<Message<T>>,
    event_loop: &EventLoopWindowTarget<Message<T>>,
    proxy: &EventLoopProxy<Message<T>>,
    control_flow: &mut ControlFlow,
    context: EventLoopIterationContext<'_, T>,
    web_context: &WebContextStore,
  ) -> bool {
    if let Ok(payload) = self.create_window_channel.1.try_recv() {
      egui_impl::create_gl_window(
        event_loop,
        &context.webview_id_map,
        &self.windows,
        &context.window_event_listeners,
        &context.menu_event_listeners,
        payload.label,
        payload.app,
        payload.native_options,
        proxy,
      );
    }
    egui_impl::handle_gl_loop(
      &self.windows,
      event,
      event_loop,
      control_flow,
      context,
      web_context,
      &mut self.is_focused,
    )
  }
}
