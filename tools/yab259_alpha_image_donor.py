#!/usr/bin/env python3
"""Prepare the exact repaired Yab #259 donor with product order and exact image alpha."""

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

EXPECTED_ALPHA_REPAIR_FILES = EXPECTED_ORDERED_REPAIR_FILES


def prepare_alpha_image_pdf_donor(
    repository: pathlib.Path,
    checkout: pathlib.Path,
) -> tuple[str, str, str]:
    """Apply the proven repair/order stack plus exact PDF soft-mask image alpha."""

    base_repair_sha256, order_repair_sha256 = prepare_order_preserving_pdf_donor(
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
    },
    Rgba {
        width: u32,
        height: u32,
        rgb_bytes: Vec<u8>,
        alpha_bytes: Vec<u8>,
    },
    Unsupported {
""",
        label="Yab #259 exact alpha prepared-image variant",
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
    let pixel_count = width as usize * height as usize;
    let mut rgb = Vec::with_capacity(pixel_count * 3);
    let mut alpha = Vec::with_capacity(pixel_count);
    let mut has_non_opaque_alpha = false;
    for pixel in rgba.pixels() {
        rgb.extend_from_slice(&pixel.0[..3]);
        alpha.push(pixel.0[3]);
        has_non_opaque_alpha |= pixel.0[3] != 255;
    }

    if has_non_opaque_alpha {
        Ok(PreparedImage::Rgba {
            width,
            height,
            rgb_bytes: rgb,
            alpha_bytes: alpha,
        })
    } else {
        Ok(PreparedImage::Rgb {
            width,
            height,
            bytes: rgb,
        })
    }
""",
        label="Yab #259 exact alpha decode without flattening",
    )

    replace_once(
        pdf,
        """            match prepared {
                PreparedImage::Rgb { .. } => {
                    append_image(content, node, resource_id);
""",
        """            match prepared {
                PreparedImage::Rgb { .. } | PreparedImage::Rgba { .. } => {
                    append_image(content, node, resource_id);
""",
        label="Yab #259 alpha image paint admission",
    )

    replace_once(
        pdf,
        """        .filter_map(|(resource_id, image)| {
            matches!(image, PreparedImage::Rgb { .. }).then_some(*resource_id)
        })
""",
        """        .filter_map(|(resource_id, image)| {
            matches!(
                image,
                PreparedImage::Rgb { .. } | PreparedImage::Rgba { .. }
            )
            .then_some(*resource_id)
        })
""",
        label="Yab #259 alpha image object admission",
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
    let font_object_ids = font_ids
""",
        """    let first_image_object_id = 3 + surfaces.len() * 2;
    let mut next_image_object_id = first_image_object_id;
    let mut image_object_ids = BTreeMap::<ResourceId, (usize, Option<usize>)>::new();
    for resource_id in &image_ids {
        let image_id = next_image_object_id;
        next_image_object_id += 1;
        let soft_mask_id = matches!(
            prepared_images.get(resource_id),
            Some(PreparedImage::Rgba { .. })
        )
        .then(|| {
            let value = next_image_object_id;
            next_image_object_id += 1;
            value
        });
        image_object_ids.insert(*resource_id, (image_id, soft_mask_id));
    }

    let first_font_object_id = next_image_object_id;
    let image_object_count = first_font_object_id - first_image_object_id;
    let font_object_ids = font_ids
""",
        label="Yab #259 alpha image/soft-mask object identities",
    )

    replace_once(
        pdf,
        """    let mut objects = Vec::<Vec<u8>>::with_capacity(
        2 + surfaces.len() * 2 + image_ids.len() + font_ids.len() * 5,
    );
""",
        """    let mut objects = Vec::<Vec<u8>>::with_capacity(
        2 + surfaces.len() * 2 + image_object_count + font_ids.len() * 5,
    );
""",
        label="Yab #259 alpha PDF object capacity",
    )

    replace_once(
        pdf,
        """                    image_name(*resource_id),
                    image_object_id[resource_id]
""",
        """                    image_name(*resource_id),
                    image_object_ids[resource_id].0
""",
        label="Yab #259 alpha page XObject identity",
    )

    replace_once(
        pdf,
        """    for resource_id in image_ids {
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
        """    for resource_id in image_ids {
        match &prepared_images[&resource_id] {
            PreparedImage::Rgb {
                width,
                height,
                bytes,
            } => {
                let mut stream = format!(
                    "<< /Type /XObject /Subtype /Image /Width {width} /Height {height} /ColorSpace /DeviceRGB /BitsPerComponent 8 /Length {} >>\nstream\n",
                    bytes.len()
                )
                .into_bytes();
                stream.extend_from_slice(bytes);
                stream.extend_from_slice(b"\nendstream");
                objects.push(stream);
            }
            PreparedImage::Rgba {
                width,
                height,
                rgb_bytes,
                alpha_bytes,
            } => {
                let (_, Some(soft_mask_id)) = image_object_ids[&resource_id] else {
                    unreachable!("RGBA image must have a soft-mask object identity")
                };
                let mut stream = format!(
                    "<< /Type /XObject /Subtype /Image /Width {width} /Height {height} /ColorSpace /DeviceRGB /BitsPerComponent 8 /SMask {soft_mask_id} 0 R /Length {} >>\nstream\n",
                    rgb_bytes.len()
                )
                .into_bytes();
                stream.extend_from_slice(rgb_bytes);
                stream.extend_from_slice(b"\nendstream");
                objects.push(stream);

                let mut mask = format!(
                    "<< /Type /XObject /Subtype /Image /Width {width} /Height {height} /ColorSpace /DeviceGray /BitsPerComponent 8 /Length {} >>\nstream\n",
                    alpha_bytes.len()
                )
                .into_bytes();
                mask.extend_from_slice(alpha_bytes);
                mask.extend_from_slice(b"\nendstream");
                objects.push(mask);
            }
            PreparedImage::Unsupported { .. } => {
                unreachable!("image object list contains only admitted images")
            }
        }
    }
""",
        label="Yab #259 exact PDF soft-mask image emission",
    )

    run_checked(
        ["git", "diff", "--check"],
        cwd=checkout,
        label="alpha-image Yab #259 donor has whitespace errors",
    )
    repairs = changed_files(checkout)
    if repairs != EXPECTED_ALPHA_REPAIR_FILES:
        raise RuntimeError(
            "unexpected alpha-image Yab #259 repair file set: "
            + ",".join(sorted(repairs))
        )

    text = pdf.read_text(encoding="utf-8")
    if "pdf.image.alpha_unsupported" in text:
        raise RuntimeError("Yab #259 alpha rejection survived bounded repair")
    if "/SMask {soft_mask_id} 0 R" not in text or "/DeviceGray" not in text:
        raise RuntimeError("Yab #259 alpha soft-mask emission is missing")
    if "surfaces.sort_by_key(|surface| surface.origin)" in text:
        raise RuntimeError("Yab #259 page-order sort returned in alpha donor")
    if "nodes.sort_by_key(|node| node.origin)" in text:
        raise RuntimeError("Yab #259 node-order sort returned in alpha donor")
    if "reports.sort_by_key(|node| node.origin)" not in text:
        raise RuntimeError("Yab #259 deterministic report ordering was removed")

    patch = run_checked(
        ["git", "diff", "--binary"],
        cwd=checkout,
        label="cannot fingerprint alpha-image Yab #259 repair bundle",
    )
    alpha_repair_sha256 = hashlib.sha256(patch.encode("utf-8")).hexdigest()
    return base_repair_sha256, order_repair_sha256, alpha_repair_sha256


__all__ = [
    "bind_yab_repository",
    "prepare_alpha_image_pdf_donor",
]
