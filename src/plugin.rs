// Copyright 2019-2021 Tauri Programme within The Commons Conservancy
// SPDX-License-Identifier: Apache-2.0
// SPDX-License-Identifier: MIT

use eframe::CreationContext;
use raw_window_handle::HasRawWindowHandle;
use tauri_runtime::{window::WindowEvent, RunEvent, UserEvent};
#[cfg(target_os = "linux")]
use tauri_runtime_wry::wry::application::platform::unix::WindowExtUnix;
#[cfg(windows)]
use tauri_runtime_wry::wry::application::platform::windows::WindowExtWindows;
use tauri_runtime_wry::{
  center_window,
  wry::application::{
    event::{Event, WindowEvent as TaoWindowEvent},
    event_loop::{ControlFlow, EventLoopProxy, EventLoopWindowTarget},
    menu::CustomMenuItem,
    window::Fullscreen,
  },
  Context as WryContext, CursorIconWrapper, EventLoopIterationContext, Message,
  PhysicalPositionWrapper, PhysicalSizeWrapper, Plugin, PositionWrapper, RawWindowHandle,
  SizeWrapper, WebContextStore, WebviewId, WindowEventListeners, WindowEventWrapper, WindowId,
  WindowMenuEventListeners, WindowMessage,
};

use crate::{Error, Result};

use std::{
  cell::RefCell,
  collections::HashMap,
  ops::Deref,
  rc::Rc,
  sync::{
    mpsc::{channel, Receiver, Sender, SyncSender},
    Arc, Mutex,
  },
};

pub mod window;
pub(super) use window::Window;

pub type AppCreator = Box<dyn FnOnce(&CreationContext<'_>) -> Box<dyn eframe::App + Send> + Send>;

#[derive(Debug, Clone, Default)]
pub struct WebviewIdStore(Arc<Mutex<HashMap<WindowId, WebviewId>>>);

impl WebviewIdStore {
  pub fn insert(&self, w: WindowId, id: WebviewId) {
    self.0.lock().unwrap().insert(w, id);
  }

  fn get(&self, w: &WindowId) -> Option<WebviewId> {
    self.0.lock().unwrap().get(w).copied()
  }
}

pub struct CreateWindowPayload {
  window_id: WebviewId,
  label: String,
  app_creator: AppCreator,
  title: String,
  native_options: eframe::NativeOptions,
  tx: Sender<Result<()>>,
}

#[derive(Clone)]
pub struct MainThreadContext {
  pub(crate) windows: Arc<Mutex<HashMap<WebviewId, WindowWrapper>>>,
}

// SAFETY: we ensure this type is only used on the main thread.
#[allow(clippy::non_send_fields_in_send_ty)]
unsafe impl Send for MainThreadContext {}

// SAFETY: we ensure this type is only used on the main thread.
#[allow(clippy::non_send_fields_in_send_ty)]
unsafe impl Sync for MainThreadContext {}

#[derive(Clone)]
pub struct Context<T: UserEvent> {
  pub(crate) inner: WryContext<T>,
  pub(crate) main_thread: MainThreadContext,
  pub(crate) webview_id_map: WebviewIdStore,
}

pub struct EguiPlugin<T: UserEvent> {
  pub(crate) context: Context<T>,
  pub(crate) create_window_channel: (
    SyncSender<CreateWindowPayload>,
    Receiver<CreateWindowPayload>,
  ),
  pub(crate) is_focused: bool,
}

pub struct EguiPluginHandle<T: UserEvent = tauri::EventLoopMessage> {
  context: Context<T>,
  create_window_tx: SyncSender<CreateWindowPayload>,
}

impl<T: UserEvent> EguiPlugin<T> {
  pub(crate) fn handle(&self) -> EguiPluginHandle<T> {
    EguiPluginHandle {
      context: self.context.clone(),
      create_window_tx: self.create_window_channel.0.clone(),
    }
  }
}

impl<T: UserEvent> EguiPluginHandle<T> {
  /// Fetch a single window by its label.
  pub fn get_window(&self, label: &str) -> Option<Window<T>> {
    let windows = self.context.main_thread.windows.lock().unwrap();
    for (id, w) in &*windows {
      if w.label == label {
        return Some(Window {
          id: *id,
          context: self.context.clone(),
        });
      }
    }
    None
  }

  /// Fetch all managed windows.
  pub fn windows(&self) -> HashMap<String, Window<T>> {
    let windows = self.context.main_thread.windows.lock().unwrap();
    let mut list = HashMap::new();
    for (id, w) in &*windows {
      list.insert(
        w.label.clone(),
        Window {
          id: *id,
          context: self.context.clone(),
        },
      );
    }
    list
  }

  pub fn create_window(
    &self,
    label: String,
    app_creator: AppCreator,
    title: String,
    native_options: eframe::NativeOptions,
  ) -> crate::Result<Window<T>> {
    let window_id = rand::random();

    self.context.inner.run_threaded(|main_thread| {
      if let Some(main_thread) = main_thread {
        create_gl_window(
          &main_thread.window_target,
          &self.context.webview_id_map,
          &self.context.main_thread.windows,
          label,
          app_creator,
          title,
          native_options,
          window_id,
          &self.context.inner.proxy,
        )
      } else {
        let (tx, rx) = channel();
        let payload = CreateWindowPayload {
          window_id,
          label,
          app_creator,
          title,
          native_options,
          tx,
        };
        self.create_window_tx.send(payload).unwrap();
        // force the event loop to receive a new event
        let _ = self
          .context
          .inner
          .proxy
          .send_event(Message::Task(Box::new(move || {})));
        rx.recv().unwrap()
      }
    })?;
    Ok(Window {
      id: window_id,
      context: self.context.clone(),
    })
  }
}

impl<T: UserEvent> Plugin<T> for EguiPlugin<T> {
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
      let res = create_gl_window(
        event_loop,
        &self.context.webview_id_map,
        &self.context.main_thread.windows,
        payload.label,
        payload.app_creator,
        payload.title,
        payload.native_options,
        payload.window_id,
        proxy,
      );
      payload.tx.send(res).unwrap();
    }
    handle_gl_loop(
      &self.context,
      event,
      event_loop,
      control_flow,
      context,
      web_context,
      &mut self.is_focused,
    )
  }
}

#[allow(dead_code)]
pub enum MaybeRc<T> {
  Actual(T),
  Rc(Rc<T>),
}

impl<T> MaybeRc<T> {
  #[allow(dead_code)]
  pub fn new(t: T) -> Self {
    Self::Actual(t)
  }
}

impl<T> AsRef<T> for MaybeRc<T> {
  fn as_ref(&self) -> &T {
    match self {
      Self::Actual(t) => t,
      Self::Rc(t) => t,
    }
  }
}

impl<T> Deref for MaybeRc<T> {
  type Target = T;

  fn deref(&self) -> &Self::Target {
    match self {
      Self::Actual(v) => v,
      Self::Rc(r) => r.deref(),
    }
  }
}

impl<T> std::borrow::Borrow<T> for MaybeRc<T> {
  fn borrow(&self) -> &T {
    match self {
      Self::Actual(v) => v,
      Self::Rc(r) => r.borrow(),
    }
  }
}

#[allow(dead_code)]
pub enum MaybeRcCell<T> {
  Actual(RefCell<T>),
  RcCell(Rc<RefCell<T>>),
}

impl<T> MaybeRcCell<T> {
  #[allow(dead_code)]
  pub fn new(t: T) -> Self {
    Self::Actual(RefCell::new(t))
  }
}

impl<T> Deref for MaybeRcCell<T> {
  type Target = RefCell<T>;

  fn deref(&self) -> &Self::Target {
    match self {
      Self::Actual(v) => v,
      Self::RcCell(r) => r.deref(),
    }
  }
}

pub struct GlutinWindowContext {
  pub context: MaybeRc<glutin::ContextWrapper<glutin::PossiblyCurrent, glutin::window::Window>>,
  app: Box<dyn eframe::App + Send>,
  glow_context: Arc<glow::Context>,
  painter: MaybeRcCell<egui_glow::Painter>,
  integration: MaybeRcCell<eframe::native::epi_integration::EpiIntegration>,
}

#[allow(dead_code)]
pub struct WindowWrapper {
  label: String,
  inner: Option<Box<GlutinWindowContext>>,
  menu_items: Option<HashMap<u16, CustomMenuItem>>,
  window_event_listeners: WindowEventListeners,
  menu_event_listeners: WindowMenuEventListeners,
}

#[allow(clippy::too_many_arguments)]
pub fn create_gl_window<T: UserEvent>(
  event_loop: &EventLoopWindowTarget<Message<T>>,
  webview_id_map: &WebviewIdStore,
  windows: &Arc<Mutex<HashMap<WebviewId, WindowWrapper>>>,
  label: String,
  app_creator: AppCreator,
  title: String,
  native_options: eframe::NativeOptions,
  window_id: WebviewId,
  proxy: &EventLoopProxy<Message<T>>,
) -> Result<()> {
  if let Some(window) = windows
    .lock()
    .expect("poisoned window collection")
    .values_mut()
    .next()
  {
    on_window_close(&mut window.inner);
  }

  use eframe::native::epi_integration;

  let storage = epi_integration::create_storage(&label);
  let window_settings = epi_integration::load_window_settings(storage.as_deref());

  let window_builder =
    epi_integration::window_builder(&native_options, &window_settings).with_title(title);

  use eframe::HardwareAcceleration;
  let hardware_acceleration = match native_options.hardware_acceleration {
    HardwareAcceleration::Required => Some(true),
    HardwareAcceleration::Preferred => None,
    HardwareAcceleration::Off => Some(false),
  };

  let gl_window = unsafe {
    glutin::ContextBuilder::new()
      .with_hardware_acceleration(hardware_acceleration)
      .with_depth_buffer(native_options.depth_buffer)
      .with_multisampling(native_options.multisampling)
      .with_srgb(true)
      .with_stencil_buffer(native_options.stencil_buffer)
      .with_vsync(native_options.vsync)
      .build_windowed(window_builder, event_loop)?
      .make_current()
      .map_err(|(_, e)| e)?
  };

  webview_id_map.insert(gl_window.window().id(), window_id);

  let gl = unsafe { glow::Context::from_loader_function(|s| gl_window.get_proc_address(s)) };
  let gl = std::sync::Arc::new(gl);

  unsafe {
    use glow::HasContext as _;
    gl.enable(glow::FRAMEBUFFER_SRGB);
  }

  let painter =
    egui_glow::Painter::new(gl.clone(), None, "").map_err(Error::FailedToCreatePainter)?;

  let system_theme = native_options.system_theme();
  let mut integration = epi_integration::EpiIntegration::new(
    event_loop,
    painter.max_texture_side(),
    gl_window.window(),
    system_theme,
    storage,
    Some(gl.clone()),
  );
  let theme = system_theme.unwrap_or(native_options.default_theme);
  integration.egui_ctx.set_visuals(theme.egui_visuals());

  {
    let event_loop_proxy = egui::mutex::Mutex::new(proxy.clone());
    integration.egui_ctx.set_request_repaint_callback(move || {
      event_loop_proxy
        .lock()
        .send_event(Message::Window(window_id, WindowMessage::RequestRedraw))
        .ok();
    });
  }

  let mut app = app_creator(&eframe::CreationContext {
    egui_ctx: integration.egui_ctx.clone(),
    integration_info: integration.frame.info(),
    storage: integration.frame.storage(),
    gl: Some(gl.clone()),
    #[cfg(feature = "wgpu")]
    wgpu_render_state: None,
  });

  if app.warm_up_enabled() {
    integration.warm_up(app.as_mut(), gl_window.window());
  }

  windows.lock().expect("poisoned window collection").insert(
    window_id,
    WindowWrapper {
      label,
      inner: Some(Box::new(GlutinWindowContext {
        context: MaybeRc::new(gl_window),
        app,
        glow_context: gl,
        painter: MaybeRcCell::new(painter),
        integration: MaybeRcCell::new(integration),
      })),
      menu_items: Default::default(),
      menu_event_listeners: Default::default(),
      window_event_listeners: Default::default(),
    },
  );

  Ok(())
}

fn win_mac_gl_loop<T: UserEvent>(
  control_flow: &mut ControlFlow,
  glutin_window_context: &mut GlutinWindowContext,
  event: &Event<Message<T>>,
  is_focused: bool,
) -> bool {
  let gl_window = &glutin_window_context.context;
  let gl = &glutin_window_context.glow_context;
  let app = &mut glutin_window_context.app;
  let mut integration = glutin_window_context.integration.borrow_mut();
  let mut painter = glutin_window_context.painter.borrow_mut();
  let window = gl_window.window();

  let mut paint = || {
    let screen_size_in_pixels: [u32; 2] = window.inner_size().into();

    egui_glow::painter::clear(
      gl,
      screen_size_in_pixels,
      app.clear_color(&integration.egui_ctx.style().visuals),
    );

    let egui::FullOutput {
      platform_output,
      repaint_after,
      textures_delta,
      shapes,
    } = integration.update(app.as_mut(), window);

    integration.handle_platform_output(window, platform_output);

    let clipped_primitives = integration.egui_ctx.tessellate(shapes);

    painter.paint_and_update_textures(
      screen_size_in_pixels,
      integration.egui_ctx.pixels_per_point(),
      &clipped_primitives,
      &textures_delta,
    );

    integration.post_rendering(app.as_mut(), window);

    gl_window.swap_buffers().unwrap();

    let mut should_close = false;

    *control_flow = if integration.should_close() {
      should_close = true;
      ControlFlow::Wait
    } else if repaint_after.is_zero() {
      window.request_redraw();
      ControlFlow::Poll
    } else if let Some(repaint_after_instant) = std::time::Instant::now().checked_add(repaint_after)
    {
      // if repaint_after is something huge and can't be added to Instant,
      // we will use `ControlFlow::Wait` instead.
      // technically, this might lead to some weird corner cases where the user *WANTS*
      // winit to use `WaitUntil(MAX_INSTANT)` explicitly. they can roll their own
      // egui backend impl i guess.
      ControlFlow::WaitUntil(repaint_after_instant)
    } else {
      ControlFlow::Wait
    };

    integration.maybe_autosave(app.as_mut(), window);

    if !is_focused {
      // On Mac, a minimized Window uses up all CPU: https://github.com/emilk/egui/issues/325
      // We can't know if we are minimized: https://github.com/rust-windowing/winit/issues/208
      // But we know if we are focused (in foreground). When minimized, we are not focused.
      // However, a user may want an egui with an animation in the background,
      // so we still need to repaint quite fast.
      std::thread::sleep(std::time::Duration::from_millis(10));
    }

    should_close
  };

  match event {
    Event::RedrawEventsCleared => paint(),
    Event::RedrawRequested(_) => paint(),
    _ => false,
  }
}

pub fn handle_gl_loop<T: UserEvent>(
  egui_context: &Context<T>,
  event: &Event<'_, Message<T>>,
  _event_loop: &EventLoopWindowTarget<Message<T>>,
  control_flow: &mut ControlFlow,
  context: EventLoopIterationContext<'_, T>,
  _web_context: &WebContextStore,
  is_focused: &mut bool,
) -> bool {
  let mut prevent_default = false;
  let Context {
    main_thread: MainThreadContext { windows, .. },
    webview_id_map,
    ..
  } = egui_context;
  let EventLoopIterationContext { callback, .. } = context;
  let has_egui_window = !windows.lock().unwrap().is_empty();
  if has_egui_window {
    let mut windows_lock = windows.lock().unwrap();

    let iter = windows_lock.values_mut();

    let mut should_close = false;

    for win in iter {
      let mut should_close = false;
      if let Some(glutin_window_context) = &mut win.inner {
        should_close = win_mac_gl_loop(control_flow, glutin_window_context, event, *is_focused);
      }

      if should_close {
        on_window_close(&mut win.inner);
      }
    }

    match event {
      Event::WindowEvent {
        event, window_id, ..
      } => {
        if let Some(window_id) = webview_id_map.get(window_id) {
          if let TaoWindowEvent::Destroyed = event {
            windows_lock.remove(&window_id);
          }

          if let Some(window) = windows_lock.get_mut(&window_id) {
            let label = &window.label;
            let glutin_window_context = &mut window.inner;
            let window_event_listeners = &window.window_event_listeners;
            let handled = match event {
              TaoWindowEvent::Focused(new_focused) => {
                *is_focused = *new_focused;
                false
              }
              TaoWindowEvent::Resized(physical_size) => {
                // Resize with 0 width and height is used by winit to signal a minimize event on Windows.
                // See: https://github.com/rust-windowing/winit/issues/208
                // This solves an issue where the app would panic when minimizing on Windows.
                if physical_size.width > 0 && physical_size.height > 0 {
                  if let Some(glutin_window_context) = glutin_window_context.as_ref() {
                    glutin_window_context.context.resize(*physical_size);
                  }
                }
                false
              }
              TaoWindowEvent::CloseRequested => on_close_requested(
                callback,
                (label, glutin_window_context),
                window_event_listeners,
              ),
              _ => false,
            };

            if let Some(glutin_window_context) = glutin_window_context.as_mut() {
              let gl_window = &glutin_window_context.context;
              let app = &mut glutin_window_context.app;
              if !handled {
                let mut integration = glutin_window_context.integration.borrow_mut();
                integration.on_event(app.as_mut(), event);
                if integration.should_close() {
                  should_close = true;
                  *control_flow = ControlFlow::Wait;
                }
              }
              gl_window.window().request_redraw();
            }
            if should_close {
              on_window_close(glutin_window_context);
            } else if let Some(window) = windows_lock.get(&window_id) {
              if let Some(event) = WindowEventWrapper::from(event).0 {
                let label = window.label.clone();
                let window_event_listeners = window.window_event_listeners.clone();
                drop(windows_lock);
                callback(RunEvent::WindowEvent {
                  label,
                  event: event.clone(),
                });
                let listeners = window_event_listeners.lock().unwrap();
                let handlers = listeners.values();
                for handler in handlers {
                  handler(&event);
                }
              }
            }

            prevent_default = true;
          }
        }
      }

      Event::UserEvent(message) => {
        drop(windows_lock);
        handle_user_message(message, windows);
      }

      _ => (),
    }
  }

  prevent_default
}

pub(crate) fn handle_user_message<T: UserEvent>(
  message: &Message<T>,
  windows: &Arc<Mutex<HashMap<WebviewId, WindowWrapper>>>,
) {
  if let Message::Window(window_id, message) = message {
    if let Some(glutin_window_context_opt) = windows
      .lock()
      .unwrap()
      .get_mut(window_id)
      .map(|win| &mut win.inner)
    {
      if let Some(glutin_window_context) = glutin_window_context_opt {
        let window = glutin_window_context.context.window();
        match message {
          WindowMessage::ScaleFactor(tx) => tx.send(window.scale_factor()).unwrap(),
          WindowMessage::InnerPosition(tx) => tx
            .send(
              window
                .inner_position()
                .map(|p| PhysicalPositionWrapper(p).into())
                .map_err(|_| tauri_runtime::Error::FailedToSendMessage),
            )
            .unwrap(),
          WindowMessage::OuterPosition(tx) => tx
            .send(
              window
                .outer_position()
                .map(|p| PhysicalPositionWrapper(p).into())
                .map_err(|_| tauri_runtime::Error::FailedToSendMessage),
            )
            .unwrap(),
          WindowMessage::InnerSize(tx) => tx
            .send(PhysicalSizeWrapper(window.inner_size()).into())
            .unwrap(),
          WindowMessage::OuterSize(tx) => tx
            .send(PhysicalSizeWrapper(window.outer_size()).into())
            .unwrap(),
          WindowMessage::IsFullscreen(tx) => tx.send(window.fullscreen().is_some()).unwrap(),
          WindowMessage::IsMaximized(tx) => tx.send(window.is_maximized()).unwrap(),
          WindowMessage::IsDecorated(tx) => tx.send(window.is_decorated()).unwrap(),
          WindowMessage::IsResizable(tx) => tx.send(window.is_resizable()).unwrap(),
          WindowMessage::IsVisible(tx) => tx.send(window.is_visible()).unwrap(),
          WindowMessage::IsMenuVisible(tx) => tx.send(window.is_menu_visible()).unwrap(),
          WindowMessage::CurrentMonitor(tx) => tx.send(window.current_monitor()).unwrap(),
          WindowMessage::PrimaryMonitor(tx) => tx.send(window.primary_monitor()).unwrap(),
          WindowMessage::AvailableMonitors(tx) => {
            tx.send(window.available_monitors().collect()).unwrap()
          }
          WindowMessage::RawWindowHandle(tx) => tx
            .send(RawWindowHandle(window.raw_window_handle()))
            .unwrap(),
          WindowMessage::Theme(tx) => {
            tx.send(tauri_runtime_wry::map_theme(&window.theme()))
              .unwrap();
          }
          // Setters
          WindowMessage::Center => {
            let _ = center_window(window, window.inner_size());
          }
          WindowMessage::RequestUserAttention(request_type) => {
            window.request_user_attention(request_type.as_ref().map(|r| r.0));
          }
          WindowMessage::SetResizable(resizable) => window.set_resizable(*resizable),
          WindowMessage::SetTitle(title) => window.set_title(title),
          WindowMessage::Maximize => window.set_maximized(true),
          WindowMessage::Unmaximize => window.set_maximized(false),
          WindowMessage::Minimize => window.set_minimized(true),
          WindowMessage::Unminimize => window.set_minimized(false),
          WindowMessage::ShowMenu => window.show_menu(),
          WindowMessage::HideMenu => window.hide_menu(),
          WindowMessage::Show => window.set_visible(true),
          WindowMessage::Hide => window.set_visible(false),
          WindowMessage::Close => {
            on_window_close(glutin_window_context_opt);
          }
          WindowMessage::SetDecorations(decorations) => window.set_decorations(*decorations),
          WindowMessage::SetAlwaysOnTop(always_on_top) => {
            window.set_always_on_top(*always_on_top);
          }
          WindowMessage::SetSize(size) => {
            window.set_inner_size(SizeWrapper::from(*size).0);
          }
          WindowMessage::SetMinSize(size) => {
            window.set_min_inner_size(size.map(|s| SizeWrapper::from(s).0));
          }
          WindowMessage::SetMaxSize(size) => {
            window.set_max_inner_size(size.map(|s| SizeWrapper::from(s).0));
          }
          WindowMessage::SetPosition(position) => {
            window.set_outer_position(PositionWrapper::from(*position).0)
          }
          WindowMessage::SetFullscreen(fullscreen) => {
            if *fullscreen {
              window.set_fullscreen(Some(Fullscreen::Borderless(None)))
            } else {
              window.set_fullscreen(None)
            }
          }
          WindowMessage::SetFocus => {
            window.set_focus();
          }
          WindowMessage::SetIcon(icon) => {
            window.set_window_icon(Some(icon.clone()));
          }
          #[allow(unused_variables)]
          WindowMessage::SetSkipTaskbar(skip) => {
            #[cfg(any(windows, target_os = "linux"))]
            window.set_skip_taskbar(*skip);
          }
          WindowMessage::SetCursorGrab(grab) => {
            let _ = window.set_cursor_grab(*grab);
          }
          WindowMessage::SetCursorVisible(visible) => {
            window.set_cursor_visible(*visible);
          }
          WindowMessage::SetCursorIcon(icon) => {
            window.set_cursor_icon(CursorIconWrapper::from(*icon).0);
          }
          WindowMessage::SetCursorPosition(position) => {
            let _ = window.set_cursor_position(PositionWrapper::from(*position).0);
          }
          WindowMessage::DragWindow => {
            let _ = window.drag_window();
          }
          WindowMessage::UpdateMenuItem(_id, _update) => {
            // TODO
          }
          WindowMessage::RequestRedraw => {
            window.request_redraw();
          }
          _ => (),
        }
      }
    }
  }
}

fn on_close_requested<'a, T: UserEvent>(
  callback: &'a mut (dyn FnMut(RunEvent<T>) + 'static),
  (label, glutin_window_context): (&str, &mut Option<Box<GlutinWindowContext>>),
  window_event_listeners: &WindowEventListeners,
) -> bool {
  let (tx, rx) = channel();
  let listeners = window_event_listeners.lock().unwrap();
  let handlers = listeners.values();
  for handler in handlers {
    handler(&WindowEvent::CloseRequested {
      signal_tx: tx.clone(),
    });
  }
  callback(RunEvent::WindowEvent {
    label: label.into(),
    event: WindowEvent::CloseRequested { signal_tx: tx },
  });
  if let Ok(true) = rx.try_recv() {
    true
  } else {
    on_window_close(glutin_window_context);
    false
  }
}

fn on_window_close(glutin_window_context: &mut Option<Box<GlutinWindowContext>>) {
  // Destrooy GL context if its a GLWindow
  if let Some(mut glutin_window_context) = glutin_window_context.take() {
    let app = glutin_window_context.app.as_mut();
    glutin_window_context
      .integration
      .borrow_mut()
      .save(app, glutin_window_context.context.window());
    app.on_exit(Some(&glutin_window_context.glow_context));
    glutin_window_context.painter.borrow_mut().destroy();
  }
}
