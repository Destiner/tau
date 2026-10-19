use super::*;
use gpui_kit::{Image, SvgRenderer};
use std::sync::Arc;

// Valid GIFs with a 1x1 subframe on a configurable logical canvas. The decoder
// composites each subframe to the full canvas, so tiny input has large output.
fn animation(width: u16, height: u16, frames: usize) -> Vec<u8> {
    let mut bytes = b"GIF89a".to_vec();
    bytes.extend(width.to_le_bytes());
    bytes.extend(height.to_le_bytes());
    bytes.extend([0x80, 0, 0, 0, 0, 0, 255, 255, 255]);
    for _ in 0..frames {
        bytes.extend([
            0x21, 0xf9, 4, 0, 1, 0, 0, 0, // graphics control, 10ms delay
            0x2c, 0, 0, 0, 0, 1, 0, 1, 0, 0, // 1x1 image descriptor
            2, 2, 0x44, 0x01, 0, // clear, pixel, end LZW codes
        ]);
    }
    bytes.push(0x3b);
    bytes
}
fn accepted(format: ImageFormat, bytes: &[u8]) -> bool {
    admit(format, bytes, &AtomicBool::new(false)).is_some()
}

#[test]
fn valid_small_gif_preserves_all_animation_frames() {
    let bytes = animation(2, 2, 3);
    assert!(accepted(ImageFormat::Gif, &bytes));
    let image = Image::from_bytes(ImageFormat::Gif, bytes).with_decode_limits(DECODE_LIMITS);
    let decoded = image.to_image_data(SvgRenderer::new(Arc::new(()))).unwrap();
    assert_eq!(decoded.frame_count(), 3);
    assert_eq!(decoded.as_bytes(0).unwrap().len(), 2 * 2 * 4);
}

#[test]
fn compact_valid_gif_canvas_and_cumulative_frames_are_bounded_before_decode() {
    let giant = animation(4096, 4096, 1);
    assert!(giant.len() < 64 * 1024 * 1024);
    assert!(!accepted(ImageFormat::Gif, &giant));
    let cumulative = animation(1024, 1024, 9);
    assert!(cumulative.len() < 1024);
    assert!(!accepted(ImageFormat::Gif, &cumulative));
    assert!(accepted(ImageFormat::Gif, &animation(1024, 1024, 8)));
}

#[test]
fn frame_count_is_independently_bounded() {
    assert!(accepted(ImageFormat::Gif, &animation(1, 1, 256)));
    let adversarial = animation(1, 1, 10_000);
    assert!(adversarial.len() < 1024 * 1024);
    assert!(!accepted(ImageFormat::Gif, &adversarial));
}

#[test]
fn oversized_subframe_cannot_hide_behind_small_canvas() {
    let mut bytes = animation(1, 1, 1);
    let descriptor = bytes.iter().position(|byte| *byte == 0x2c).unwrap();
    bytes[descriptor + 5..descriptor + 7].copy_from_slice(&4096_u16.to_le_bytes());
    bytes[descriptor + 7..descriptor + 9].copy_from_slice(&4096_u16.to_le_bytes());
    assert!(!accepted(ImageFormat::Gif, &bytes));
}

fn png_header(width: u32, height: u32) -> Vec<u8> {
    let mut bytes = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR".to_vec();
    bytes.extend(width.to_be_bytes());
    bytes.extend(height.to_be_bytes());
    bytes
}
#[test]
fn static_dimensions_reject_before_any_pixel_allocation() {
    assert!(accepted(ImageFormat::Png, &png_header(1024, 1024)));
    assert!(!accepted(ImageFormat::Png, &png_header(4096, 4096)));
    assert!(!accepted(ImageFormat::Png, &png_header(8193, 1)));
    assert!(!accepted(ImageFormat::Png, &png_header(u32::MAX, u32::MAX)));
    let mut jpeg = vec![0xff, 0xd8, 0xff, 0xc0, 0, 7, 8];
    jpeg.extend(4096_u16.to_be_bytes());
    jpeg.extend(4096_u16.to_be_bytes());
    jpeg.extend([0xff, 0xda]);
    assert!(!accepted(ImageFormat::Jpeg, &jpeg));
    let mut bmp = vec![0; 54];
    bmp[..2].copy_from_slice(b"BM");
    bmp[14..18].copy_from_slice(&40_u32.to_le_bytes());
    bmp[18..22].copy_from_slice(&4096_i32.to_le_bytes());
    bmp[22..26].copy_from_slice(&(-4096_i32).to_le_bytes());
    assert!(!accepted(ImageFormat::Bmp, &bmp));
}

#[test]
fn jpeg_cannot_change_dimensions_after_an_entropy_scan() {
    let mut bytes = vec![0xff, 0xd8, 0xff, 0xc0, 0, 7, 8, 0, 2, 0, 2];
    bytes.extend([0xff, 0xda, 0, 2, 0x42, 0xff, 0, 0xff, 0xd0]);
    bytes.extend([0xff, 0xc0, 0, 7, 8, 0x10, 0, 0x10, 0, 0xff, 0xd9]);
    assert!(!accepted(ImageFormat::Jpeg, &bytes));
}

#[test]
fn webp_and_ico_inspect_actual_dimensions() {
    let mut webp = b"RIFF\0\0\0\0WEBPVP8L\x05\0\0\0\x2f".to_vec();
    webp.extend((4095_u32 | (4095_u32 << 14)).to_le_bytes());
    webp.push(0);
    assert!(!accepted(ImageFormat::Webp, &webp));
    let png = png_header(4096, 4096);
    let mut ico = vec![0, 0, 1, 0, 1, 0, 1, 1, 0, 0, 1, 0, 32, 0];
    ico.extend((png.len() as u32).to_le_bytes());
    ico.extend(22_u32.to_le_bytes());
    ico.extend(png);
    assert!(!accepted(ImageFormat::Ico, &ico));
}

#[test]
fn svg_xml_excludes_embedded_raster_and_leaves_size_to_actual_renderer() {
    assert!(accepted(
        ImageFormat::Svg,
        br#"<svg xmlns="http://www.w3.org/2000/svg" width="2" height="2"/>"#
    ));
    let huge = br#"<svg xmlns="http://www.w3.org/2000/svg" width="2048" height="2048"/>"#;
    assert!(accepted(ImageFormat::Svg, huge));
    let image =
        Image::from_bytes(ImageFormat::Svg, huge.to_vec()).with_decode_limits(DECODE_LIMITS);
    assert!(image.to_image_data(SvgRenderer::new(Arc::new(()))).is_err());
    assert!(!accepted(ImageFormat::Svg, br#"<svg xmlns="http://www.w3.org/2000/svg" width="2" height="2"><image href="data:image/png;base64,AAAA"/></svg>"#));
}

#[test]
fn svg_doctype_and_font_sizing_use_bounded_real_decoder() {
    let doctype = r#"<?xml version="1.0"?>
<!DOCTYPE svg PUBLIC "-//W3C//DTD SVG 1.1//EN" "http://www.w3.org/Graphics/SVG/1.1/DTD/svg11.dtd">"#;
    for body in [
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="2" height="2"><rect width="2" height="2"/></svg>"#,
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="100" height="100"><text x="10" y="30" font-family="Arial" font-size="16">A</text></svg>"#,
        r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 100 100"><text x="10" y="30" font-size="16">A</text></svg>"#,
        r#"<svg xmlns="http://www.w3.org/2000/svg" font-size="16"><text x="10" y="30" font-family="Arial">A</text></svg>"#,
    ] {
        let bytes = format!("{doctype}{body}").into_bytes();
        assert!(accepted(ImageFormat::Svg, &bytes));
        let image = Image::from_bytes(ImageFormat::Svg, bytes).with_decode_limits(DECODE_LIMITS);
        for _ in 0..2 {
            let decoded = image.to_image_data(SvgRenderer::new(Arc::new(()))).unwrap();
            assert!(decoded.as_bytes(0).unwrap().len() <= MAX_BYTES as usize);
        }
    }
    for body in [
        r#"<svg xmlns="http://www.w3.org/2000/svg"><image href="data:image/png;base64,AAAA"/></svg>"#,
        r#"<svg xmlns="http://www.w3.org/2000/svg"><filter><feImage href="file:///missing.png"/></filter></svg>"#,
        r#"<svg xmlns="http://www.w3.org/2000/svg">&external;</svg>"#,
    ] {
        assert!(!accepted(
            ImageFormat::Svg,
            format!("{doctype}{body}").as_bytes()
        ));
    }
    let entity = br#"<!DOCTYPE svg [<!ENTITY external SYSTEM "file:///missing">]><svg xmlns="http://www.w3.org/2000/svg">&external;</svg>"#;
    assert!(!accepted(ImageFormat::Svg, entity));
}

#[test]
fn actual_gif_iterator_enforces_cumulative_and_frame_limits() {
    for bytes in [animation(1024, 1024, 9), animation(1, 1, 10_000)] {
        let image = Image::from_bytes(ImageFormat::Gif, bytes).with_decode_limits(DECODE_LIMITS);
        assert!(image.to_image_data(SvgRenderer::new(Arc::new(()))).is_err());
    }
    let image =
        Image::from_bytes(ImageFormat::Gif, animation(1, 1, 256)).with_decode_limits(DECODE_LIMITS);
    assert_eq!(
        image
            .to_image_data(SvgRenderer::new(Arc::new(())))
            .unwrap()
            .frame_count(),
        256
    );
}

#[test]
fn actual_bounded_gif_decoder_limit_error_fails_closed_without_retry_loop() {
    let mut bytes = animation(1, 1, 1);
    let descriptor = bytes.iter().position(|byte| *byte == 0x2c).unwrap();
    bytes[descriptor + 5..descriptor + 7].copy_from_slice(&2_u16.to_le_bytes());
    bytes[descriptor + 7..descriptor + 9].copy_from_slice(&2_u16.to_le_bytes());
    let image = Image::from_bytes(ImageFormat::Gif, bytes).with_decode_limits(
        gpui_kit::ImageDecodeLimits {
            max_bytes: 4,
            max_frames: 256,
            max_axis: 8192,
        },
    );
    assert!(image.to_image_data(SvgRenderer::new(Arc::new(()))).is_err());
}

#[test]
fn malformed_and_cancelled_admission_fail_closed() {
    let gif = animation(2, 2, 2);
    for length in 0..gif.len() {
        assert!(!accepted(ImageFormat::Gif, &gif[..length]));
    }
    assert!(admit(ImageFormat::Gif, &gif, &AtomicBool::new(true)).is_none());
    assert!(admit(ImageFormat::Png, &png_header(2, 2), &AtomicBool::new(true)).is_none());
    let mut input = Input {
        bytes: &gif,
        at: 0,
        cancelled: &AtomicBool::new(false),
    };
    input.take(6).unwrap();
    input.cancelled.store(true, Ordering::Relaxed);
    assert!(input.take(1).is_none());
}

#[test]
fn load_snapshot_svg_uses_real_budget_and_retains_it_for_display() {
    use super::super::{PreviewContext, PreviewOwner, PreviewResult, load_snapshot};
    use tau_core::file_preview::{PreparedPreview, PreviewService};
    let directory = tempfile::tempdir().unwrap();
    let context = PreviewContext {
        session: "svg-budget".into(),
        source: "exact-source".into(),
        base_path: directory.path().to_string_lossy().into_owned(),
        remote_project: None,
        document_source_path: None,
        project_root: None,
    };
    let mut owner = PreviewOwner::default();
    let request = owner.begin(context, "image.svg".into()).unwrap();
    for (source, succeeds) in [
        (
            r#"<!DOCTYPE svg PUBLIC "-//W3C//DTD SVG 1.1//EN" "http://www.w3.org/Graphics/SVG/1.1/DTD/svg11.dtd"><svg xmlns="http://www.w3.org/2000/svg" width="2" height="2"><rect width="2" height="2"/></svg>"#,
            true,
        ),
        (
            r#"<svg xmlns="http://www.w3.org/2000/svg" width="100" height="100" font-size="16"><text x="10" y="30" font-family="Arial">A</text></svg>"#,
            true,
        ),
        (
            r#"<svg xmlns="http://www.w3.org/2000/svg" width="2048" height="2048"/>"#,
            false,
        ),
        (
            r#"<svg xmlns="http://www.w3.org/2000/svg"><text x="5000" y="5000" font-family="Arial">A</text></svg>"#,
            false,
        ),
    ] {
        std::fs::write(directory.path().join("image.svg"), source).unwrap();
        let PreparedPreview::Ready(snapshot) = PreviewService::default()
            .prepare(None, &request.context.base_path, "image.svg")
            .unwrap()
        else {
            panic!("snapshot");
        };
        let PreviewResult::Ready(loaded) = load_snapshot(&request, snapshot) else {
            panic!("ready");
        };
        assert_eq!(loaded.image.is_some(), succeeds);
        if let Some(raw) = loaded.image {
            assert_eq!(raw.decode_limits, Some(DECODE_LIMITS));
            assert!(raw.to_image_data(SvgRenderer::new(Arc::new(()))).is_ok());
        }
    }
}

#[test]
fn load_snapshot_admits_only_budgeted_raw_gifs_and_owner_cancels_work() {
    use super::super::{PreviewContext, PreviewOwner, PreviewResult, load_snapshot};
    use crate::components::files::FileContent;
    use tau_core::file_preview::{PreparedPreview, PreviewService};
    let directory = tempfile::tempdir().unwrap();
    let context = PreviewContext {
        session: "image-budget".into(),
        source: "exact-source".into(),
        base_path: directory.path().to_string_lossy().into_owned(),
        remote_project: None,
        document_source_path: None,
        project_root: None,
    };
    let mut owner = PreviewOwner::default();
    let request = owner.begin(context, "image.gif".into()).unwrap();
    let token = owner.decode_cancelled.clone();
    for (bytes, succeeds) in [
        (animation(2, 2, 3), true),
        (animation(1024, 1024, 9), false),
    ] {
        std::fs::write(directory.path().join("image.gif"), bytes).unwrap();
        let PreparedPreview::Ready(snapshot) = PreviewService::default()
            .prepare(None, &request.context.base_path, "image.gif")
            .unwrap()
        else {
            panic!("snapshot");
        };
        let PreviewResult::Ready(loaded) = load_snapshot(&request, snapshot) else {
            panic!("ready");
        };
        assert_eq!(loaded.image.is_some(), succeeds);
        if succeeds {
            assert!(matches!(loaded.content, FileContent::Image));
        } else {
            assert!(matches!(loaded.content, FileContent::Error(_)));
        }
    }
    owner.invalidate();
    assert!(token.load(Ordering::Relaxed));
    assert!(!owner.is_current(&request));
}
