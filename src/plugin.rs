// Copyright 2019-2021 Tauri Programme within The Commons Conservancy
// SPDX-License-Identifier: Apache-2.0
// SPDX-License-Identifier: MIT

use eframe::{
  native::epi_integration::{self, EpiIntegration},
  CreationContext, NativeOptions, Theme,
};
use egui::NumExt;
use glutin::{display::GetGlDisplay, prelude::*};
use raw_window_handle::HasRawWindowHandle;
use tao::event_loop::EventLoop;
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
  Context, CursorIconWrapper, EventLoopIterationContext, Message, PhysicalPositionWrapper,
  PhysicalSizeWrapper, Plugin, PositionWrapper, RawWindowHandle, SizeWrapper, WebContextStore,
  WebviewId, WindowEventListeners, WindowEventWrapper, WindowId, WindowMenuEventListeners,
  WindowMessage,
};

use crate::{Error, Result};

use std::{
  cell::RefCell,
  collections::HashMap,
  ops::Deref,
  rc::Rc,
  sync::{
    mpsc::{channel, sync_channel, Receiver, Sender, SyncSender},
    Arc, Mutex,
  },
  time::Instant,
};

// pub mod window;
// pub(super) use window::Window;

pub type AppCreator = Box<dyn FnOnce(&CreationContext<'_>) -> Box<dyn eframe::App + Send> + Send>;

#[derive(Debug)]
enum EventResult {
  Wait,

  /// Causes a synchronous repaint inside the event handler. This should only
  /// be used in special situations if the window must be repainted while
  /// handling a specific event. This occurs on Windows when handling resizes.
  ///
  /// `RepaintNow` creates a new frame synchronously, and should therefore
  /// only be used for extremely urgent repaints.
  RepaintNow,

  /// Queues a repaint for once the event loop handles its next redraw. Exists
  /// so that multiple input events can be handled in one frame. Does not
  /// cause any delay like `RepaintNow`.
  RepaintNext,

  RepaintAt(Instant),

  Exit,
}

pub struct CreateWindowPayload {
  app_creator: AppCreator,
  title: String,
  native_options: NativeOptions,
}

pub struct EguiPlugin<T: UserEvent> {
  pub(crate) context: Context<T>,
  pub(crate) create_window_channel: (
    SyncSender<CreateWindowPayload>,
    Receiver<CreateWindowPayload>,
  ),
  pub(crate) running: Option<GlowWinitRunning>,
  pub(crate) is_focused: bool,
  pub(crate) next_repaint_time: Instant,
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
  pub fn get_window(&self) -> Option<()> {
    //TODO
    None
  }

  pub fn create_window(
    &self,
    label: String,
    app_creator: AppCreator,
    title: String,
    native_options: eframe::NativeOptions,
  ) -> crate::Result<()> {
    self.context.run_threaded(|main_thread| {
      // if let Some(main_thread) = main_thread {
      //   todo!()
      //   // create_gl_window(
      //   //   &main_thread.window_target,
      //   //   label,
      //   //   app_creator,
      //   //   title,
      //   //   native_options,
      //   //   window_id,
      //   //   &self.context.proxy,
      //   // )
      // } else {
      // let (tx, rx) = channel();
      let payload = CreateWindowPayload {
        app_creator,
        title,
        native_options,
      };
      self.create_window_tx.send(payload).unwrap();
      // force the event loop to receive a new event
      let _ = self
        .context
        .proxy
        .send_event(Message::Task(Box::new(move || {})));
      // }
    });
    Ok(())
  }
}

impl<T: UserEvent> Plugin<T> for EguiPlugin<T> {
  fn on_event(
    &mut self,
    event: &Event<Message<T>>,
    event_loop: &EventLoopWindowTarget<Message<T>>,
    proxy: &EventLoopProxy<Message<T>>,
    control_flow: &mut ControlFlow,
    context: EventLoopIterationContext<'_, T>,
    _web_context: &WebContextStore,
  ) -> bool {
    // Create egui context if received payload. Close previous one if there's one.
    if let Ok(payload) = self.create_window_channel.1.try_recv() {
      self.save_and_destroy();

      let storage = epi_integration::create_storage(&payload.title);

      if let Ok((gl_window, gl)) = create_glutin_windowed_context(
        event_loop,
        storage.as_deref(),
        &payload.title,
        &payload.native_options,
      ) {
        let gl = Arc::new(gl);

        let painter =
          egui_glow::Painter::new(gl.clone(), "", payload.native_options.shader_version)
            .unwrap_or_else(|error| panic!("some OpenGL error occurred {}\n", error));
        let system_theme = system_theme(gl_window.window(), &payload.native_options);
        let mut integration = epi_integration::EpiIntegration::new(
          event_loop,
          painter.max_texture_side(),
          gl_window.window(),
          system_theme,
          &payload.title,
          &payload.native_options,
          storage,
          Some(gl.clone()),
        );
        let theme = system_theme.unwrap_or(payload.native_options.default_theme);
        integration.egui_ctx.set_visuals(theme.egui_visuals());

        if payload.native_options.mouse_passthrough {
          gl_window.window().set_ignore_cursor_events(true).unwrap();
        }

        {
          // TODO Set correct window_id
          let window_id = 1000;
          let event_loop_proxy = proxy.clone();
          integration
            .egui_ctx
            .set_request_repaint_callback(move |_info| {
              // log::trace!("request_repaint_callback: {info:?}");
              // let when = Instant::now() + info.after;
              // let frame_nr = info.current_frame_nr;
              event_loop_proxy
                .send_event(Message::Window(window_id, WindowMessage::RequestRedraw))
                .ok();
            });
        }

        let mut app = (payload.app_creator)(&eframe::CreationContext {
          egui_ctx: integration.egui_ctx.clone(),
          integration_info: integration.frame.info(),
          storage: integration.frame.storage(),
          gl: Some(gl.clone()),
        });

        if app.warm_up_enabled() {
          integration.warm_up(app.as_mut(), gl_window.window());
        }

        self.running = Some(GlowWinitRunning {
          gl_window,
          gl,
          painter,
          integration,
          app,
          menu_items: Default::default(),
          window_event_listeners: Default::default(),
          menu_event_listeners: Default::default(),
        });
      }
    }

    // Handle event
    let mut prevent_default = false;

    let event_result = match event {
      tao::event::Event::LoopDestroyed => EventResult::Exit,

      // Platform-dependent event handlers to workaround a winit bug
      // See: https://github.com/rust-windowing/winit/issues/987
      // See: https://github.com/rust-windowing/winit/issues/1619
      tao::event::Event::RedrawEventsCleared if cfg!(windows) => {
        self.next_repaint_time = extremely_far_future();
        self.run_ui_and_paint()
      }
      tao::event::Event::RedrawRequested(_) if !cfg!(windows) => {
        self.next_repaint_time = extremely_far_future();
        self.run_ui_and_paint()
      }

      tao::event::Event::UserEvent(Message::Window(id, WindowMessage::RequestRedraw)) => {
        // TODO check window ID
        EventResult::RepaintNow
      }

      tao::event::Event::NewEvents(tao::event::StartCause::ResumeTimeReached { .. }) => {
        EventResult::Wait
      } // We just woke up to check next_repaint_time

      event => match self.handle_gl_loop(event, event_loop, &mut prevent_default) {
        Ok(event_result) => event_result,
        Err(err) => {
          log::error!("Exiting because of error: {err:?}");
          EventResult::Exit
        }
      },
    };

    match event_result {
      EventResult::Wait => {}
      EventResult::RepaintNow => {
        //log::trace!("Repaint caused by winit::Event: {:?}", event);
        if cfg!(windows) {
          // Fix flickering on Windows, see https://github.com/emilk/egui/pull/2280
          self.next_repaint_time = extremely_far_future();
          self.run_ui_and_paint();
        } else {
          // Fix for https://github.com/emilk/egui/issues/2425
          self.next_repaint_time = Instant::now();
        }
      }
      EventResult::RepaintNext => {
        //log::trace!("Repaint caused by winit::Event: {:?}", event);
        self.next_repaint_time = Instant::now();
      }
      EventResult::RepaintAt(repaint_time) => {
        self.next_repaint_time = self.next_repaint_time.min(repaint_time);
      }
      EventResult::Exit => {
        log::debug!("Asking to exit event loop…");
        self.save_and_destroy();
      }
    }

    *control_flow = if self.next_repaint_time <= Instant::now() {
      if let Some(window) = self.window() {
        window.request_redraw();
      }
      self.next_repaint_time = extremely_far_future();
      ControlFlow::Poll
    } else {
      ControlFlow::WaitUntil(self.next_repaint_time)
    };

    prevent_default
  }
}

impl<T: UserEvent> EguiPlugin<T> {
  fn handle_gl_loop(
    &mut self,
    event: &Event<'_, Message<T>>,
    event_loop: &EventLoopWindowTarget<Message<T>>,
    prevent_default: &mut bool,
  ) -> Result<EventResult> {
    let result = if let Some(running) = &mut self.running {
      match event {
        tao::event::Event::Resumed => {
          running.gl_window.on_resume(event_loop)?;
          EventResult::RepaintNow
        }
        tao::event::Event::Suspended => {
          self.running.as_mut().unwrap().gl_window.on_suspend()?;

          EventResult::Wait
        }
        tao::event::Event::WindowEvent { event, .. } => {
          // On Windows, if a window is resized by the user, it should repaint synchronously, inside the
          // event handler.
          //
          // If this is not done, the compositor will assume that the window does not want to redraw,
          // and continue ahead.
          //
          // In eframe's case, that causes the window to rapidly flicker, as it struggles to deliver
          // new frames to the compositor in time.
          //
          // The flickering is technically glutin or glow's fault, but we should be responding properly
          // to resizes anyway, as doing so avoids dropping frames.
          //
          // See: https://github.com/emilk/egui/issues/903
          let mut repaint_asap = false;

          match &event {
            tao::event::WindowEvent::Focused(new_focused) => {
              self.is_focused = *new_focused;
            }
            tao::event::WindowEvent::Resized(physical_size) => {
              repaint_asap = true;

              // Resize with 0 width and height is used by winit to signal a minimize event on Windows.
              // See: https://github.com/rust-windowing/winit/issues/208
              // This solves an issue where the app would panic when minimizing on Windows.
              if physical_size.width > 0 && physical_size.height > 0 {
                running.gl_window.resize(*physical_size);
              }
            }
            tao::event::WindowEvent::ScaleFactorChanged { new_inner_size, .. } => {
              repaint_asap = true;
              running.gl_window.resize(**new_inner_size);
            }
            tao::event::WindowEvent::CloseRequested if running.integration.should_close() => {
              log::debug!("Received WindowEvent::CloseRequested");
              // TODO on_close_requested
              return Ok(EventResult::Exit);
            }
            _ => {}
          }

          let event_response = running.integration.on_event(running.app.as_mut(), event);

          *prevent_default = true;
          if running.integration.should_close() {
            EventResult::Exit
          } else if event_response.repaint {
            if repaint_asap {
              EventResult::RepaintNow
            } else {
              EventResult::RepaintNext
            }
          } else {
            EventResult::Wait
          }
        }
        // TODO userevent
        // tao::event::Event::UserEvent(message) => {
        //   //handle_user_message(message, windows);
        // }
        _ => EventResult::Wait,
      }
    } else {
      EventResult::Wait
    };
    Ok(result)
  }

  fn frame_nr(&self) -> u64 {
    self
      .running
      .as_ref()
      .map_or(0, |r| r.integration.egui_ctx.frame_nr())
  }

  fn is_focused(&self) -> bool {
    self.is_focused
  }

  fn integration(&self) -> Option<&EpiIntegration> {
    self.running.as_ref().map(|r| &r.integration)
  }

  fn window(&self) -> Option<&tao::window::Window> {
    self.running.as_ref().map(|r| r.gl_window.window())
  }

  fn save_and_destroy(&mut self) {
    if let Some(mut running) = self.running.take() {
      running
        .integration
        .save(running.app.as_mut(), running.gl_window.window());
      running.app.on_exit(Some(&running.gl));
      running.painter.destroy();
    }
  }

  fn run_ui_and_paint(&mut self) -> EventResult {
    if let Some(running) = &mut self.running {
      // #[cfg(feature = "puffin")]
      // puffin::GlobalProfiler::lock().new_frame();
      // crate::profile_scope!("frame");

      let GlowWinitRunning {
        gl_window,
        gl,
        app,
        integration,
        painter,
        ..
      } = running;

      let window = gl_window.window();

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

      let clipped_primitives = {
        // crate::profile_scope!("tessellate");
        integration.egui_ctx.tessellate(shapes)
      };

      painter.paint_and_update_textures(
        screen_size_in_pixels,
        integration.egui_ctx.pixels_per_point(),
        &clipped_primitives,
        &textures_delta,
      );

      integration.post_rendering(app.as_mut(), window);

      {
        // crate::profile_scope!("swap_buffers");
        gl_window.swap_buffers().unwrap();
      }

      integration.post_present(window);

      let control_flow = if integration.should_close() {
        EventResult::Exit
      } else if repaint_after.is_zero() {
        EventResult::RepaintNext
      } else if let Some(repaint_after_instant) =
        std::time::Instant::now().checked_add(repaint_after)
      {
        // if repaint_after is something huge and can't be added to Instant,
        // we will use `ControlFlow::Wait` instead.
        // technically, this might lead to some weird corner cases where the user *WANTS*
        // winit to use `WaitUntil(MAX_INSTANT)` explicitly. they can roll their own
        // egui backend impl i guess.
        EventResult::RepaintAt(repaint_after_instant)
      } else {
        EventResult::Wait
      };

      integration.maybe_autosave(app.as_mut(), window);

      if !self.is_focused {
        // On Mac, a minimized Window uses up all CPU: https://github.com/emilk/egui/issues/325
        // We can't know if we are minimized: https://github.com/rust-windowing/winit/issues/208
        // But we know if we are focused (in foreground). When minimized, we are not focused.
        // However, a user may want an egui with an animation in the background,
        // so we still need to repaint quite fast.
        // crate::profile_scope!("bg_sleep");
        std::thread::sleep(std::time::Duration::from_millis(10));
      }

      control_flow
    } else {
      EventResult::Wait
    }
  }
}

fn create_glutin_windowed_context<T: UserEvent>(
  event_loop: &EventLoopWindowTarget<Message<T>>,
  storage: Option<&dyn eframe::Storage>,
  title: &str,
  native_options: &NativeOptions,
) -> Result<(GlutinWindowContext, glow::Context)> {
  let window_settings = epi_integration::load_window_settings(storage.as_deref());
  let winit_window_builder =
    epi_integration::window_builder(event_loop, title, native_options, window_settings);
  let mut glutin_window_context =
    unsafe { GlutinWindowContext::new(winit_window_builder, native_options, event_loop)? };
  glutin_window_context.on_resume(event_loop)?;

  if let Some(window) = &glutin_window_context.window {
    epi_integration::apply_native_options_to_window(window, native_options);
  }

  let gl = unsafe {
    glow::Context::from_loader_function(|s| {
      let s = std::ffi::CString::new(s)
        .expect("failed to construct C string from string for gl proc address");

      glutin_window_context.get_proc_address(&s)
    })
  };

  Ok((glutin_window_context, gl))
}

pub struct GlowWinitRunning {
  gl: Arc<glow::Context>,
  painter: egui_glow::Painter,
  integration: epi_integration::EpiIntegration,
  app: Box<dyn eframe::App>,
  // Conceptually this will be split out eventually so that the rest of the state
  // can be persistent.
  gl_window: GlutinWindowContext,
  menu_items: Option<HashMap<u16, CustomMenuItem>>,
  window_event_listeners: WindowEventListeners,
  menu_event_listeners: WindowMenuEventListeners,
}

unsafe impl Send for GlowWinitRunning {}

struct GlutinWindowContext {
  builder: tao::window::WindowBuilder,
  swap_interval: glutin::surface::SwapInterval,
  gl_config: glutin::config::Config,
  current_gl_context: Option<glutin::context::PossiblyCurrentContext>,
  gl_surface: Option<glutin::surface::Surface<glutin::surface::WindowSurface>>,
  not_current_gl_context: Option<glutin::context::NotCurrentContext>,
  window: Option<tao::window::Window>,
}

impl GlutinWindowContext {
  /// There is a lot of complexity with opengl creation, so prefer extensive logging to get all the help we can to debug issues.
  ///
  #[allow(unsafe_code)]
  unsafe fn new<T: UserEvent>(
    winit_window_builder: tao::window::WindowBuilder,
    native_options: &NativeOptions,
    event_loop: &EventLoopWindowTarget<Message<T>>,
  ) -> Result<Self> {
    use glutin::prelude::*;
    // convert native options to glutin options
    let hardware_acceleration = match native_options.hardware_acceleration {
      eframe::HardwareAcceleration::Required => Some(true),
      eframe::HardwareAcceleration::Preferred => None,
      eframe::HardwareAcceleration::Off => Some(false),
    };
    let swap_interval = if native_options.vsync {
      glutin::surface::SwapInterval::Wait(std::num::NonZeroU32::new(1).unwrap())
    } else {
      glutin::surface::SwapInterval::DontWait
    };
    /*  opengl setup flow goes like this:
        1. we create a configuration for opengl "Display" / "Config" creation
        2. choose between special extensions like glx or egl or wgl and use them to create config/display
        3. opengl context configuration
        4. opengl context creation
    */
    // start building config for gl display
    let config_template_builder = glutin::config::ConfigTemplateBuilder::new()
      .prefer_hardware_accelerated(hardware_acceleration)
      .with_depth_size(native_options.depth_buffer)
      .with_stencil_size(native_options.stencil_buffer)
      .with_transparency(native_options.transparent);
    // we don't know if multi sampling option is set. so, check if its more than 0.
    let config_template_builder = if native_options.multisampling > 0 {
      config_template_builder.with_multisampling(
        native_options
          .multisampling
          .try_into()
          .expect("failed to fit multisamples option of native_options into u8"),
      )
    } else {
      config_template_builder
    };

    log::debug!(
      "trying to create glutin Display with config: {:?}",
      &config_template_builder
    );
    // create gl display. this may probably create a window too on most platforms. definitely on `MS windows`. never on android.
    let (window, gl_config) = glutin_tao::DisplayBuilder::new()
      // we might want to expose this option to users in the future. maybe using an env var or using native_options.
      .with_preference(glutin_tao::ApiPreference::FallbackEgl) // https://github.com/emilk/egui/issues/2520#issuecomment-1367841150
      .with_window_builder(Some(winit_window_builder.clone()))
      .build(
        event_loop,
        config_template_builder.clone(),
        |mut config_iterator| {
          let config = config_iterator
            .next()
            .expect("failed to find a matching configuration for creating glutin config");

          log::debug!(
            "using the first config from config picker closure. config: {:?}",
            &config
          );
          config
        },
      )
      .map_err(|e| crate::Error::NoGlutinConfigs)?;

    let gl_display = gl_config.display();

    log::debug!(
      "successfully created GL Display with version: {} and supported features: {:?}",
      gl_display.version_string(),
      gl_display.supported_features()
    );
    let raw_window_handle = window.as_ref().map(|w| w.raw_window_handle());
    log::debug!(
      "creating gl context using raw window handle: {:?}",
      raw_window_handle
    );

    // create gl context. if core context cannot be created, try gl es context as fallback.
    let context_attributes =
      glutin::context::ContextAttributesBuilder::new().build(raw_window_handle);
    let fallback_context_attributes = glutin::context::ContextAttributesBuilder::new()
      .with_context_api(glutin::context::ContextApi::Gles(None))
      .build(raw_window_handle);
    let gl_context = match gl_config
      .display()
      .create_context(&gl_config, &context_attributes)
    {
      Ok(it) => it,
      Err(err) => {
        log::warn!("failed to create context using default context attributes {context_attributes:?} due to error: {err}");
        log::debug!("retrying with fallback context attributes: {fallback_context_attributes:?}");
        gl_config
          .display()
          .create_context(&gl_config, &fallback_context_attributes)?
      }
    };
    let not_current_gl_context = Some(gl_context);

    // the fun part with opengl gl is that we never know whether there is an error. the context creation might have failed, but
    // it could keep working until we try to make surface current or swap buffers or something else. future glutin improvements might
    // help us start from scratch again if we fail context creation and go back to preferEgl or try with different config etc..
    // https://github.com/emilk/egui/pull/2541#issuecomment-1370767582
    Ok(GlutinWindowContext {
      builder: winit_window_builder,
      swap_interval,
      gl_config,
      current_gl_context: None,
      window,
      gl_surface: None,
      not_current_gl_context,
    })
  }

  /// This will be run after `new`. on android, it might be called multiple times over the course of the app's lifetime.
  /// roughly,
  /// 1. check if window already exists. otherwise, create one now.
  /// 2. create attributes for surface creation.
  /// 3. create surface.
  /// 4. make surface and context current.
  ///
  /// we presently assume that we will
  #[allow(unsafe_code)]
  fn on_resume<T: UserEvent>(
    &mut self,
    event_loop: &EventLoopWindowTarget<Message<T>>,
  ) -> Result<()> {
    if self.gl_surface.is_some() {
      log::warn!("on_resume called even thought we already have a surface. early return");
      return Ok(());
    }
    log::debug!("running on_resume fn.");
    // make sure we have a window or create one.
    let window = self.window.take().unwrap_or_else(|| {
      log::debug!("window doesn't exist yet. creating one now with finalize_window");
      glutin_tao::finalize_window(event_loop, self.builder.clone(), &self.gl_config)
        .expect("failed to finalize glutin window")
    });
    // surface attributes
    let (width, height): (u32, u32) = window.inner_size().into();
    let width = std::num::NonZeroU32::new(width.at_least(1)).unwrap();
    let height = std::num::NonZeroU32::new(height.at_least(1)).unwrap();
    let surface_attributes = glutin::surface::SurfaceAttributesBuilder::<
      glutin::surface::WindowSurface,
    >::new()
    .build(window.raw_window_handle(), width, height);
    log::debug!(
      "creating surface with attributes: {:?}",
      &surface_attributes
    );
    // create surface
    let gl_surface = unsafe {
      self
        .gl_config
        .display()
        .create_window_surface(&self.gl_config, &surface_attributes)?
    };
    log::debug!("surface created successfully: {gl_surface:?}.making context current");
    // make surface and context current.
    let not_current_gl_context = self
      .not_current_gl_context
      .take()
      .expect("failed to get not current context after resume event. impossible!");
    let current_gl_context = not_current_gl_context.make_current(&gl_surface)?;
    // try setting swap interval. but its not absolutely necessary, so don't panic on failure.
    log::debug!("made context current. setting swap interval for surface");
    if let Err(e) = gl_surface.set_swap_interval(&current_gl_context, self.swap_interval) {
      log::error!("failed to set swap interval due to error: {e:?}");
    }
    // we will reach this point only once in most platforms except android.
    // create window/surface/make context current once and just use them forever.
    self.gl_surface = Some(gl_surface);
    self.current_gl_context = Some(current_gl_context);
    self.window = Some(window);
    Ok(())
  }

  /// only applies for android. but we basically drop surface + window and make context not current
  fn on_suspend(&mut self) -> Result<()> {
    log::debug!("received suspend event. dropping window and surface");
    self.gl_surface.take();
    self.window.take();
    if let Some(current) = self.current_gl_context.take() {
      log::debug!("context is current, so making it non-current");
      self.not_current_gl_context = Some(current.make_not_current()?);
    } else {
      log::debug!("context is already not current??? could be duplicate suspend event");
    }
    Ok(())
  }

  fn window(&self) -> &tao::window::Window {
    self.window.as_ref().expect("winit window doesn't exist")
  }

  fn resize(&self, physical_size: tao::dpi::PhysicalSize<u32>) {
    let width = std::num::NonZeroU32::new(physical_size.width.at_least(1)).unwrap();
    let height = std::num::NonZeroU32::new(physical_size.height.at_least(1)).unwrap();
    self
      .gl_surface
      .as_ref()
      .expect("failed to get surface to resize")
      .resize(
        self
          .current_gl_context
          .as_ref()
          .expect("failed to get current context to resize surface"),
        width,
        height,
      );
  }

  fn swap_buffers(&self) -> glutin::error::Result<()> {
    self
      .gl_surface
      .as_ref()
      .expect("failed to get surface to swap buffers")
      .swap_buffers(
        self
          .current_gl_context
          .as_ref()
          .expect("failed to get current context to swap buffers"),
      )
  }

  fn get_proc_address(&self, addr: &std::ffi::CStr) -> *const std::ffi::c_void {
    self.gl_config.display().get_proc_address(addr)
  }
}

fn system_theme(window: &tao::window::Window, options: &NativeOptions) -> Option<Theme> {
  // TODO detect system theam
  None
}

// ----------------------------------------------------------------------------

fn extremely_far_future() -> std::time::Instant {
  std::time::Instant::now() + std::time::Duration::from_secs(10_000_000_000)
}

/*
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
*/
