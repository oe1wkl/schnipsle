// SPDX-License-Identifier: Apache-2.0
//! Turns a picture many programs cannot show, such as HEIC or TIFF, into a JPEG
//! or, if it has transparency, a PNG (ADR-0027), and tells a picture that only
//! its orientation turns upright, which office programs ignore (ADR-0028). `ImageIO` reads and writes it on
//! any thread, on macOS and iOS alike. The bindings of `objc2` do not cover it yet,
//! so its few functions are declared here.

use std::ptr::NonNull;

use objc2_core_foundation::{
    CFBoolean, CFData, CFDictionary, CFMutableData, CFNumber, CFRetained, CFString, CFType,
};

/// JPEG quality; a photo stays free of visible artefacts at about a fifth of the size of TIFF.
const QUALITY: f64 = 0.9;

#[link(name = "ImageIO", kind = "framework")]
unsafe extern "C-unwind" {
    static kCGImageSourceCreateThumbnailFromImageAlways: &'static CFString;
    static kCGImageSourceCreateThumbnailWithTransform: &'static CFString;
    static kCGImageDestinationLossyCompressionQuality: &'static CFString;
    static kCGImagePropertyHasAlpha: &'static CFString;
    static kCGImagePropertyOrientation: &'static CFString;
    fn CGImageSourceCreateWithData(
        data: &CFData,
        options: Option<&CFType>,
    ) -> Option<NonNull<CFType>>;
    fn CGImageSourceCopyPropertiesAtIndex(
        source: &CFType,
        index: usize,
        options: Option<&CFType>,
    ) -> Option<NonNull<CFDictionary<CFString, CFType>>>;
    fn CGImageSourceCreateThumbnailAtIndex(
        source: &CFType,
        index: usize,
        options: Option<&CFType>,
    ) -> Option<NonNull<CFType>>;
    fn CGImageDestinationCreateWithData(
        data: &CFMutableData,
        kind: &CFString,
        count: usize,
        options: Option<&CFType>,
    ) -> Option<NonNull<CFType>>;
    fn CGImageDestinationAddImage(
        destination: &CFType,
        image: &CFType,
        properties: Option<&CFType>,
    );
    fn CGImageDestinationFinalize(destination: &CFType) -> bool;
}

/// A picture in a format that programs show.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConvertedPicture {
    /// The file.
    pub bytes: Vec<u8>,
    /// Its media type.
    pub media_type: &'static str,
    /// Its extension, without a dot.
    pub extension: &'static str,
}

/// `bytes` as a JPEG, or a PNG if the picture has transparency, upright and at
/// its full size. `None` if this is not a picture the system can read.
#[must_use]
pub fn convert_picture(bytes: &[u8]) -> Option<ConvertedPicture> {
    let source = source_of(bytes)?;
    let image = upright_image(&source)?;
    let (kind, media_type, extension) = if has_transparency(&source) {
        ("public.png", "image/png", "png")
    } else {
        ("public.jpeg", "image/jpeg", "jpg")
    };
    Some(ConvertedPicture {
        bytes: encoded(&image, kind)?,
        media_type,
        extension,
    })
}

/// Whether `bytes` are a picture whose orientation says to show it turned or
/// mirrored: its pixels are not upright as they are stored.
#[must_use]
pub fn is_turned(bytes: &[u8]) -> bool {
    let Some(source) = source_of(bytes) else {
        return false;
    };
    // SAFETY: a constant of the framework, valid for the whole program.
    let key = unsafe { kCGImagePropertyOrientation };
    property(&source, key)
        .and_then(|value| value.downcast_ref::<CFNumber>().and_then(CFNumber::as_i64))
        .is_some_and(|orientation| orientation != 1)
}

/// Takes over what a `Create` function returned.
fn owned(pointer: Option<NonNull<CFType>>) -> Option<CFRetained<CFType>> {
    // SAFETY: only called with the result of a `Create` function, which the caller owns.
    pointer.map(|pointer| unsafe { CFRetained::from_raw(pointer) })
}

fn source_of(bytes: &[u8]) -> Option<CFRetained<CFType>> {
    let data = CFData::from_bytes(bytes);
    // SAFETY: `data` is a valid `CFData`, and options are optional.
    owned(unsafe { CGImageSourceCreateWithData(&data, None) })
}

/// The picture with its orientation applied: other programs may ignore that
/// property. A "thumbnail" without a maximum size is the size of the picture.
fn upright_image(source: &CFType) -> Option<CFRetained<CFType>> {
    // SAFETY: a constant of the framework, valid for the whole program.
    let always = unsafe { kCGImageSourceCreateThumbnailFromImageAlways };
    // SAFETY: a constant of the framework, valid for the whole program.
    let transform = unsafe { kCGImageSourceCreateThumbnailWithTransform };
    let options = CFDictionary::from_slices(
        &[always, transform],
        &[CFBoolean::new(true), CFBoolean::new(true)],
    );
    let options: &CFType = &options;
    // SAFETY: `source` is an image source, and `options` a dictionary as documented.
    owned(unsafe { CGImageSourceCreateThumbnailAtIndex(source, 0, Some(options)) })
}

/// What the file says. The image read from it cannot tell: one from a HEIC has
/// an alpha channel even if every pixel is opaque.
fn has_transparency(source: &CFType) -> bool {
    // SAFETY: a constant of the framework, valid for the whole program.
    let key = unsafe { kCGImagePropertyHasAlpha };
    property(source, key)
        .and_then(|value| value.downcast_ref::<CFBoolean>().map(CFBoolean::as_bool))
        .unwrap_or(false)
}

/// A property the file gives for its first picture.
fn property(source: &CFType, key: &CFString) -> Option<CFRetained<CFType>> {
    // SAFETY: `source` is an image source, and options are optional.
    let properties = unsafe { CGImageSourceCopyPropertiesAtIndex(source, 0, None) }?;
    // SAFETY: the result of a `Copy` function, which the caller owns.
    let properties = unsafe { CFRetained::from_raw(properties) };
    properties.get(key)
}

/// `image` as a file of the type `kind`.
fn encoded(image: &CFType, kind: &str) -> Option<Vec<u8>> {
    let data = CFMutableData::new(None, 0)?;
    let kind = CFString::from_str(kind);
    // SAFETY: `data` and `kind` are valid and outlive the call; one image follows.
    let destination = owned(unsafe { CGImageDestinationCreateWithData(&data, &kind, 1, None) })?;
    // SAFETY: a constant of the framework, valid for the whole program.
    let quality_key = unsafe { kCGImageDestinationLossyCompressionQuality };
    let quality = CFNumber::new_f64(QUALITY);
    let properties = CFDictionary::from_slices(&[quality_key], &[&*quality]);
    let properties: &CFType = &properties;
    // SAFETY: `destination` is an image destination, `image` a `CGImage`.
    unsafe { CGImageDestinationAddImage(&destination, image, Some(properties)) };
    // SAFETY: `destination` holds the one image it was created for.
    let finished = unsafe { CGImageDestinationFinalize(&destination) };
    finished.then(|| data.to_vec())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[link(name = "CoreGraphics", kind = "framework")]
    unsafe extern "C-unwind" {
        fn CGImageGetWidth(image: &CFType) -> usize;
        fn CGImageGetHeight(image: &CFType) -> usize;
    }

    /// An uncompressed little-endian TIFF, all white, without or with an alpha channel.
    fn tiff(width: u32, height: u32, orientation: u32, alpha: bool) -> Vec<u8> {
        let samples: u32 = if alpha { 4 } else { 3 };
        let pixels = (width * height * samples) as usize;
        let mut tags: Vec<(u16, u16, u32)> = vec![
            (256, 4, width),
            (257, 4, height),
            (258, 3, 8),
            (259, 3, 1),
            (262, 3, 2),
            (273, 4, 8),
            (274, 3, orientation),
            (277, 3, samples),
            (278, 4, height),
            (279, 4, u32::try_from(pixels).unwrap()),
        ];
        if alpha {
            tags.push((338, 3, 2));
        }
        let mut file = b"II*\0".to_vec();
        let directory = 8 + pixels + (8 * samples as usize);
        file.extend(u32::try_from(directory).unwrap().to_le_bytes());
        file.extend(vec![0xff; pixels]);
        // BitsPerSample has one value per sample, so it lives outside the directory.
        let bits_at = u32::try_from(file.len()).unwrap();
        file.extend([8u8, 0].repeat(samples as usize * 4));
        tags[2] = (258, 3, bits_at);
        file.truncate(directory);
        file.extend(u16::try_from(tags.len()).unwrap().to_le_bytes());
        for (tag, kind, value) in tags {
            file.extend(tag.to_le_bytes());
            file.extend(kind.to_le_bytes());
            let count: u32 = if tag == 258 { samples } else { 1 };
            file.extend(count.to_le_bytes());
            file.extend(value.to_le_bytes());
        }
        file.extend(0u32.to_le_bytes());
        file
    }

    fn size(bytes: &[u8]) -> (usize, usize) {
        let image = upright_image(&source_of(bytes).unwrap()).unwrap();
        // SAFETY: `image` is a `CGImage`.
        let width = unsafe { CGImageGetWidth(&image) };
        // SAFETY: `image` is a `CGImage`.
        let height = unsafe { CGImageGetHeight(&image) };
        (width, height)
    }

    #[test]
    fn a_tiff_becomes_a_jpeg_of_its_full_size() {
        let picture = convert_picture(&tiff(600, 400, 1, false)).unwrap();
        assert_eq!(picture.media_type, "image/jpeg");
        assert_eq!(picture.extension, "jpg");
        assert_eq!(&picture.bytes[..2], [0xff, 0xd8]);
        assert_eq!(size(&picture.bytes), (600, 400));
    }

    #[test]
    fn a_turned_picture_comes_out_upright() {
        // Orientation 6: stored on its side, shown turned by 90 degrees.
        let picture = convert_picture(&tiff(600, 400, 6, false)).unwrap();
        assert_eq!(size(&picture.bytes), (400, 600));
    }

    #[test]
    fn a_picture_turned_by_its_orientation_is_told() {
        assert!(is_turned(&tiff(60, 40, 6, false)));
        assert!(is_turned(&tiff(60, 40, 2, false)));
        assert!(!is_turned(&tiff(60, 40, 1, false)));
        assert!(!is_turned(b"this is no picture"));
    }

    #[test]
    fn transparency_makes_a_png() {
        let picture = convert_picture(&tiff(20, 10, 1, true)).unwrap();
        assert_eq!(picture.media_type, "image/png");
        assert_eq!(picture.extension, "png");
        assert_eq!(&picture.bytes[1..4], b"PNG");
    }

    #[test]
    fn a_heic_becomes_a_jpeg_where_the_system_can_write_one_to_try() {
        let image = upright_image(&source_of(&tiff(64, 64, 1, false)).unwrap()).unwrap();
        let Some(heic) = encoded(&image, "public.heic") else {
            return;
        };
        let picture = convert_picture(&heic).unwrap();
        assert_eq!(picture.media_type, "image/jpeg");
        assert_eq!(size(&picture.bytes), (64, 64));
    }

    #[test]
    fn what_is_no_picture_gives_nothing() {
        assert_eq!(convert_picture(b"this is no picture"), None);
        assert_eq!(convert_picture(b""), None);
    }
}
