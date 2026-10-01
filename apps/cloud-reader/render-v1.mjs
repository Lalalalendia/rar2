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
  if (width <= 0 || height <= 0 || fontSize <= 0 || lineHeight <= 0) return null;

  const lines = [];
  let cursorY = y;
  for (const line of [...(layout.lines ?? [])].sort((left, right) => left.line_index - right.line_index)) {
    const lineIndex = safeInteger(line.line_index, "text.line_index");
    const currentLineHeight = safeInteger(line.line_height_emu, "text.line_height_emu");
    const lineXOffset = safeInteger(line.x_offset_emu ?? 0, "text.x_offset_emu");
    const measuredWidth = safeInteger(line.measured_width_emu, "text.measured_width_emu");
    if (currentLineHeight <= 0 || lineXOffset < 0 || measuredWidth < 0) return null;

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
      x: x + lineXOffset,
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

function appendTableText(group, node) {
  const table = node.table;
  if (!table) return;
  for (const cell of table.cells ?? []) {
    const geometry = tableCellPaintGeometry(cell);
    if (!geometry || !cell.text) continue;

    const foreign = previewForeignObject(geometry, {
      "data-table-cell-id": cell.cell_id,
      "data-table-row": cell.row,
      "data-table-column": cell.column,
      "data-table-paint-authority": "none",
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

  const image = svgNode("image", {
    x: plan.geometry.x,
    y: plan.geometry.y,
    width: plan.geometry.width,
    height: plan.geometry.height,
    preserveAspectRatio: "none",
    "clip-path": "url(#" + clipId + ")",
    "data-resource-id": plan.resource_id,
    "data-resource-availability": plan.availability
  });
  image.setAttribute("href", plan.href);
  group.appendChild(image);
  return true;
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

  const fill = rgb(node.paint?.fill_rgb);
  if (fill) {
    group.appendChild(svgNode("rect", {
      x: bounds.x,
      y: bounds.y,
      width: bounds.width,
      height: bounds.height,
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
  if (stroke && Number(line.width_emu) > 0) {
    group.appendChild(svgNode("rect", {
      x: bounds.x,
      y: bounds.y,
      width: bounds.width,
      height: bounds.height,
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
