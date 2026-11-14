// SPDX-License-Identifier: Apache-2.0
// Prototype of docs/spikes/ios-pdf.md: swiftc -O ios-pdf-relink.swift -o relink
// ./relink printed.pdf page.html out.pdf (out.pdf shows PDFKit's faulty internal links)
// Spike prototype: puts the links of an HTML page back into the PDF its print
// path made, by finding their texts on the pages in document order.
import Foundation
import PDFKit

let args = CommandLine.arguments
let doc = PDFDocument(url: URL(fileURLWithPath: args[1]))!
let html = try! String(contentsOfFile: args[2], encoding: .utf8)

func plain(_ s: Substring) -> String {
    var t = s.replacingOccurrences(of: "<[^>]*>", with: " ", options: .regularExpression)
    for (e, c) in [("&amp;", "&"), ("&lt;", "<"), ("&gt;", ">"), ("&quot;", "\""), ("&#39;", "'"), ("&nbsp;", " ")] {
        t = t.replacingOccurrences(of: e, with: c)
    }
    return t.replacingOccurrences(of: "\\s+", with: " ", options: .regularExpression)
        .trimmingCharacters(in: .whitespaces)
}

enum Event { case link(href: String, text: String), target(id: String, text: String) }
var events: [Event] = []
let tag = try! NSRegularExpression(pattern: "<(a|[a-z0-9]+)\\b([^>]*)>", options: [.caseInsensitive])
let body = html.range(of: "<body")!.lowerBound
let ns = html as NSString
for m in tag.matches(in: html, range: NSRange(body.utf16Offset(in: html)..<ns.length)) {
    let name = ns.substring(with: m.range(at: 1)).lowercased()
    let attrs = ns.substring(with: m.range(at: 2))
    let after = m.range.location + m.range.length
    func attr(_ a: String) -> String? {
        guard let r = attrs.range(of: "\(a)=\"([^\"]*)\"", options: .regularExpression) else { return nil }
        return String(attrs[r].dropFirst(a.count + 2).dropLast())
    }
    if let id = attr("id") {
        // The text after the tag: up to the next 4 words, tags (pictures too) left out.
        var rest = ns.substring(with: NSRange(location: after, length: min(200_000, ns.length - after)))
        // Only the first block with text: a heading, a paragraph.
        rest = rest.replacingOccurrences(of: "</(h[1-6]|p|li|td|th|div|pre)>.*", with: "", options: [.regularExpression, .caseInsensitive])
        rest = rest.replacingOccurrences(of: "<[^>]*>", with: " ", options: .regularExpression)
        rest = String(rest.prefix(400))
        let words = plain(Substring(rest)).split(separator: " ").prefix(4).joined(separator: " ")
        if !words.isEmpty { events.append(.target(id: id, text: words)) }
    }
    if name == "a", let href = attr("href") {
        let close = ns.range(of: "</a>", range: NSRange(location: after, length: ns.length - after))
        let text = plain(Substring(ns.substring(with: NSRange(location: after, length: close.location - after))))
        events.append(.link(href: href, text: text))
    }
}

/// A place in the text of the document: a page and a character in its text.
struct Place: Comparable { var page: Int; var at: Int
    static func < (a: Place, b: Place) -> Bool { (a.page, a.at) < (b.page, b.at) } }
let texts = (0..<doc.pageCount).map { (doc.page(at: $0)!.string ?? "") as NSString }

/// `text` at or after `from`, its words apart by any white space (a line break in
/// the PDF where the HTML has a space).
func find(_ text: String, from: Place) -> (PDFSelection, Place, Place)? {
    let words = text.split(separator: " ").map { NSRegularExpression.escapedPattern(for: String($0)) }
    guard !words.isEmpty, let re = try? NSRegularExpression(pattern: words.joined(separator: "\\s+")) else { return nil }
    for p in from.page..<doc.pageCount {
        let t = texts[p]
        let begin = p == from.page ? from.at : 0
        guard begin <= t.length, let m = re.firstMatch(in: t as String, range: NSRange(begin..<t.length)) else { continue }
        guard let sel = doc.page(at: p)!.selection(for: m.range) else { return nil }
        return (sel, Place(page: p, at: m.range.location), Place(page: p, at: m.range.location + m.range.length))
    }
    return nil
}

var last = Place(page: 0, at: 0)
var targets: [String: (PDFPage, CGPoint)] = [:]
var found: [(PDFSelection, String)] = []
var missed: [String] = []
for e in events {
    switch e {
    case let .target(id, text):
        // A target is looked for from the last link or target, without moving on,
        // as its text is often the next link's too.
        if let (s, at, _) = find(text, from: last), let page = s.pages.first {
            _ = at
            let b = s.bounds(for: page)
            targets[id] = (page, CGPoint(x: b.minX, y: b.maxY + 12))
        } else { missed.append("target \(id) [\(text)]") }
    case let .link(href, text):
        if text.isEmpty { continue }
        if let (s, _, end) = find(text, from: last) { found.append((s, href)); last = end }
        else { missed.append("link \(href) [\(text)]") }
    }
}
var made = 0
for (sel, href) in found {
    let lines = sel.selectionsByLine()
    for line in lines { for page in line.pages {
        let b = line.bounds(for: page)
        var to = href
        if href.hasPrefix("#"), let (tp, pt) = targets[String(href.dropFirst())] { to = "p\(doc.index(for: tp) + 1) y \(Int(pt.y))" }
        print("p\(doc.index(for: page) + 1) \(Int(b.minX)),\(Int(b.minY)) \(Int(b.width))x\(Int(b.height)) [\(line.string ?? "")] -> \(to)")
    } }
    for line in sel.selectionsByLine() {
        for page in line.pages {
            let a = PDFAnnotation(bounds: line.bounds(for: page), forType: .link, withProperties: nil)
            if href.hasPrefix("#") {
                guard let (tp, pt) = targets[String(href.dropFirst())] else { missed.append("no target \(href)"); continue }
                a.destination = PDFDestination(page: tp, at: pt)
            } else {
                a.url = URL(string: href)
            }
            page.addAnnotation(a); made += 1
        }
    }
}
let start = Date()
doc.write(to: URL(fileURLWithPath: args[3]))
print("events \(events.count), links made \(made), missed \(missed.count) in \(String(format: "%.3f", -start.timeIntervalSinceNow)) s")
for m in missed { print("  missed \(m)") }
