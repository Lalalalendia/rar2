const FORBIDDEN_SOURCE_KEYS = new Set([
  "raw_pub_bytes", "raw_bytes", "source_path", "filesystem_path",
  "cfb_path", "stream_path", "stream_name", "parser_record",
  "carrier", "source_ref", "byte_range"
]);

const SVG_NS = "http://www.w3.org/2000/svg";
const XHTML_NS = "http://www.w3.org/1999/xhtml";
const Q16_ONE = 65_536;
// Physical fallback metrics from chaptera-desktop-fallback-font-resource.
// Browser font sizes (including SVG attributes) are clamped. Paint text in
// bounded pixel-sized local coordinates, then map it back to canonical EMU.
// Server line positions remain authoritative; previews remain approximate.
const PREVIEW_FONT_SIZE_EMU = 114_300;
const PREVIEW_LINE_HEIGHT_EMU = 142_875;
const EMU_PER_CSS_PX = 9525;

function previewForeignObject(bounds, attrs) {
  return svgNode("foreignObject", {
    x: 0,
    y: 0,
    width: safeInteger(bounds.width, "preview.bounds.width") / EMU_PER_CSS_PX,
    height: safeInteger(bounds.height, "preview.bounds.height") / EMU_PER_CSS_PX,
    transform: "translate(" + safeInteger(bounds.x, "preview.bounds.x") + " "
      + safeInteger(bounds.y, "preview.bounds.y") + ") scale(" + EMU_PER_CSS_PX + ")",
    ...attrs
  });
}

function previewTextStyle(div, plan = null) {
  div.style.width = "100%";
  div.style.height = "100%";
  div.style.overflow = "hidden";
  div.style.whiteSpace = "pre-wrap";
  div.style.fontFamily = "system-ui, sans-serif";
  div.style.fontSize = ((plan?.font_size_emu ?? PREVIEW_FONT_SIZE_EMU) / EMU_PER_CSS_PX) + "px";
  div.style.lineHeight = ((plan?.line_height_emu ?? PREVIEW_LINE_HEIGHT_EMU) / EMU_PER_CSS_PX) + "px";
  div.style.color = "#000";
}

function safeInteger(value, label) {
  if (!Number.isSafeInteger(value)) {
    throw new RangeError(label + " must be a JavaScript-safe integer");
  }
  return value;
}

function finiteNumber(value, label) {
  const number = Number(value);
  if (!Number.isFinite(number)) throw new TypeError(label + " must be finite");
  return number;
}

export function assertReaderSceneSourceNeutral(value, at = "$") {
  if (Array.isArray(value)) {
    value.forEach((child, index) => assertReaderSceneSourceNeutral(child, at + "[" + index + "]"));
    return;
  }
  if (value && typeof value === "object") {
    for (const [key, child] of Object.entries(value)) {
      if (FORBIDDEN_SOURCE_KEYS.has(key)) {
        throw new Error("renderer input contains forbidden source field " + key + " at " + at);
      }
      assertReaderSceneSourceNeutral(child, at + "." + key);
    }
  }
}

function svgNode(tag, attrs = {}) {
  const node = document.createElementNS(SVG_NS, tag);
  for (const [key, value] of Object.entries(attrs)) {
    if (value !== null && value !== undefined) node.setAttribute(key, String(value));
  }
  return node;
}

function rgb(value) {
  if (!Array.isArray(value) || value.length !== 3) return null;
  return "rgb(" + value.map(Number).join(" ") + ")";
}

function fontDataUrl(resource) {
  if (resource?.availability !== "inline_data_url") return null;
  const value = resource.inline_data_url;
  if (typeof value !== "string") return null;
  if (!/^data:font\/(?:ttf|otf);base64,[A-Za-z0-9+/=]+$/.test(value)) return null;
  return value;
}

async function installFonts(payload) {
  const installed = new Map();
  if (typeof FontFace !== "function" || !document.fonts) return installed;

  for (const resource of payload.fonts ?? []) {
    const href = fontDataUrl(resource);
    if (!href) continue;
    const family = "ChapteraReader_" + String(resource.expected_sha256 ?? "").slice(0, 16);
    try {
      const face = new FontFace(family, "url(" + href + ")");
      await face.load();
      document.fonts.add(face);
      installed.set(resource.resource_id, Object.freeze({ family, resource }));
    } catch {
      // Fail closed to the explicit preview-only path below.
    }
  }
  return installed;
}

function imageDataUrl(resource) {
  if (resource?.availability !== "inline_data_url") return null;
  const value = resource.inline_data_url;
  if (typeof value !== "string") return null;
  if (!/^data:image\/(?:png|jpeg|jpg|gif);base64,[A-Za-z0-9+/=]+$/.test(value)) return null;
  return value;
}

export function imagePaintGeometry(bounds, sourceWindow = null) {
  const x = safeInteger(bounds.x, "bounds.x");
  const y = safeInteger(bounds.y, "bounds.y");
  const width = safeInteger(bounds.width, "bounds.width");
  const height = safeInteger(bounds.height, "bounds.height");
  if (width <= 0 || height <= 0) return null;

  if (!sourceWindow) {
    return Object.freeze({ x, y, width, height });
  }

  const left = safeInteger(sourceWindow.left_q16, "source_window.left_q16") / Q16_ONE;
  const top = safeInteger(sourceWindow.top_q16, "source_window.top_q16") / Q16_ONE;
  const right = safeInteger(sourceWindow.right_q16, "source_window.right_q16") / Q16_ONE;
  const bottom = safeInteger(sourceWindow.bottom_q16, "source_window.bottom_q16") / Q16_ONE;
  if (right <= left || bottom <= top) return null;

  const sourceLeft = Math.max(left, 0);
  const sourceTop = Math.max(top, 0);
  const sourceRight = Math.min(right, 1);
  const sourceBottom = Math.min(bottom, 1);
  if (sourceRight <= sourceLeft || sourceBottom <= sourceTop) return null;

  const windowWidth = right - left;
  const windowHeight = bottom - top;
  return Object.freeze({
    x: x - (left / windowWidth) * width,
    y: y - (top / windowHeight) * height,
    width: width / windowWidth,
    height: height / windowHeight
  });
}

function nodeTransform(node) {
  const transform = node.transform;
  if (!transform) return null;
  const a = finiteNumber(transform.a, "transform.a");
  const b = finiteNumber(transform.b, "transform.b");
  const c = finiteNumber(transform.c, "transform.c");
  const d = finiteNumber(transform.d, "transform.d");
  const tx = safeInteger(transform.tx, "transform.tx");
  const ty = safeInteger(transform.ty, "transform.ty");
  if (a === 1 && b === 0 && c === 0 && d === 1 && tx === 0 && ty === 0) return null;
  return "matrix(" + [a, b, c, d, tx, ty].join(" ") + ")";
}

export function resolvedTextLinePaintPlan(node) {
  const layout = node?.text_layout;
  if (!layout || layout.disposition !== "shared_resolved") return null;
  const bounds = node.text_bounds ?? node.bounds;
  const x = safeInteger(bounds.x, "text.bounds.x");
  const y = safeInteger(bounds.y, "text.bounds.y");
  const width = safeInteger(bounds.width, "text.bounds.width");
  const height = safeInteger(bounds.height, "text.bounds.height");
  const fontSize = safeInteger(layout.font_size_emu, "text.font_size_emu");
  const lineHeight = safeInteger(layout.line_height_emu, "text.line_height_emu");
  const color = rgb(layout.color_rgb) ?? "rgb(0 0 0)";
  if (width <= 0 || height <= 0 || fontSize <= 0 || lineHeight <= 0) return null;

  const verticalOffset = safeInteger(layout.vertical_offset_emu ?? 0, "text.vertical_offset_emu");
  if (verticalOffset < 0 || verticalOffset > height) return null;

  const lines = [];
  let cursorY = y + verticalOffset;
  for (const line of [...(layout.lines ?? [])].sort((left, right) => left.line_index - right.line_index)) {
    const lineIndex = safeInteger(line.line_index, "text.line_index");
    const currentLineHeight = safeInteger(line.line_height_emu, "text.line_height_emu");
    const measuredWidth = safeInteger(line.measured_width_emu, "text.measured_width_emu");
    const lineOffset = safeInteger(line.x_offset_emu ?? 0, "text.x_offset_emu");
    if (currentLineHeight <= 0 || measuredWidth < 0 || lineOffset < 0 || lineOffset + measuredWidth > width) return null;

    const spans = [];
    for (const span of line.spans ?? []) {
      const scalarStart = safeInteger(span.scalar_start, "text.span.scalar_start");
      const scalarEnd = safeInteger(span.scalar_end, "text.span.scalar_end");
      const xOffset = safeInteger(span.x_offset_emu, "text.span.x_offset_emu");
      const spanWidth = safeInteger(span.measured_width_emu, "text.span.measured_width_emu");
      const spanFontSize = safeInteger(span.font_size_emu, "text.span.font_size_emu");
      if (scalarEnd <= scalarStart || xOffset < 0 || spanWidth < 0 || spanFontSize <= 0) return null;
      spans.push(Object.freeze({
        scalar_start: scalarStart,
        scalar_end: scalarEnd,
        text: String(span.text ?? ""),
        x_offset_emu: xOffset,
        measured_width_emu: spanWidth,
        font_size_emu: spanFontSize
      }));
    }

    lines.push(Object.freeze({
      line_index: lineIndex,
      x: x + lineOffset,
      y: cursorY,
      text: String(line.text ?? ""),
      measured_width_emu: measuredWidth,
      line_height_emu: currentLineHeight,
      spans: Object.freeze(spans)
    }));
    cursorY += currentLineHeight;
    if (cursorY - y > height) return null;
  }

  return Object.freeze({
    bounds: Object.freeze({ x, y, width, height }),
    font_resource_id: layout.font_resource_id,
    font_size_emu: fontSize,
    line_height_emu: lineHeight,
    color,
    lines: Object.freeze(lines)
  });
}

function appendPreviewText(group, node, plan = null) {
  if (!node.text) return;
  const bounds = node.text_bounds ?? node.bounds;
  const foreign = previewForeignObject(bounds, {
    "data-text-authority": "browser-preview-only"
  });
  const div = document.createElementNS(XHTML_NS, "div");
  previewTextStyle(div, plan);
  div.textContent = node.text;
  foreign.appendChild(div);
  group.appendChild(foreign);
}

function appendText(group, defs, node, fonts, index) {
  if (!node.text) return;
  const plan = resolvedTextLinePaintPlan(node);
  const installed = plan ? fonts.get(plan.font_resource_id) ?? null : null;
  if (!plan || !installed) {
    appendPreviewText(group, node, plan);
    return;
  }

  const clipId = "chaptera-reader-text-clip-" + index;
  const clipPath = svgNode("clipPath", { id: clipId });
  clipPath.appendChild(svgNode("rect", {
    x: plan.bounds.x,
    y: plan.bounds.y,
    width: plan.bounds.width,
    height: plan.bounds.height
  }));
  defs.appendChild(clipPath);

  // Clip in canonical node space before changing the text's local units.
  // The enclosing node transform still applies to both clip and line paint.
  const clipped = svgNode("g", { "clip-path": "url(#" + clipId + ")" });
  const local = svgNode("g", {
    transform: "translate(" + plan.bounds.x + " " + plan.bounds.y + ") scale(" + EMU_PER_CSS_PX + ")"
  });
  clipped.appendChild(local);
  group.appendChild(clipped);

  for (const line of plan.lines) {
    const text = svgNode("text", {
      x: (line.x - plan.bounds.x) / EMU_PER_CSS_PX,
      y: (line.y - plan.bounds.y) / EMU_PER_CSS_PX,
      "font-family": installed.family,
      "font-size": plan.font_size_emu / EMU_PER_CSS_PX,
      fill: plan.color,
      "text-rendering": "geometricPrecision",
      "dominant-baseline": "text-before-edge",
      "data-text-authority": "server-shared-resolved",
      "data-text-line-index": line.line_index,
      "data-measured-width-emu": line.measured_width_emu
    });
    text.setAttribute("xml:space", "preserve");
    if (line.spans.length) {
      for (const span of line.spans) {
        const tspan = svgNode("tspan", {
          x: (line.x + span.x_offset_emu - plan.bounds.x) / EMU_PER_CSS_PX,
          "font-size": span.font_size_emu / EMU_PER_CSS_PX,
          "data-text-span-start": span.scalar_start,
          "data-text-span-end": span.scalar_end,
          "data-measured-width-emu": span.measured_width_emu
        });
        tspan.setAttribute("xml:space", "preserve");
        tspan.textContent = span.text;
        text.appendChild(tspan);
      }
    } else {
      text.textContent = line.text;
    }
    local.appendChild(text);
  }
}

export function tableCellPaintGeometry(cell) {
  const bounds = cell?.bounds;
  if (!bounds) return null;
  const x = safeInteger(bounds.x, "table.cell.bounds.x");
  const y = safeInteger(bounds.y, "table.cell.bounds.y");
  const width = safeInteger(bounds.width, "table.cell.bounds.width");
  const height = safeInteger(bounds.height, "table.cell.bounds.height");
  if (width <= 0 || height <= 0) return null;
  return Object.freeze({ x, y, width, height });
}

export function tableCellFillPaintPlan(cell) {
  const geometry = tableCellPaintGeometry(cell);
  const fill = rgb(cell?.fill_rgb);
  if (!geometry || cell?.fill_visible !== true || !fill) return null;
  return Object.freeze({ geometry, fill });
}

export function tableBorderPaintPlan(border) {
  const x1 = safeInteger(border?.x1_emu, "table.border.x1_emu");
  const y1 = safeInteger(border?.y1_emu, "table.border.y1_emu");
  const x2 = safeInteger(border?.x2_emu, "table.border.x2_emu");
  const y2 = safeInteger(border?.y2_emu, "table.border.y2_emu");
  const width = safeInteger(border?.width_emu, "table.border.width_emu");
  const stroke = rgb(border?.rgb);
  if (!stroke || width <= 0 || (x1 === x2 && y1 === y2)) return null;
  return Object.freeze({ x1, y1, x2, y2, stroke, width });
}

function appendTableText(group, node) {
  const table = node.table;
  if (!table) return;

  for (const cell of table.cells ?? []) {
    const fillPlan = tableCellFillPaintPlan(cell);
    if (!fillPlan) continue;
    group.appendChild(svgNode("rect", {
      x: fillPlan.geometry.x,
      y: fillPlan.geometry.y,
      width: fillPlan.geometry.width,
      height: fillPlan.geometry.height,
      fill: fillPlan.fill,
      "data-table-cell-id": cell.cell_id,
      "data-table-cell-paint-authority": "source-t595"
    }));
  }

  for (const border of table.borders ?? []) {
    const plan = tableBorderPaintPlan(border);
    if (!plan) continue;
    group.appendChild(svgNode("line", {
      x1: plan.x1,
      y1: plan.y1,
      x2: plan.x2,
      y2: plan.y2,
      stroke: plan.stroke,
      "stroke-width": plan.width,
      "data-table-border-authority": "source-t840"
    }));
  }

  for (const cell of table.cells ?? []) {
    const geometry = tableCellPaintGeometry(cell);
    if (!geometry || !cell.text) continue;
    const fillPlan = tableCellFillPaintPlan(cell);
    const foreign = previewForeignObject(geometry, {
      "data-table-cell-id": cell.cell_id,
      "data-table-row": cell.row,
      "data-table-column": cell.column,
      "data-table-paint-authority": fillPlan ? "source-t595" : "none",
      "data-text-authority": "browser-preview-only"
    });
    const div = document.createElementNS(XHTML_NS, "div");
    previewTextStyle(div);
    div.style.padding = "2px";
    div.style.boxSizing = "border-box";
    div.textContent = cell.text.replace(/\r/g, "\n");
    foreign.appendChild(div);
    group.appendChild(foreign);
  }
}

export function imageRecolorPaintPlan(node) {
  const effect = node?.image_recolor;
  if (!effect || effect.preserve_grays !== false) return null;
  const target = effect.target_rgb;
  if (!Array.isArray(target) || target.length !== 3) return null;
  const channels = target.map((value) => {
    const channel = Number(value);
    if (!Number.isInteger(channel) || channel < 0 || channel > 255) {
      throw new RangeError("image_recolor.target_rgb must contain byte values");
    }
    return channel / 255;
  });

  // Bounded #802 A/B candidate: grayscale luminance drives a tint from the
  // persisted recolor target at black to white at full luminance. This is
  // renderer math only; source semantics stay the resolved target RGB +
  // preserve_grays disposition.
  const luminance = [0.2125, 0.7154, 0.0721];
  const rows = channels.map((targetChannel) => [
    luminance[0] * (1 - targetChannel),
    luminance[1] * (1 - targetChannel),
    luminance[2] * (1 - targetChannel),
    0,
    targetChannel
  ]);
  rows.push([0, 0, 0, 1, 0]);

  return Object.freeze({
    target_rgb: Object.freeze([...target]),
    values: rows.flat().join(" ")
  });
}

export function imageResourcePaintPlan(node, resource) {
  const href = imageDataUrl(resource);
  if (!href) return null;
  const geometry = imagePaintGeometry(node?.bounds, node?.image_source_window ?? null);
  if (!geometry) return null;
  return Object.freeze({
    href,
    resource_id: resource.resource_id,
    availability: resource.availability,
    geometry
  });
}

function appendImage(group, defs, node, resource, clipId) {
  const plan = imageResourcePaintPlan(node, resource);
  if (!plan) return false;

  const clipPath = svgNode("clipPath", { id: clipId });
  clipPath.appendChild(svgNode("rect", {
    x: node.bounds.x,
    y: node.bounds.y,
    width: node.bounds.width,
    height: node.bounds.height
  }));
  defs.appendChild(clipPath);

  const recolor = imageRecolorPaintPlan(node);
  let filterId = null;
  if (recolor) {
    filterId = clipId + "-recolor";
    const filter = svgNode("filter", {
      id: filterId,
      "color-interpolation-filters": "sRGB"
    });
    filter.appendChild(svgNode("feColorMatrix", {
      type: "matrix",
      values: recolor.values
    }));
    defs.appendChild(filter);
  }

  const image = svgNode("image", {
    x: plan.geometry.x,
    y: plan.geometry.y,
    width: plan.geometry.width,
    height: plan.geometry.height,
    preserveAspectRatio: "none",
    "clip-path": "url(#" + clipId + ")",
    filter: filterId ? "url(#" + filterId + ")" : null,
    "data-resource-id": plan.resource_id,
    "data-resource-availability": plan.availability,
    "data-image-recolor-authority": recolor ? "source-picture-recolor" : null
  });
  image.setAttribute("href", plan.href);
  group.appendChild(image);
  return true;
}

export function presetShapePaintGeometry(node) {
  const bounds = node?.bounds;
  if (!bounds) return null;
  const x = safeInteger(bounds.x, "node.bounds.x");
  const y = safeInteger(bounds.y, "node.bounds.y");
  const width = safeInteger(bounds.width, "node.bounds.width");
  const height = safeInteger(bounds.height, "node.bounds.height");
  if (width <= 0 || height <= 0) return null;

  if (node.paint?.preset_shape !== "round_rect") {
    return Object.freeze({ tag: "rect", attrs: Object.freeze({ x, y, width, height }) });
  }

  const radius = Math.round(Math.min(width, height) * 16667 / 100000);
  return Object.freeze({
    tag: "rect",
    attrs: Object.freeze({ x, y, width, height, rx: radius, ry: radius })
  });
}

function renderNode(svg, defs, node, resources, fonts, index) {
  const bounds = node.bounds;
  for (const [key, value] of Object.entries(bounds)) safeInteger(value, "node.bounds." + key);
  if (bounds.width <= 0 || bounds.height <= 0) return;

  const group = svgNode("g", {
    "data-node-id": node.node_id,
    "data-node-kind": node.kind
  });
  const transform = nodeTransform(node);
  if (transform) group.setAttribute("transform", transform);

  const shapeGeometry = presetShapePaintGeometry(node);
  const fill = rgb(node.paint?.fill_rgb);
  if (fill && shapeGeometry) {
    group.appendChild(svgNode(shapeGeometry.tag, {
      ...shapeGeometry.attrs,
      fill,
      stroke: "none"
    }));
  }

  let paintedResource = false;
  if (node.resource_id) {
    const resource = resources.get(node.resource_id) ?? null;
    paintedResource = appendImage(
      group,
      defs,
      node,
      resource,
      "chaptera-reader-clip-" + index
    );
    if (!paintedResource) {
      const placeholder = svgNode("rect", {
        x: bounds.x,
        y: bounds.y,
        width: bounds.width,
        height: bounds.height,
        fill: "none",
        stroke: "rgba(180,80,0,.65)",
        "stroke-dasharray": "38100 25400",
        "data-resource-missing": node.resource_id
      });
      group.appendChild(placeholder);
    }
  }

  const line = node.paint?.line;
  const stroke = rgb(line?.rgb);
  if (stroke && Number(line.width_emu) > 0 && shapeGeometry) {
    group.appendChild(svgNode(shapeGeometry.tag, {
      ...shapeGeometry.attrs,
      fill: "none",
      stroke,
      "stroke-width": safeInteger(line.width_emu, "line.width_emu")
    }));
  }

  appendTableText(group, node);
  appendText(group, defs, node, fonts, index);
  svg.appendChild(group);
}

export async function renderReaderScene(host, payload, options = {}) {
  assertReaderSceneSourceNeutral(payload);
  if (payload?.protocol_version !== "chaptera.reader-scene.v1") {
    throw new Error("renderer requires chaptera.reader-scene.v1");
  }

  const emuPerCssPx = finiteNumber(options.emuPerCssPx ?? 9525, "emuPerCssPx");
  const maxPageWidth = finiteNumber(options.maxPageWidth ?? 920, "maxPageWidth");
  if (!(emuPerCssPx > 0) || !(maxPageWidth > 0)) {
    throw new RangeError("renderer view scale must be positive");
  }

  const fonts = await installFonts(payload);
  host.replaceChildren();
  const resources = new Map((payload.resources ?? []).map((resource) => [resource.resource_id, resource]));
  const pages = [...(payload.pages ?? [])].sort((a, b) => a.order - b.order);
  let nodeIndex = 0;

  for (const pageModel of pages) {
    safeInteger(pageModel.width_emu, "page.width_emu");
    safeInteger(pageModel.height_emu, "page.height_emu");
    if (pageModel.width_emu <= 0 || pageModel.height_emu <= 0) continue;

    const naturalWidth = pageModel.width_emu / emuPerCssPx;
    const scale = Math.min(1, maxPageWidth / Math.max(1, naturalWidth));
    const svg = svgNode("svg", {
      class: "page",
      viewBox: "0 0 " + pageModel.width_emu + " " + pageModel.height_emu,
      width: naturalWidth * scale,
      height: (pageModel.height_emu / emuPerCssPx) * scale,
      "data-page-id": pageModel.page_id,
      "data-renderer": "svg"
    });
    svg.style.display = "block";
    svg.style.background = "#fff";
    svg.style.overflow = "hidden";

    const defs = svgNode("defs");
    svg.appendChild(defs);
    svg.appendChild(svgNode("rect", {
      x: 0,
      y: 0,
      width: pageModel.width_emu,
      height: pageModel.height_emu,
      fill: "#fff"
    }));

    for (const node of payload.nodes ?? []) {
      if (node.page_id !== pageModel.page_id) continue;
      renderNode(svg, defs, node, resources, fonts, nodeIndex);
      nodeIndex += 1;
    }
    host.appendChild(svg);
  }
}
