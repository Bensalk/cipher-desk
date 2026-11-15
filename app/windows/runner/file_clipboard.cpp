#include "file_clipboard.h"

#include <flutter/standard_method_codec.h>
#include <windows.h>
#include <shellapi.h>
#include <shlobj.h>
#include <ole2.h>

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

// Opens the clipboard. Another program (Explorer, right after Ctrl+C) may
// still hold it for a moment, so try again briefly before giving up.
bool OpenClipboardPatiently() {
  for (int i = 0; i < 20; ++i) {
    if (OpenClipboard(nullptr)) return true;
    Sleep(25);
  }
  return false;
}

// Appends the file paths in a DROPFILES block to `out`.
void AppendDropFiles(HDROP drop, flutter::EncodableList& out) {
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
}

// Files via the OLE clipboard. On Windows 11, File Explorer may copy files
// with OleSetClipboard and leave only a private "DataObject" format on the
// classic clipboard, so CF_HDROP is not listed there; the OLE data object
// still renders it on request.
flutter::EncodableList ReadFilesOle() {
  flutter::EncodableList out;
  OleInitialize(nullptr);  // no-op if this thread already has OLE/COM
  IDataObject* data_obj = nullptr;
  if (FAILED(OleGetClipboard(&data_obj)) || data_obj == nullptr) return out;
  FORMATETC fmt = {CF_HDROP, nullptr, DVASPECT_CONTENT, -1, TYMED_HGLOBAL};
  STGMEDIUM medium = {};
  if (SUCCEEDED(data_obj->GetData(&fmt, &medium))) {
    if (medium.tymed == TYMED_HGLOBAL && medium.hGlobal != nullptr) {
      HDROP drop = static_cast<HDROP>(GlobalLock(medium.hGlobal));
      if (drop != nullptr) {
        AppendDropFiles(drop, out);
        GlobalUnlock(medium.hGlobal);
      }
    }
    ReleaseStgMedium(&medium);
  }
  data_obj->Release();
  return out;
}

// Paths of the files on the clipboard; empty if it holds none.
flutter::EncodableList ReadFiles() {
  flutter::EncodableList out;
  if (IsClipboardFormatAvailable(CF_HDROP) && OpenClipboardPatiently()) {
    HANDLE data = GetClipboardData(CF_HDROP);
    if (data != nullptr) {
      HDROP drop = static_cast<HDROP>(GlobalLock(data));
      if (drop != nullptr) {
        AppendDropFiles(drop, out);
        GlobalUnlock(data);
      }
    }
    CloseClipboard();
  }
  if (out.empty()) out = ReadFilesOle();
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

  if (!OpenClipboardPatiently()) {
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
