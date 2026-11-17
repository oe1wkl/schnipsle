// SPDX-License-Identifier: Apache-2.0
//
// Rewrites each PNG given as an argument without an alpha channel, in place.
// App Store Connect rejects iOS app icons that have one, even a fully opaque one.
//
//   swift scripts/strip-png-alpha.swift <file.png>...

import CoreGraphics
import Foundation
import ImageIO
import UniformTypeIdentifiers

for path in CommandLine.arguments.dropFirst() {
    let url = URL(fileURLWithPath: path) as CFURL
    guard let source = CGImageSourceCreateWithURL(url, nil),
          let image = CGImageSourceCreateImageAtIndex(source, 0, nil),
          let space = CGColorSpace(name: CGColorSpace.sRGB),
          let context = CGContext(
              data: nil, width: image.width, height: image.height, bitsPerComponent: 8,
              bytesPerRow: 0, space: space, bitmapInfo: CGImageAlphaInfo.noneSkipLast.rawValue)
    else {
        fatalError("cannot read \(path)")
    }
    context.draw(image, in: CGRect(x: 0, y: 0, width: image.width, height: image.height))
    guard let opaque = context.makeImage(),
          let destination = CGImageDestinationCreateWithURL(
              url, UTType.png.identifier as CFString, 1, nil)
    else {
        fatalError("cannot write \(path)")
    }
    CGImageDestinationAddImage(destination, opaque, nil)
    guard CGImageDestinationFinalize(destination) else {
        fatalError("cannot write \(path)")
    }
}
