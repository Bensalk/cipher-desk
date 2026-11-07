#ifndef RUNNER_FILE_CLIPBOARD_H_
#define RUNNER_FILE_CLIPBOARD_H_

#include <flutter/binary_messenger.h>
#include <flutter/encodable_value.h>
#include <flutter/method_channel.h>

#include <memory>

// Files on the Windows clipboard (what Explorer copies and pastes), for copy
// and paste of files in a remote session, like AnyDesk. Method channel
// "rdpv/file_clipboard":
//   sequence()          -> int   clipboard change counter
//   readFiles()         -> list  paths of copied files (empty if none)
//   writeFiles([paths]) -> int   puts the files on the clipboard (Ctrl+V in
//                                Explorer copies them); returns the counter
//                                right after, so the app can ignore its own
//                                change.
std::unique_ptr<flutter::MethodChannel<flutter::EncodableValue>>
CreateFileClipboardChannel(flutter::BinaryMessenger* messenger);

#endif  // RUNNER_FILE_CLIPBOARD_H_
