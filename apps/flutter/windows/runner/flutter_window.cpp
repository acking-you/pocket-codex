#include "flutter_window.h"

#include <optional>

#include "flutter/generated_plugin_registrant.h"

// Set in main.cpp: the RegisterWindowMessage id a second launch broadcasts to
// ask this (already-running) instance to surface its window.
extern UINT g_pocket_codex_show_msg;

namespace {

// Restores, shows and force-foregrounds |hwnd|, defeating Windows' foreground
// lock with the AttachThreadInput trick (the second instance already called
// AllowSetForegroundWindow(ASFW_ANY)). Used to surface the window when hidden
// to the tray and when a second launch asks the running instance to appear.
void ForceForegroundWindow(HWND hwnd) {
  if (hwnd == nullptr) {
    return;
  }
  if (::IsIconic(hwnd)) {
    ::ShowWindow(hwnd, SW_RESTORE);
  }
  ::ShowWindow(hwnd, SW_SHOW);

  HWND foreground = ::GetForegroundWindow();
  DWORD foreground_thread =
      ::GetWindowThreadProcessId(foreground, nullptr);
  DWORD this_thread = ::GetCurrentThreadId();
  // foreground_thread is 0 when no window holds focus; AttachThreadInput(0, ...)
  // is invalid, so skip the attach in that case.
  bool attached = foreground_thread != 0 &&
                  foreground_thread != this_thread &&
                  ::AttachThreadInput(foreground_thread, this_thread, TRUE);
  ::BringWindowToTop(hwnd);
  ::SetForegroundWindow(hwnd);
  ::SetActiveWindow(hwnd);
  if (attached) {
    ::AttachThreadInput(foreground_thread, this_thread, FALSE);
  }
}

}  // namespace

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
  // A second launch asked us to surface (see main.cpp). The id is unique to
  // this app, so only our own windows ever receive it.
  if (message == g_pocket_codex_show_msg && g_pocket_codex_show_msg != 0) {
    ForceForegroundWindow(GetHandle());
    return 0;
  }

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
