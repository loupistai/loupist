use crate::app::CommandError;

#[derive(Debug, PartialEq, Eq, serde::Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(tag = "status", rename_all = "camelCase")]
pub(crate) enum ClipboardImage {
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    Image {
        #[serde(rename = "dataBase64")]
        data_base64: String,
    },
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    NoImage,
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    TooLarge,
    #[cfg_attr(target_os = "macos", allow(dead_code))]
    Unavailable,
}

#[tauri::command]
pub(crate) fn clipboard_read_image(
    webview: tauri::Webview,
) -> Result<ClipboardImage, CommandError> {
    if webview.label() != "main" || webview.window().label() != "main" {
        return Err(CommandError::invalid(
            "clipboard images are available only to the main webview",
        ));
    }
    #[cfg(target_os = "macos")]
    return Ok(macos::read_image(
        &objc2_app_kit::NSPasteboard::generalPasteboard(),
    ));
    #[cfg(not(target_os = "macos"))]
    Ok(ClipboardImage::Unavailable)
}

#[cfg(target_os = "macos")]
mod macos {
    use base64::{engine::general_purpose::STANDARD, Engine as _};
    use objc2_app_kit::{
        NSBitmapImageFileType, NSBitmapImageRep, NSPasteboard, NSPasteboardTypePNG,
        NSPasteboardTypeTIFF,
    };
    use objc2_foundation::{NSData, NSDictionary, NSString};

    use super::ClipboardImage;
    use crate::commands::attachments::MAX_CHAT_IMAGE_ATTACHMENT_BYTES;

    pub(super) fn read_image(pasteboard: &NSPasteboard) -> ClipboardImage {
        // SAFETY: AppKit provides both type constants for the process lifetime.
        if let Some(png) = pasteboard.dataForType(unsafe { NSPasteboardTypePNG }) {
            return encode_png(&png);
        }
        let Some(tiff) = pasteboard.dataForType(unsafe { NSPasteboardTypeTIFF }) else {
            return ClipboardImage::NoImage;
        };
        if tiff.length() > MAX_CHAT_IMAGE_ATTACHMENT_BYTES {
            return ClipboardImage::TooLarge;
        }
        let Some(bitmap) = NSBitmapImageRep::imageRepWithData(&tiff) else {
            return ClipboardImage::NoImage;
        };
        // SAFETY: The dictionary is empty, so it contains no incompatible properties.
        let png = unsafe {
            bitmap.representationUsingType_properties(
                NSBitmapImageFileType::PNG,
                &NSDictionary::<NSString>::new(),
            )
        };
        png.as_deref().map_or(ClipboardImage::NoImage, encode_png)
    }

    fn encode_png(png: &NSData) -> ClipboardImage {
        if png.length() > MAX_CHAT_IMAGE_ATTACHMENT_BYTES {
            return ClipboardImage::TooLarge;
        }
        let bytes = png.to_vec();
        if !bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
            return ClipboardImage::NoImage;
        }
        ClipboardImage::Image {
            data_base64: STANDARD.encode(bytes),
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn clipboard_read_image_prefers_png_converts_tiff_and_bounds_bytes() {
            // A private pasteboard keeps the test independent of the user's clipboard.
            let board = NSPasteboard::pasteboardWithUniqueName();
            assert_eq!(read_image(&board), ClipboardImage::NoImage);
            let png_base64 = "iVBORw0KGgoAAAANSUhEUgAAAAIAAAACCAIAAAD91JpzAAAAEklEQVR4nGP4z8DAAMIM/4EAAB/uBfsL2WiLAAAAAElFTkSuQmCC";
            let png = NSData::from_vec(STANDARD.decode(png_base64).unwrap());
            let tiff = NSBitmapImageRep::imageRepWithData(&png)
                .unwrap()
                .TIFFRepresentation()
                .unwrap();
            // SAFETY: AppKit provides both type constants for the process lifetime.
            let (png_type, tiff_type) = unsafe { (NSPasteboardTypePNG, NSPasteboardTypeTIFF) };
            board.clearContents();
            assert!(board.setData_forType(Some(&tiff), tiff_type));
            let ClipboardImage::Image { data_base64 } = read_image(&board) else {
                panic!("expected PNG converted from TIFF");
            };
            assert!(STANDARD
                .decode(data_base64)
                .unwrap()
                .starts_with(b"\x89PNG"));
            assert!(board.setData_forType(Some(&png), png_type));
            assert_eq!(
                read_image(&board),
                ClipboardImage::Image {
                    data_base64: png_base64.into()
                }
            );
            let oversized = NSData::from_vec(vec![0; MAX_CHAT_IMAGE_ATTACHMENT_BYTES + 1]);
            for kind in [png_type, tiff_type] {
                board.clearContents();
                assert!(board.setData_forType(Some(&oversized), kind));
                assert_eq!(read_image(&board), ClipboardImage::TooLarge);
            }
            board.clearContents();
        }
    }
}
