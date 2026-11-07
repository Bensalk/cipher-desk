#include "flutter_window.h"

#include <flutter/standard_method_codec.h>

#include <optional>

#ifndef WDA_EXCLUDEFROMCAPTURE
#define WDA_EXCLUDEFROMCAPTURE 0x00000011
#endif

#include "flutter/generated_plugin_registrant.h"

FlutterWindow::FlutterWindow(const flutter::DartProject& project)
    : project_(project) {}

FlutterWindow::~FlutterWindow() {}

bool FlutterWindow::OnCreate() {
  if (!Win32Window::OnCreate()) {
    return false;
  }

  RECT frame = GetClientArea();

  // The size here must match the window dimensions to avoid unnecessary surface
  // creation / destruction in the startup path.
  flutter_controller_ = std::make_unique<flutter::FlutterViewController>(
      frame.right - frame.left, frame.bottom - frame.top, project_);
  // Ensure that basic setup of the controller was successful.
  if (!flutter_controller_->engine() || !flutter_controller_->view()) {
    return false;
  }
  RegisterPlugins(flutter_controller_->engine());

  // Visible to screen capture until Dart turns protection on (only while a
  // remote session is shown). Not protected by default: Dart's first call can
  // arrive before the handler below exists and be lost, and a protected window
  // on a shared computer is invisible to the viewer but still takes clicks.
  secure_channel_ =
      std::make_unique<flutter::MethodChannel<flutter::EncodableValue>>(
          flutter_controller_->engine()->messenger(), "rdpv/secure_window",
          &flutter::StandardMethodCodec::GetInstance());
  secure_channel_->SetMethodCallHandler(
      [this](const flutter::MethodCall<flutter::EncodableValue>& call,
             std::unique_ptr<flutter::MethodResult<flutter::EncodableValue>>
                 result) {
        if (call.method_name() != "setSecure") {
          result->NotImplemented();
          return;
        }
        const auto* on = std::get_if<bool>(call.arguments());
        if (on == nullptr) {
          result->Error("bad_argument", "expected a bool");
          return;
        }
        // WDA_EXCLUDEFROMCAPTURE needs Windows 10 2004; older versions show
        // the window black instead (WDA_MONITOR).
        HWND window = GetHandle();
        if (!SetWindowDisplayAffinity(window,
                                      *on ? WDA_EXCLUDEFROMCAPTURE : WDA_NONE) &&
            *on) {
          SetWindowDisplayAffinity(window, WDA_MONITOR);
        }
        result->Success();
      });
  window_channel_ =
      std::make_unique<flutter::MethodChannel<flutter::EncodableValue>>(
          flutter_controller_->engine()->messenger(), "rdpv/window",
          &flutter::StandardMethodCodec::GetInstance());
  window_channel_->SetMethodCallHandler(
      [this](const flutter::MethodCall<flutter::EncodableValue>& call,
             std::unique_ptr<flutter::MethodResult<flutter::EncodableValue>>
                 result) {
        if (call.method_name() != "setFullScreen") {
          result->NotImplemented();
          return;
        }
        const auto* on = std::get_if<bool>(call.arguments());
        if (on == nullptr) {
          result->Error("bad_argument", "expected a bool");
          return;
        }
        SetFullScreen(*on);
        result->Success();
      });
  SetChildContent(flutter_controller_->view()->GetNativeWindow());

  flutter_controller_->engine()->SetNextFrameCallback([&]() {
    this->Show();
  });

  // Flutter can complete the first frame before the "show window" callback is
  // registered. The following call ensures a frame is pending to ensure the
  // window is shown. It is a no-op if the first frame hasn't completed yet.
  flutter_controller_->ForceRedraw();

  return true;
}

void FlutterWindow::SetFullScreen(bool on) {
  HWND window = GetHandle();
  if (on && !fullscreen_) {
    // Remember the frame, size and place, then cover the whole monitor the
    // window is on without a frame.
    MONITORINFO monitor = {sizeof(monitor)};
    if (!GetWindowPlacement(window, &saved_placement_) ||
        !GetMonitorInfo(MonitorFromWindow(window, MONITOR_DEFAULTTONEAREST),
                        &monitor)) {
      return;
    }
    saved_style_ = GetWindowLongPtr(window, GWL_STYLE);
    SetWindowLongPtr(window, GWL_STYLE, saved_style_ & ~WS_OVERLAPPEDWINDOW);
    SetWindowPos(window, HWND_TOP, monitor.rcMonitor.left, monitor.rcMonitor.top,
                 monitor.rcMonitor.right - monitor.rcMonitor.left,
                 monitor.rcMonitor.bottom - monitor.rcMonitor.top,
                 SWP_NOOWNERZORDER | SWP_FRAMECHANGED);
    fullscreen_ = true;
  } else if (!on && fullscreen_) {
    SetWindowLongPtr(window, GWL_STYLE, saved_style_);
    SetWindowPlacement(window, &saved_placement_);
    SetWindowPos(window, nullptr, 0, 0, 0, 0,
                 SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_NOOWNERZORDER |
                     SWP_FRAMECHANGED);
    fullscreen_ = false;
  }
}

void FlutterWindow::OnDestroy() {
  if (flutter_controller_) {
    flutter_controller_ = nullptr;
  }

  Win32Window::OnDestroy();
}

LRESULT
FlutterWindow::MessageHandler(HWND hwnd, UINT const message,
                              WPARAM const wparam,
                              LPARAM const lparam) noexcept {
  // Give Flutter, including plugins, an opportunity to handle window messages.
  if (flutter_controller_) {
    std::optional<LRESULT> result =
        flutter_controller_->HandleTopLevelWindowProc(hwnd, message, wparam,
                                                      lparam);
    if (result) {
      return *result;
    }
  }

  switch (message) {
    case WM_FONTCHANGE:
      flutter_controller_->engine()->ReloadSystemFonts();
      break;
  }

  return Win32Window::MessageHandler(hwnd, message, wparam, lparam);
}
