#include "file_clipboard.h"

#include <flutter/standard_method_codec.h>
#include <windows.h>
#include <shellapi.h>
#include <shlobj.h>

#include <cstring>
#include <string>
#include <vector>

namespace {

// Most files read from one copy (a guard against huge selections).
constexpr UINT kMaxFiles = 1000;

std::string ToUtf8(const std::wstring& wide) {
  if (wide.empty()) return std::string();
  int size = WideCharToMultiByte(CP_UTF8, 0, wide.data(),
                                 static_cast<int>(wide.size()), nullptr, 0,
                                 nullptr, nullptr);
  if (size <= 0) return std::string();
  std::string out(static_cast<size_t>(size), '\0');
  WideCharToMultiByte(CP_UTF8, 0, wide.data(), static_cast<int>(wide.size()),
                      out.data(), size, nullptr, nullptr);
  return out;
}

std::wstring ToWide(const std::string& utf8) {
  if (utf8.empty()) return std::wstring();
  int size = MultiByteToWideChar(CP_UTF8, 0, utf8.data(),
                                 static_cast<int>(utf8.size()), nullptr, 0);
  if (size <= 0) return std::wstring();
  std::wstring out(static_cast<size_t>(size), L'\0');
  MultiByteToWideChar(CP_UTF8, 0, utf8.data(), static_cast<int>(utf8.size()),
                      out.data(), size);
  return out;
}

// Paths of the files on the clipboard; empty if it holds none.
flutter::EncodableList ReadFiles() {
  flutter::EncodableList out;
  if (!IsClipboardFormatAvailable(CF_HDROP)) return out;
  if (!OpenClipboard(nullptr)) return out;
  HANDLE data = GetClipboardData(CF_HDROP);
  if (data != nullptr) {
    HDROP drop = static_cast<HDROP>(GlobalLock(data));
    if (drop != nullptr) {
      UINT count = DragQueryFileW(drop, 0xFFFFFFFF, nullptr, 0);
      if (count > kMaxFiles) count = kMaxFiles;
      for (UINT i = 0; i < count; ++i) {
        UINT len = DragQueryFileW(drop, i, nullptr, 0);
        if (len == 0) continue;
        std::wstring path(len + 1, L'\0');
        DragQueryFileW(drop, i, path.data(), len + 1);
        path.resize(len);
        out.push_back(flutter::EncodableValue(ToUtf8(path)));
      }
      GlobalUnlock(data);
    }
  }
  CloseClipboard();
  return out;
}

// Puts `paths` on the clipboard as a file list. Returns false on failure.
bool WriteFiles(const std::vector<std::wstring>& paths) {
  size_t chars = 1;  // final NUL
  for (const auto& p : paths) chars += p.size() + 1;
  size_t bytes = sizeof(DROPFILES) + chars * sizeof(wchar_t);
  HGLOBAL block = GlobalAlloc(GMEM_MOVEABLE | GMEM_ZEROINIT, bytes);
  if (block == nullptr) return false;
  auto* header = static_cast<DROPFILES*>(GlobalLock(block));
  if (header == nullptr) {
    GlobalFree(block);
    return false;
  }
  header->pFiles = sizeof(DROPFILES);
  header->fWide = TRUE;
  auto* list = reinterpret_cast<wchar_t*>(reinterpret_cast<BYTE*>(header) +
                                          sizeof(DROPFILES));
  for (const auto& p : paths) {
    memcpy(list, p.c_str(), (p.size() + 1) * sizeof(wchar_t));
    list += p.size() + 1;
  }
  *list = L'\0';
  GlobalUnlock(block);

  if (!OpenClipboard(nullptr)) {
    GlobalFree(block);
    return false;
  }
  EmptyClipboard();
  bool ok = SetClipboardData(CF_HDROP, block) != nullptr;
  if (!ok) GlobalFree(block);  // Only freed if the clipboard did not take it.
  CloseClipboard();
  return ok;
}

}  // namespace

std::unique_ptr<flutter::MethodChannel<flutter::EncodableValue>>
CreateFileClipboardChannel(flutter::BinaryMessenger* messenger) {
  auto channel =
      std::make_unique<flutter::MethodChannel<flutter::EncodableValue>>(
          messenger, "rdpv/file_clipboard",
          &flutter::StandardMethodCodec::GetInstance());
  channel->SetMethodCallHandler(
      [](const flutter::MethodCall<flutter::EncodableValue>& call,
         std::unique_ptr<flutter::MethodResult<flutter::EncodableValue>>
             result) {
        const std::string& name = call.method_name();
        if (name == "sequence") {
          result->Success(flutter::EncodableValue(
              static_cast<int64_t>(GetClipboardSequenceNumber())));
        } else if (name == "readFiles") {
          result->Success(flutter::EncodableValue(ReadFiles()));
        } else if (name == "writeFiles") {
          const auto* list =
              std::get_if<flutter::EncodableList>(call.arguments());
          if (list == nullptr) {
            result->Error("bad_argument", "expected a list of paths");
            return;
          }
          std::vector<std::wstring> paths;
          for (const auto& v : *list) {
            const auto* s = std::get_if<std::string>(&v);
            if (s != nullptr && !s->empty()) paths.push_back(ToWide(*s));
          }
          if (paths.empty() || !WriteFiles(paths)) {
            result->Error("failed", "the files could not be put on the clipboard");
            return;
          }
          result->Success(flutter::EncodableValue(
              static_cast<int64_t>(GetClipboardSequenceNumber())));
        } else {
          result->NotImplemented();
        }
      });
  return channel;
}
