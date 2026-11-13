#ifndef RUNNER_FLUTTER_WINDOW_H_
#define RUNNER_FLUTTER_WINDOW_H_

#include <flutter/dart_project.h>
#include <flutter/flutter_view_controller.h>
#include <flutter/method_channel.h>
#include <flutter/encodable_value.h>

#include <memory>

#include "win32_window.h"

// A window that does nothing but host a Flutter view.
class FlutterWindow : public Win32Window {
 public:
  // Creates a new FlutterWindow hosting a Flutter view running |project|.
  explicit FlutterWindow(const flutter::DartProject& project);
  virtual ~FlutterWindow();

 protected:
  // Win32Window:
  bool OnCreate() override;
  void OnDestroy() override;
  LRESULT MessageHandler(HWND window, UINT const message, WPARAM const wparam,
                         LPARAM const lparam) noexcept override;

 private:
  // The project to run.
  flutter::DartProject project_;

  // The Flutter instance hosted by this window.
  std::unique_ptr<flutter::FlutterViewController> flutter_controller_;

  // "Block screenshots": keeps this window out of screenshots, screen
  // recordings and screen sharing (including Cipher Desk's own Share screen).
  std::unique_ptr<flutter::MethodChannel<flutter::EncodableValue>> secure_channel_;

  // Real full screen for remote sessions: no frame, covers the whole monitor
  // (Windows then hides the taskbar), and back to the saved size and place.
  std::unique_ptr<flutter::MethodChannel<flutter::EncodableValue>> window_channel_;
  void SetFullScreen(bool on);

  // Copy and paste of files with Explorer (see file_clipboard.h).
  std::unique_ptr<flutter::MethodChannel<flutter::EncodableValue>> file_clipboard_channel_;
  bool fullscreen_ = false;
  LONG_PTR saved_style_ = 0;
  WINDOWPLACEMENT saved_placement_ = {sizeof(WINDOWPLACEMENT)};
};

#endif  // RUNNER_FLUTTER_WINDOW_H_
