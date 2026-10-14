// SPDX-License-Identifier: Apache-2.0
//
// Folder access for Schnipsle vaults on iOS.
//
// An iOS app may read a folder outside its sandbox (e.g. in iCloud Drive or on
// another file provider) only after the user picked it in the system picker, and
// only while it holds a security-scoped access. A bookmark stored in UserDefaults
// lets the app regain that access after a restart without asking again.

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
  private let bookmarkKey = "schnipsle.vault.bookmark"
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
    guard let data = UserDefaults.standard.data(forKey: bookmarkKey) else {
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
      UserDefaults.standard.removeObject(forKey: bookmarkKey)
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
    UserDefaults.standard.set(data, forKey: bookmarkKey)
  }
}

@_cdecl("init_plugin_vault_access")
public func initPlugin() -> Plugin {
  return VaultAccessPlugin()
}
