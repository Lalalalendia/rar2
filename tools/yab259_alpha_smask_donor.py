#!/usr/bin/env python3
"""Prepare exact repaired/order-preserving Yab #259 donor with exact image alpha soft masks."""

from __future__ import annotations

import hashlib
import pathlib

from run_yab259_fixed_pdf_closure import changed_files, replace_once, run_checked
from yab259_order_preserving_donor import (
    EXPECTED_ORDERED_REPAIR_FILES,
    ORDERED_PDF_FILE,
    bind_yab_repository,
    prepare_order_preserving_pdf_donor,
)

EXPECTED_ALPHA_REPAIR_FILES = EXPECTED_ORDERED_REPAIR_FILES | {ORDERED_PDF_FILE}


def prepare_alpha_smask_pdf_donor(
    repository: pathlib.Path,
    checkout: pathlib.Path,
) -> tuple[str, str, str]:
    base_repair_sha256, ordered_repair_sha256 = prepare_order_preserving_pdf_donor(
        repository,
        checkout,
    )
    pdf = checkout / ORDERED_PDF_FILE

    replace_once(
        pdf,
        """enum PreparedImage {
    Rgb {
        width: u32,
        height: u32,
        bytes: Vec<u8>,
    },
    Unsupported {
""",
        """enum PreparedImage {
    Rgb {
        width: u32,
        height: u32,
        bytes: Vec<u8>,
        alpha: Option<Vec<u8>>,
    },
    Unsupported {
""",
        label="Yab #259 alpha prepared-image plane",
    )

    replace_once(
        pdf,
        """    let rgba = decoded.to_rgba8();
    if rgba.pixels().any(|pixel| pixel.0[3] != 255) {
        return Ok(PreparedImage::Unsupported {
            code: "pdf.image.alpha_unsupported".into(),
            message: "exact image contains non-opaque alpha; bounded PDF v0.1 does not flatten or invent a background".into(),
        });
    }

    let (width, height) = rgba.dimensions();
    let mut rgb = Vec::with_capacity(width as usize * height as usize * 3);
    for pixel in rgba.pixels() {
        rgb.extend_from_slice(&pixel.0[..3]);
    }
    Ok(PreparedImage::Rgb {
        width,
        height,
        bytes: rgb,
    })
""",
        """    let rgba = decoded.to_rgba8();
    let (width, height) = rgba.dimensions();
    let mut rgb = Vec::with_capacity(width as usize * height as usize * 3);
    let mut alpha = Vec::with_capacity(width as usize * height as usize);
    let mut has_alpha = false;
    for pixel in rgba.pixels() {
        rgb.extend_from_slice(&pixel.0[..3]);
        alpha.push(pixel.0[3]);
        has_alpha |= pixel.0[3] != 255;
    }
    Ok(PreparedImage::Rgb {
        width,
        height,
        bytes: rgb,
        alpha: has_alpha.then_some(alpha),
    })
""",
        label="Yab #259 exact RGBA split",
    )

    replace_once(
        pdf,
        """    let first_image_object_id = 3 + surfaces.len() * 2;
    let image_object_id = image_ids
        .iter()
        .enumerate()
        .map(|(index, resource_id)| (*resource_id, first_image_object_id + index))
        .collect::<BTreeMap<_, _>>();

    let first_font_object_id = first_image_object_id + image_ids.len();
""",
        """    let first_image_object_id = 3 + surfaces.len() * 2;
    let mut next_image_object_id = first_image_object_id;
    let mut image_object_ids = BTreeMap::<ResourceId, (usize, Option<usize>)>::new();
    for resource_id in &image_ids {
        let rgb_object_id = next_image_object_id;
        next_image_object_id += 1;
        let alpha_object_id = match &prepared_images[resource_id] {
            PreparedImage::Rgb {
                alpha: Some(_), ..
            } => {
                let object_id = next_image_object_id;
                next_image_object_id += 1;
                Some(object_id)
            }
            PreparedImage::Rgb { alpha: None, .. } => None,
            PreparedImage::Unsupported { .. } => unreachable!(
                "image object list contains only prepared RGB images"
            ),
        };
        image_object_ids.insert(*resource_id, (rgb_object_id, alpha_object_id));
    }

    let first_font_object_id = next_image_object_id;
""",
        label="Yab #259 alpha image object allocation",
    )

    replace_once(
        pdf,
        """    let mut objects = Vec::<Vec<u8>>::with_capacity(
        2 + surfaces.len() * 2 + image_ids.len() + font_ids.len() * 5,
    );
""",
        """    let image_object_count = image_object_ids
        .values()
        .map(|(_, alpha_object_id)| if alpha_object_id.is_some() { 2 } else { 1 })
        .sum::<usize>();
    let mut objects = Vec::<Vec<u8>>::with_capacity(
        2 + surfaces.len() * 2 + image_object_count + font_ids.len() * 5,
    );
""",
        label="Yab #259 alpha image object capacity",
    )

    replace_once(
        pdf,
        """                    image_name(*resource_id),
                    image_object_id[resource_id]
""",
        """                    image_name(*resource_id),
                    image_object_ids[resource_id].0
""",
        label="Yab #259 RGB XObject reference",
    )

    replace_once(
        pdf,
        """    #[test]
    fn unsupported_transform_is_reported_not_silently_rendered() {
""",
        """    #[test]
    fn exact_rgba_png_uses_soft_mask_and_remains_painted() {
        use std::io::Cursor;

        let mut rgba = image::RgbaImage::new(1, 1);
        rgba.put_pixel(0, 0, image::Rgba([10, 20, 30, 128]));
        let mut encoded = Cursor::new(Vec::new());
        image::DynamicImage::ImageRgba8(rgba)
            .write_to(&mut encoded, ImageFormat::Png)
            .unwrap();

        let resources = FixedPdfResources {
            node_paints: Vec::new(),
            images: vec![FixedImageResource {
                resource_id: resource_id(43),
                mime: "image/png".into(),
                node_ids: vec![node_id(10)],
                bytes: encoded.into_inner(),
            }],
            ..FixedPdfResources::default()
        };

        let output = render_bounded_pdf(
            &scene(),
            &resources,
            &PdfTargetProfile::basic_geometry_v0_1(),
        )
        .unwrap();

        let text = String::from_utf8_lossy(&output.bytes);
        assert!(text.contains("/SMask "));
        assert!(text.contains("/ColorSpace /DeviceGray"));
        assert_eq!(
            output
                .report
                .nodes
                .iter()
                .filter(|node| node.code == "pdf.node.painted_exact_image")
                .count(),
            1
        );
        assert!(
            output
                .report
                .diagnostics
                .iter()
                .all(|diagnostic| diagnostic.code != "pdf.image.alpha_unsupported")
        );
    }

    #[test]
    fn unsupported_transform_is_reported_not_silently_rendered() {
""",
        label="Yab #259 exact alpha soft-mask regression",
    )

    replace_once(
        pdf,
        r"""    for resource_id in image_ids {
        let PreparedImage::Rgb {
            width,
            height,
            bytes,
        } = &prepared_images[&resource_id]
        else {
            unreachable!("image object list contains only RGB images")
        };
        let mut stream = format!(
            "<< /Type /XObject /Subtype /Image /Width {width} /Height {height} /ColorSpace /DeviceRGB /BitsPerComponent 8 /Length {} >>\nstream\n",
            bytes.len()
        )
        .into_bytes();
        stream.extend_from_slice(bytes);
        stream.extend_from_slice(b"\nendstream");
        objects.push(stream);
    }
""",
        r"""    for resource_id in image_ids {
        let PreparedImage::Rgb {
            width,
            height,
            bytes,
            alpha,
        } = &prepared_images[&resource_id]
        else {
            unreachable!("image object list contains only RGB images")
        };
        let (_, alpha_object_id) = image_object_ids[&resource_id];
        let smask = alpha_object_id
            .map(|object_id| format!(" /SMask {object_id} 0 R"))
            .unwrap_or_default();
        let mut stream = format!(
            "<< /Type /XObject /Subtype /Image /Width {width} /Height {height} /ColorSpace /DeviceRGB /BitsPerComponent 8{smask} /Length {} >>\nstream\n",
            bytes.len()
        )
        .into_bytes();
        stream.extend_from_slice(bytes);
        stream.extend_from_slice(b"\nendstream");
        objects.push(stream);

        if let Some(alpha) = alpha {
            let mut mask_stream = format!(
                "<< /Type /XObject /Subtype /Image /Width {width} /Height {height} /ColorSpace /DeviceGray /BitsPerComponent 8 /Length {} >>\nstream\n",
                alpha.len()
            )
            .into_bytes();
            mask_stream.extend_from_slice(alpha);
            mask_stream.extend_from_slice(b"\nendstream");
            objects.push(mask_stream);
        }
    }
""",
        label="Yab #259 PDF alpha SMask serialization",
    )

    run_checked(
        ["git", "diff", "--check"],
        cwd=checkout,
        label="alpha-SMask repaired Yab #259 donor has whitespace errors",
    )

    repairs = changed_files(checkout)
    if repairs != EXPECTED_ALPHA_REPAIR_FILES:
        raise RuntimeError(
            "unexpected alpha-SMask Yab #259 repair file set: "
            + ",".join(sorted(repairs))
        )

    text = pdf.read_text(encoding="utf-8")
    if "pdf.image.alpha_unsupported" in text:
        raise RuntimeError("Yab #259 alpha rejection survived bounded SMask repair")
    if "/SMask {object_id} 0 R" not in text:
        raise RuntimeError("Yab #259 alpha SMask binding is missing")
    if "/ColorSpace /DeviceGray" not in text:
        raise RuntimeError("Yab #259 alpha soft-mask image is missing DeviceGray")
    if "reports.sort_by_key(|node| node.origin)" not in text:
        raise RuntimeError("deterministic report ordering was accidentally removed")

    patch = run_checked(
        ["git", "diff", "--binary"],
        cwd=checkout,
        label="cannot fingerprint alpha-SMask Yab #259 repair bundle",
    )
    alpha_repair_sha256 = hashlib.sha256(patch.encode("utf-8")).hexdigest()
    return base_repair_sha256, ordered_repair_sha256, alpha_repair_sha256


__all__ = [
    "bind_yab_repository",
    "prepare_alpha_smask_pdf_donor",
]
