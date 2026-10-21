// SPDX-License-Identifier: Apache-2.0
//
// Folder access for Schnipsle vaults on iOS.
//
// An iOS app may read a folder outside its sandbox (e.g. in iCloud Drive or on
// another file provider) only after the user picked it in the system picker, and
// only while it holds a security-scoped access. A bookmark lets the app regain
// that access after a restart without asking again. It is per-device state, so
// it lives in Application Support (excluded from backups: a bookmark only works
// on the device that made it), like the desktop's vault.json. Builds before M1
// kept it in UserDefaults; it is moved over the first time it is read.

import SwiftRs
import Tauri
import UIKit
import UniformTypeIdentifiers
import WebKit

struct PathArgs: Decodable {
  let path: String
}

/// Receives the picker's result and forwards it to a closure.
final class FolderPickerDelegate: NSObject, UIDocumentPickerDelegate {
  var onResult: ((URL?) -> Void)?

  func documentPicker(_ controller: UIDocumentPickerViewController, didPickDocumentsAt urls: [URL]) {
    onResult?(urls.first)
    onResult = nil
  }

  func documentPickerWasCancelled(_ controller: UIDocumentPickerViewController) {
    onResult?(nil)
    onResult = nil
  }
}

class VaultAccessPlugin: Plugin {
  /// Where builds before M1 kept the bookmark.
  private let legacyBookmarkKey = "schnipsle.vault.bookmark"
  private let bookmarkFileName = "vault.bookmark"
  private let pickerDelegate = FolderPickerDelegate()
  /// The folder we currently hold security-scoped access to.
  private var activeURL: URL?

  /// Shows the folder picker; resolves `{ path }` or `{ path: null }` if cancelled.
  @objc public func pickFolder(_ invoke: Invoke) {
    DispatchQueue.main.async {
      if self.pickerDelegate.onResult != nil {
        invoke.reject("A folder picker is already open")
        return
      }
      self.pickerDelegate.onResult = { url in
        guard let url = url else {
          invoke.resolve(["path": nil])
          return
        }
        do {
          self.activate(url)
          try self.saveBookmark(for: url)
          invoke.resolve(["path": url.path])
        } catch {
          invoke.reject("Could not keep access to the folder: \(error.localizedDescription)")
        }
      }
      let picker = UIDocumentPickerViewController(
        forOpeningContentTypes: [UTType.folder], asCopy: false)
      picker.delegate = self.pickerDelegate
      picker.allowsMultipleSelection = false
      picker.modalPresentationStyle = .formSheet
      guard let presenter = self.manager.viewController else {
        self.pickerDelegate.onResult = nil
        invoke.reject("No view controller to present the folder picker")
        return
      }
      presenter.present(picker, animated: true, completion: nil)
    }
  }

  /// Re-opens the stored folder; resolves `{ path }` or `{ path: null }`.
  @objc public func restoreFolder(_ invoke: Invoke) {
    guard let data = loadBookmark() else {
      invoke.resolve(["path": nil])
      return
    }
    do {
      var isStale = false
      let url = try URL(
        resolvingBookmarkData: data, options: [], relativeTo: nil, bookmarkDataIsStale: &isStale)
      activate(url)
      if isStale {
        // The folder moved or was renamed; refresh the bookmark while we have access.
        try saveBookmark(for: url)
      }
      invoke.resolve(["path": url.path])
    } catch {
      // The folder is gone or access was revoked: forget it, the user picks again.
      forgetBookmark()
      invoke.resolve(["path": nil])
    }
  }

  /// Asks iCloud to download a file that exists here only as a placeholder.
  @objc public func download(_ invoke: Invoke) throws {
    let args = try invoke.parseArgs(PathArgs.self)
    do {
      try FileManager.default.startDownloadingUbiquitousItem(
        at: URL(fileURLWithPath: args.path))
      invoke.resolve()
    } catch {
      invoke.reject("Download request failed: \(error.localizedDescription)")
    }
  }

  private func activate(_ url: URL) {
    if let previous = activeURL, previous != url {
      previous.stopAccessingSecurityScopedResource()
    }
    // Returns false for folders inside the app's own sandbox, which need no grant.
    _ = url.startAccessingSecurityScopedResource()
    activeURL = url
  }

  private func saveBookmark(for url: URL) throws {
    let data = try url.bookmarkData(
      options: [], includingResourceValuesForKeys: nil, relativeTo: nil)
    try writeBookmark(data)
  }

  /// `Library/Application Support/vault.bookmark`, creating the folder if needed.
  private func bookmarkFile() throws -> URL {
    let folder = try FileManager.default.url(
      for: .applicationSupportDirectory, in: .userDomainMask, appropriateFor: nil, create: true)
    return folder.appendingPathComponent(bookmarkFileName)
  }

  private func writeBookmark(_ data: Data) throws {
    var file = try bookmarkFile()
    try data.write(to: file, options: .atomic)
    var values = URLResourceValues()
    values.isExcludedFromBackup = true
    try? file.setResourceValues(values)
  }

  /// The stored bookmark; one still in UserDefaults is moved to the file.
  private func loadBookmark() -> Data? {
    if let file = try? bookmarkFile(), let data = try? Data(contentsOf: file) {
      return data
    }
    guard let legacy = UserDefaults.standard.data(forKey: legacyBookmarkKey) else {
      return nil
    }
    do {
      try writeBookmark(legacy)
      UserDefaults.standard.removeObject(forKey: legacyBookmarkKey)
    } catch {
      // Keep it where it is; it still works and is moved next time.
      Logger.error("bookmark not moved to Application Support: \(error.localizedDescription)")
    }
    return legacy
  }

  private func forgetBookmark() {
    if let file = try? bookmarkFile() {
      try? FileManager.default.removeItem(at: file)
    }
    UserDefaults.standard.removeObject(forKey: legacyBookmarkKey)
  }
}

@_cdecl("init_plugin_vault_access")
public func initPlugin() -> Plugin {
  return VaultAccessPlugin()
}
