use super::Context;
use tauri::{
  CursorIcon, Icon, Monitor, PhysicalPosition, PhysicalSize, Position, Result, Size, Theme,
  UserAttentionType,
};
use tauri_runtime::{Error, UserEvent};
use tauri_runtime_wry::{Message, MonitorHandleWrapper, WebviewId, WindowMessage, WryIcon};

use std::sync::mpsc::channel;

pub struct Window<T: UserEvent> {
  pub(crate) id: WebviewId,
  pub(crate) context: Context<T>,
}

macro_rules! window_getter {
  ($self: ident, $message: expr) => {{
    let (tx, rx) = channel();

    $self.context.inner.run_threaded(|main_thread| {
      let message = Message::Window($self.id, $message(tx));
      if main_thread.is_some() {
        super::handle_user_message(&message, &$self.context.main_thread.windows);
        Ok(())
      } else {
        $self
          .context
          .inner
          .proxy
          .send_event(message)
          .map_err(|_| Error::EventLoopClosed)
          .map_err(tauri::Error::from)
      }
    })?;
    rx.recv()
      .map_err(|_| Error::FailedToReceiveMessage)
      .map_err(tauri::Error::from)
  }};
}

/// Window getters.
impl<T: UserEvent> Window<T> {
  /// Returns the scale factor that can be used to map logical pixels to physical pixels, and vice versa.
  pub fn scale_factor(&self) -> Result<f64> {
    window_getter!(self, WindowMessage::ScaleFactor)
  }

  /// Returns the position of the top-left hand corner of the window's client area relative to the top-left hand corner of the desktop.
  pub fn inner_position(&self) -> Result<PhysicalPosition<i32>> {
    window_getter!(self, WindowMessage::InnerPosition)?.map_err(Into::into)
  }

  /// Returns the position of the top-left hand corner of the window relative to the top-left hand corner of the desktop.
  pub fn outer_position(&self) -> Result<PhysicalPosition<i32>> {
    window_getter!(self, WindowMessage::OuterPosition)?.map_err(Into::into)
  }

  /// Returns the physical size of the window's client area.
  ///
  /// The client area is the content of the window, excluding the title bar and borders.
  pub fn inner_size(&self) -> Result<PhysicalSize<u32>> {
    window_getter!(self, WindowMessage::InnerSize)
  }

  /// Returns the physical size of the entire window.
  ///
  /// These dimensions include the title bar and borders. If you don't want that (and you usually don't), use inner_size instead.
  pub fn outer_size(&self) -> Result<PhysicalSize<u32>> {
    window_getter!(self, WindowMessage::OuterSize)
  }

  /// Gets the window's current fullscreen state.
  pub fn is_fullscreen(&self) -> Result<bool> {
    window_getter!(self, WindowMessage::IsFullscreen)
  }

  /// Gets the window's current maximized state.
  pub fn is_maximized(&self) -> Result<bool> {
    window_getter!(self, WindowMessage::IsMaximized)
  }

  /// Gets the window’s current decoration state.
  pub fn is_decorated(&self) -> Result<bool> {
    window_getter!(self, WindowMessage::IsDecorated)
  }

  /// Gets the window’s current resizable state.
  pub fn is_resizable(&self) -> Result<bool> {
    window_getter!(self, WindowMessage::IsResizable)
  }

  /// Gets the window's current vibility state.
  pub fn is_visible(&self) -> Result<bool> {
    window_getter!(self, WindowMessage::IsVisible)
  }

  /// Returns the monitor on which the window currently resides.
  ///
  /// Returns None if current monitor can't be detected.
  pub fn current_monitor(&self) -> Result<Option<Monitor>> {
    Ok(
      window_getter!(self, WindowMessage::CurrentMonitor)?
        .map(|m| MonitorHandleWrapper(m).into())
        .map(|m: tauri_runtime::monitor::Monitor| m.into()),
    )
  }

  /// Returns the primary monitor of the system.
  ///
  /// Returns None if it can't identify any monitor as a primary one.
  pub fn primary_monitor(&self) -> Result<Option<Monitor>> {
    Ok(
      window_getter!(self, WindowMessage::PrimaryMonitor)?
        .map(|m| MonitorHandleWrapper(m).into())
        .map(|m: tauri_runtime::monitor::Monitor| m.into()),
    )
  }

  /// Returns the list of all the monitors available on the system.
  pub fn available_monitors(&self) -> Result<Vec<Monitor>> {
    Ok(
      window_getter!(self, WindowMessage::AvailableMonitors)?
        .into_iter()
        .map(|m| MonitorHandleWrapper(m).into())
        .map(|m: tauri_runtime::monitor::Monitor| m.into())
        .collect(),
    )
  }

  /// Returns the current window theme.
  ///
  /// ## Platform-specific
  ///
  /// - **macOS**: Only supported on macOS 10.14+.
  /// - **Linux**: Not implemented, always return [`Theme::Light`].
  pub fn theme(&self) -> Result<Theme> {
    window_getter!(self, WindowMessage::Theme)
  }
}

/// Window setters and actions.
impl<T: UserEvent> Window<T> {
  fn send_event(&self, message: WindowMessage) -> Result<()> {
    self.context.inner.run_threaded(|main_thread| {
      let message = Message::Window(self.id, message);
      if main_thread.is_some() {
        super::handle_user_message(&message, &self.context.main_thread.windows);
        Ok(())
      } else {
        self
          .context
          .inner
          .proxy
          .send_event(message)
          .map_err(|_| Error::EventLoopClosed.into())
      }
    })
  }

  /// Centers the window.
  pub fn center(&self) -> Result<()> {
    self.send_event(WindowMessage::Center)
  }

  /// Requests user attention to the window, this has no effect if the application
  /// is already focused. How requesting for user attention manifests is platform dependent,
  /// see `UserAttentionType` for details.
  ///
  /// Providing `None` will unset the request for user attention. Unsetting the request for
  /// user attention might not be done automatically by the WM when the window receives input.
  ///
  /// ## Platform-specific
  ///
  /// - **macOS:** `None` has no effect.
  /// - **Linux:** Urgency levels have the same effect.
  pub fn request_user_attention(&self, request_type: Option<UserAttentionType>) -> Result<()> {
    self.send_event(WindowMessage::RequestUserAttention(
      request_type.map(Into::into),
    ))
  }

  /// Determines if this window should be resizable.
  pub fn set_resizable(&self, resizable: bool) -> Result<()> {
    self.send_event(WindowMessage::SetResizable(resizable))
  }

  /// Set this window's title.
  pub fn set_title(&self, title: &str) -> Result<()> {
    self.send_event(WindowMessage::SetTitle(title.into()))
  }

  /// Maximizes this window.
  pub fn maximize(&self) -> Result<()> {
    self.send_event(WindowMessage::Maximize)
  }

  /// Un-maximizes this window.
  pub fn unmaximize(&self) -> Result<()> {
    self.send_event(WindowMessage::Unmaximize)
  }

  /// Minimizes this window.
  pub fn minimize(&self) -> Result<()> {
    self.send_event(WindowMessage::Minimize)
  }

  /// Un-minimizes this window.
  pub fn unminimize(&self) -> Result<()> {
    self.send_event(WindowMessage::Unminimize)
  }

  /// Show this window.
  pub fn show(&self) -> Result<()> {
    self.send_event(WindowMessage::Show)
  }

  /// Hide this window.
  pub fn hide(&self) -> Result<()> {
    self.send_event(WindowMessage::Hide)
  }

  /// Closes this window.
  /// # Panics
  ///
  /// - Panics if the event loop is not running yet, usually when called on the [`setup`](crate::Builder#method.setup) closure.
  /// - Panics when called on the main thread, usually on the [`run`](crate::App#method.run) closure.
  ///
  /// You can spawn a task to use the API using [`crate::async_runtime::spawn`] or [`std::thread::spawn`] to prevent the panic.
  pub fn close(&self) -> Result<()> {
    self.send_event(WindowMessage::Close)
  }

  /// Determines if this window should be [decorated].
  ///
  /// [decorated]: https://en.wikipedia.org/wiki/Window_(computing)#Window_decoration
  pub fn set_decorations(&self, decorations: bool) -> Result<()> {
    self.send_event(WindowMessage::SetDecorations(decorations))
  }

  /// Determines if this window should always be on top of other windows.
  pub fn set_always_on_top(&self, always_on_top: bool) -> Result<()> {
    self.send_event(WindowMessage::SetAlwaysOnTop(always_on_top))
  }

  /// Resizes this window.
  pub fn set_size<S: Into<Size>>(&self, size: S) -> Result<()> {
    self.send_event(WindowMessage::SetSize(size.into()))
  }

  /// Sets this window's minimum size.
  pub fn set_min_size<S: Into<Size>>(&self, size: Option<S>) -> Result<()> {
    self.send_event(WindowMessage::SetMinSize(size.map(|s| s.into())))
  }

  /// Sets this window's maximum size.
  pub fn set_max_size<S: Into<Size>>(&self, size: Option<S>) -> Result<()> {
    self.send_event(WindowMessage::SetMaxSize(size.map(|s| s.into())))
  }

  /// Sets this window's position.
  pub fn set_position<Pos: Into<Position>>(&self, position: Pos) -> Result<()> {
    self.send_event(WindowMessage::SetPosition(position.into()))
  }

  /// Determines if this window should be fullscreen.
  pub fn set_fullscreen(&self, fullscreen: bool) -> Result<()> {
    self.send_event(WindowMessage::SetFullscreen(fullscreen))
  }

  /// Bring the window to front and focus.
  pub fn set_focus(&self) -> Result<()> {
    self.send_event(WindowMessage::SetFocus)
  }

  /// Sets this window' icon.
  pub fn set_icon(&self, icon: Icon) -> Result<()> {
    let icon: tauri_runtime::Icon = icon.try_into()?;
    self.send_event(WindowMessage::SetIcon(
      WryIcon::try_from(icon)
        .map_err(tauri_runtime::Error::from)?
        .0,
    ))
  }

  /// Whether to show the window icon in the task bar or not.
  pub fn set_skip_taskbar(&self, skip: bool) -> Result<()> {
    self.send_event(WindowMessage::SetSkipTaskbar(skip))
  }

  /// Grabs the cursor, preventing it from leaving the window.
  ///
  /// There's no guarantee that the cursor will be hidden. You should
  /// hide it by yourself if you want so.
  ///
  /// ## Platform-specific
  ///
  /// - **Linux:** Unsupported.
  /// - **macOS:** This locks the cursor in a fixed location, which looks visually awkward.
  pub fn set_cursor_grab(&self, grab: bool) -> Result<()> {
    self.send_event(WindowMessage::SetCursorGrab(grab))
  }

  /// Modifies the cursor's visibility.
  ///
  /// If `false`, this will hide the cursor. If `true`, this will show the cursor.
  ///
  /// ## Platform-specific
  ///
  /// - **Windows:** The cursor is only hidden within the confines of the window.
  /// - **macOS:** The cursor is hidden as long as the window has input focus, even if the cursor is
  ///   outside of the window.
  pub fn set_cursor_visible(&self, visible: bool) -> Result<()> {
    self.send_event(WindowMessage::SetCursorVisible(visible))
  }

  /// Modifies the cursor icon of the window.
  pub fn set_cursor_icon(&self, icon: CursorIcon) -> Result<()> {
    self.send_event(WindowMessage::SetCursorIcon(icon))
  }

  /// Changes the position of the cursor in window coordinates.
  pub fn set_cursor_position<Pos: Into<Position>>(&self, position: Pos) -> Result<()> {
    self.send_event(WindowMessage::SetCursorPosition(position.into()))
  }

  /// Starts dragging the window.
  pub fn start_dragging(&self) -> Result<()> {
    self.send_event(WindowMessage::DragWindow)
  }
}
