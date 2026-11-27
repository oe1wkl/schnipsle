// SPDX-License-Identifier: Apache-2.0
//
// Where the default vault goes at the first start (ADR-0048 rules 2 and 4,
// amended by ADR-0050 rules 2 and 3).
//
// - iCloud Drive off for the app (`ubiquityIdentityToken` is nil), or no
//   container: the app's own Documents folder, at once.
// - Otherwise the Documents folder of the app's iCloud container, once
//   iCloud's answer can be trusted. NSMetadataQuery answers offline from what
//   the device already knows, so an empty result counts only if the device is
//   online and 5 more seconds of updates bring nothing, or if this device has
//   seen the container synced before. A result that holds a vault counts at
//   once.
//
// The query asks only for what the decision needs: items named `vault.yaml`
// and items whose name starts with "Schnipsle". The core reads the folders
// from their paths (`schnipsle_core::default_vault::Listed`).

import Foundation
import Network

/// What the first start may decide from.
enum FolderAnswer {
  /// iCloud Drive is off for the app: the app's own Documents folder.
  case local(URL)
  /// The container's Documents folder, with the paths the query listed in it.
  case iCloud(URL, [String])
  /// iCloud's answer cannot be trusted yet: offline, never synced here.
  case offline

  var json: [String: Any] {
    switch self {
    case .local(let url): return ["state": "local", "path": url.path]
    case .iCloud(let url, let listed): return ["state": "iCloud", "path": url.path, "listed": listed]
    case .offline: return ["state": "offline"]
    }
  }
}

/// Finds the folder for the default vault. Used from the main queue only.
final class DefaultFolder {
  static let containerID = "iCloud.cc.kraml.schnipsle"
  /// Set once a query here returned an uploaded item (ADR-0050 rule 3).
  static let syncedKey = "schnipsle.icloud.synced"
  /// Seconds of quiet the query must stay without a vault, online (rule 3).
  static let quiet: TimeInterval = 5

  private var documents: URL?
  private var query: NSMetadataQuery?
  private var observers: [NSObjectProtocol] = []
  private var monitor: NWPathMonitor?
  private var pathKnown = false
  private var online = false
  private var gathered = false
  private var listedCount = 0
  private var quietTimer: Timer?
  private var answer: FolderAnswer?
  /// Callers waiting; those not `untilReady` also take `.offline`.
  private var waiters: [(untilReady: Bool, done: (FolderAnswer) -> Void)] = []

  /// The app's own Documents folder, shown in Files under "On My iPhone".
  static func localDocuments() -> URL {
    FileManager.default.urls(for: .documentDirectory, in: .userDomainMask)[0]
  }

  /// Calls `done` on the main queue with where the default vault goes. With
  /// `untilReady` false it also answers `.offline`; with true it waits until
  /// iCloud's answer can be trusted. The query keeps running between calls.
  func find(untilReady: Bool, done: @escaping (FolderAnswer) -> Void) {
    // Rule 2: the token tells at once whether iCloud Drive is on for the app.
    if FileManager.default.ubiquityIdentityToken == nil {
      DispatchQueue.main.async { done(.local(Self.localDocuments())) }
      return
    }
    // The container may take a while the first time: not on the main queue.
    DispatchQueue.global(qos: .userInitiated).async {
      let container = FileManager.default.url(forUbiquityContainerIdentifier: Self.containerID)
      DispatchQueue.main.async {
        guard let container = container else {
          done(.local(Self.localDocuments()))
          return
        }
        if let answer = self.answer {
          done(answer)
          return
        }
        self.waiters.append((untilReady, done))
        self.start(in: container.appendingPathComponent("Documents", isDirectory: true))
        self.evaluate()
      }
    }
  }

  private func start(in documents: URL) {
    guard query == nil else { return }
    self.documents = documents
    let query = NSMetadataQuery()
    query.searchScopes = [NSMetadataQueryUbiquitousDocumentsScope]
    query.predicate = NSPredicate(
      format: "%K == %@ OR %K BEGINSWITH %@",
      NSMetadataItemFSNameKey, "vault.yaml", NSMetadataItemFSNameKey, "Schnipsle")
    let center = NotificationCenter.default
    observers.append(
      center.addObserver(
        forName: .NSMetadataQueryDidFinishGathering, object: query, queue: .main
      ) { [weak self] _ in
        self?.gathered = true
        self?.changed()
      })
    observers.append(
      center.addObserver(forName: .NSMetadataQueryDidUpdate, object: query, queue: .main) {
        [weak self] _ in self?.changed()
      })
    self.query = query
    query.start()

    let monitor = NWPathMonitor()
    monitor.pathUpdateHandler = { [weak self] path in
      DispatchQueue.main.async {
        guard let self = self else { return }
        self.pathKnown = true
        self.online = path.status == .satisfied
        self.evaluate()
      }
    }
    monitor.start(queue: .global(qos: .utility))
    self.monitor = monitor
  }

  /// After the query's first result or an update.
  private func changed() {
    guard gathered, let query = query else { return }
    query.disableUpdates()
    var uploaded = false
    for case let item as NSMetadataItem in query.results
    where item.value(forAttribute: NSMetadataUbiquitousItemIsUploadedKey) as? Bool == true {
      uploaded = true
      break
    }
    let count = query.resultCount
    query.enableUpdates()
    if uploaded {
      UserDefaults.standard.set(true, forKey: Self.syncedKey)
    }
    // Items still arriving: the 5 seconds start again.
    if count != listedCount, quietTimer != nil {
      quietTimer?.invalidate()
      quietTimer = nil
    }
    listedCount = count
    evaluate()
  }

  private func evaluate() {
    guard answer == nil, gathered, pathKnown else { return }
    let listed = listedPaths()
    if listed.contains(where: Self.isVaultFile)
      || UserDefaults.standard.bool(forKey: Self.syncedKey)
    {
      decide(listed)
    } else if online {
      if quietTimer == nil {
        quietTimer = Timer.scheduledTimer(withTimeInterval: Self.quiet, repeats: false) {
          [weak self] _ in
          guard let self = self else { return }
          self.decide(self.listedPaths())
        }
      }
    } else {
      quietTimer?.invalidate()
      quietTimer = nil
      let offline = waiters.filter { !$0.untilReady }
      waiters.removeAll { !$0.untilReady }
      for waiter in offline { waiter.done(.offline) }
    }
  }

  private func decide(_ listed: [String]) {
    guard answer == nil, let documents = documents else { return }
    let answer = FolderAnswer.iCloud(documents, listed)
    self.answer = answer
    quietTimer?.invalidate()
    quietTimer = nil
    query?.stop()
    for observer in observers { NotificationCenter.default.removeObserver(observer) }
    observers = []
    query = nil
    monitor?.cancel()
    monitor = nil
    let waiting = waiters
    waiters = []
    for waiter in waiting { waiter.done(answer) }
  }

  /// The query's items, as paths relative to the Documents folder.
  private func listedPaths() -> [String] {
    guard let query = query, let documents = documents else { return [] }
    let base = documents.resolvingSymlinksInPath().path + "/"
    query.disableUpdates()
    defer { query.enableUpdates() }
    var paths: [String] = []
    for case let item as NSMetadataItem in query.results {
      guard let path = item.value(forAttribute: NSMetadataItemPathKey) as? String else {
        continue
      }
      let full = URL(fileURLWithPath: path).resolvingSymlinksInPath().path
      if full.hasPrefix(base) {
        paths.append(String(full.dropFirst(base.count)))
      }
    }
    return paths
  }

  /// `Schnipsle…/.notesapp/vault.yaml`
  private static func isVaultFile(_ path: String) -> Bool {
    let parts = path.split(separator: "/")
    return parts.count == 3 && parts[0].hasPrefix("Schnipsle") && parts[1] == ".notesapp"
      && parts[2] == "vault.yaml"
  }
}
