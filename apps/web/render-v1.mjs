const FORBIDDEN_SOURCE_KEYS = new Set([
  "raw_pub_bytes", "raw_bytes", "source_path", "filesystem_path",
  "cfb_path", "stream_path", "stream_name", "parser_record",
  "carrier", "source_ref", "byte_range"
]);

export const RENDERER_KINDS = Object.freeze(["svg", "canvas2d", "webgl2-hybrid"]);

function finite(value, label) {
  if (!Number.isFinite(value)) throw new TypeError(label + " must be finite");
  return value;
}

function safeInteger(value, label) {
  if (!Number.isSafeInteger(value)) {
    throw new RangeError(label + " must be a JavaScript-safe integer");
  }
  return value;
}

export function assertSceneSourceNeutral(value, at = "$") {
  if (Array.isArray(value)) {
    value.forEach((child, index) => assertSceneSourceNeutral(child, at + "[" + index + "]"));
    return;
  }
  if (value && typeof value === "object") {
    for (const [key, child] of Object.entries(value)) {
      if (FORBIDDEN_SOURCE_KEYS.has(key)) {
        throw new Error("renderer input contains forbidden source field " + key + " at " + at);
      }
      assertSceneSourceNeutral(child, at + "." + key);
    }
  }
}

export function normalizeView(view = {}) {
  const emuPerCssPx = finite(view.emu_per_css_px ?? 9525, "emu_per_css_px");
  const zoom = finite(view.zoom ?? 1, "zoom");
  if (!(emuPerCssPx > 0)) throw new RangeError("emu_per_css_px must be > 0");
  if (!(zoom > 0)) throw new RangeError("zoom must be > 0");
  return Object.freeze({
    emu_per_css_px: emuPerCssPx,
    zoom,
    pan_x_css_px: finite(view.pan_x_css_px ?? 0, "pan_x_css_px"),
    pan_y_css_px: finite(view.pan_y_css_px ?? 0, "pan_y_css_px"),
    page_gap_css_px: finite(view.page_gap_css_px ?? 32, "page_gap_css_px"),
    page_margin_css_px: finite(view.page_margin_css_px ?? 24, "page_margin_css_px")
  });
}

function emuToCss(value, view) {
  return (safeInteger(value, "EMU") / view.emu_per_css_px) * view.zoom;
}

function compareExactStacking(a, b) {
  if (a.z_order !== b.z_order) return a.z_order < b.z_order ? -1 : 1;
  if (a.paint_order !== b.paint_order) return a.paint_order < b.paint_order ? -1 : 1;
  return a.node_id.localeCompare(b.node_id);
}

function rgbaCss(color) {
  if (!color) return null;
  return "rgba(" + color.r + "," + color.g + "," + color.b + "," + (color.a / 255) + ")";
}

function inlineImageHref(resource) {
  const value = resource?.inline_data_url;
  if (resource?.kind !== "image" || typeof value !== "string") return null;
  return /^data:image\/(?:png|jpeg|jpg|gif);base64,[A-Za-z0-9+/=]+$/.test(value) ? value : null;
}

function inlineFontHref(resource) {
  const value = resource?.inline_data_url;
  if (resource?.kind !== "font" || typeof value !== "string") return null;
  return /^data:font\/(?:ttf|otf);base64,[A-Za-z0-9+/=]+$/.test(value) ? value : null;
}

function fontFingerprint(resource) {
  const value = resource?.expected_sha256 ?? resource?.content_hash ?? null;
  return typeof value === "string" && /^[0-9a-f]{64}$/.test(value) ? value : null;
}

function fontFamily(resource) {
  const fingerprint = fontFingerprint(resource);
  return fingerprint ? "ChapteraEditor_" + fingerprint.slice(0, 16) : null;
}

function textColorCss(value) {
  return Array.isArray(value) && value.length === 3
    ? "rgb(" + value.map(Number).join(" ") + ")"
    : "rgba(0,0,0,0.9)";
}

export function resolvedEditorTextPlan(node, resources, page, view) {
  const layout = node?.text_layout;
  if (node?.visual_authority !== "reader_scene" || layout?.disposition !== "shared_resolved") {
    return null;
  }
  const bounds = node.text_bounds ?? node.bounds;
  if (!bounds) return null;
  const x = safeInteger(bounds.x, "text.bounds.x");
  const y = safeInteger(bounds.y, "text.bounds.y");
  const width = safeInteger(bounds.width, "text.bounds.width");
  const height = safeInteger(bounds.height, "text.bounds.height");
  const fontSize = safeInteger(layout.font_size_emu, "text.font_size_emu");
  const lineHeight = safeInteger(layout.line_height_emu, "text.line_height_emu");
  const verticalOffset = safeInteger(layout.vertical_offset_emu ?? 0, "text.vertical_offset_emu");
  if (width <= 0 || height <= 0 || fontSize <= 0 || lineHeight <= 0) return null;
  if (verticalOffset < 0 || verticalOffset > height) return null;

  const baseResource = resources.get(layout.font_resource_id) ?? null;
  const baseHref = inlineFontHref(baseResource);
  const baseFingerprint = fontFingerprint(baseResource);
  const baseFamily = fontFamily(baseResource);
  if (!baseHref || !baseFingerprint || !baseFamily) return null;
  if (baseFingerprint !== layout.font_fingerprint_sha256) return null;

  const faces = new Map();
  faces.set(baseResource.resource_id, Object.freeze({
    resource_id: baseResource.resource_id,
    family: baseFamily,
    mime: baseResource.mime,
    href: baseHref,
    fingerprint_sha256: baseFingerprint,
  }));

  const lines = [];
  let cursorY = y + verticalOffset;
  for (const line of [...(layout.lines ?? [])].sort((left, right) => left.line_index - right.line_index)) {
    const lineIndex = safeInteger(line.line_index, "text.line_index");
    const currentLineHeight = safeInteger(line.line_height_emu, "text.line_height_emu");
    const measuredWidth = safeInteger(line.measured_width_emu, "text.measured_width_emu");
    const lineOffset = safeInteger(line.x_offset_emu ?? 0, "text.x_offset_emu");
    if (currentLineHeight <= 0 || measuredWidth < 0 || lineOffset < 0 || lineOffset + measuredWidth > width) return null;
    const lineTop = cursorY - y;
    if (lineTop < 0 || lineTop >= height) return null;

    const spans = [];
    for (const span of line.spans ?? []) {
      const scalarStart = safeInteger(span.scalar_start, "text.span.scalar_start");
      const scalarEnd = safeInteger(span.scalar_end, "text.span.scalar_end");
      const xOffset = safeInteger(span.x_offset_emu, "text.span.x_offset_emu");
      const spanWidth = safeInteger(span.measured_width_emu, "text.span.measured_width_emu");
      const spanFontSize = safeInteger(span.font_size_emu, "text.span.font_size_emu");
      if (scalarEnd <= scalarStart || xOffset < 0 || spanWidth < 0 || spanFontSize <= 0) return null;
      let family = baseFamily;
      if (span.font_resource_id != null || span.font_fingerprint_sha256 != null) {
        if (typeof span.font_resource_id !== "string" || typeof span.font_fingerprint_sha256 !== "string") return null;
        const spanResource = resources.get(span.font_resource_id) ?? null;
        const spanHref = inlineFontHref(spanResource);
        const spanFingerprint = fontFingerprint(spanResource);
        const spanFamily = fontFamily(spanResource);
        if (!spanHref || !spanFingerprint || !spanFamily) return null;
        if (spanFingerprint !== span.font_fingerprint_sha256) return null;
        family = spanFamily;
        faces.set(spanResource.resource_id, Object.freeze({
          resource_id: spanResource.resource_id,
          family: spanFamily,
          mime: spanResource.mime,
          href: spanHref,
          fingerprint_sha256: spanFingerprint,
        }));
      }
      spans.push(Object.freeze({
        scalar_start: scalarStart,
        scalar_end: scalarEnd,
        text: String(span.text ?? ""),
        x: page.x + emuToCss(x + lineOffset + xOffset, view),
        measured_width_css_px: emuToCss(spanWidth, view),
        font_size_css_px: emuToCss(spanFontSize, view),
        font_family: family,
        font_resource_id: span.font_resource_id ?? null,
      }));
    }

    lines.push(Object.freeze({
      line_index: lineIndex,
      text: String(line.text ?? ""),
      x: page.x + emuToCss(x + lineOffset, view),
      y: page.y + emuToCss(cursorY, view),
      measured_width_css_px: emuToCss(measuredWidth, view),
      line_height_css_px: emuToCss(currentLineHeight, view),
      spans: Object.freeze(spans),
    }));
    cursorY += currentLineHeight;
  }

  return Object.freeze({
    authority: "server-shared-resolved",
    viewport: Object.freeze({
      x: page.x + emuToCss(x, view),
      y: page.y + emuToCss(y, view),
      width: emuToCss(width, view),
      height: emuToCss(height, view),
    }),
    font_resource_id: baseResource.resource_id,
    font_family: baseFamily,
    font_size_css_px: emuToCss(fontSize, view),
    line_height_css_px: emuToCss(lineHeight, view),
    fill: textColorCss(layout.color_rgb),
    lines: Object.freeze(lines),
    font_faces: Object.freeze([...faces.values()]),
  });
}

function previewTextCss(style, view) {
  const size = Number.isSafeInteger(style?.font_size_emu)
    ? emuToCss(style.font_size_emu, view)
    : 12;
  const rgb = Array.isArray(style?.color_rgb) && style.color_rgb.length === 3
    ? style.color_rgb
    : null;
  const fill = rgb ? "rgb(" + rgb.map(Number).join(" ") + ")" : "rgba(0,0,0,0.9)";
  return Object.freeze({ font_size_css_px: size, fill });
}

export function buildRenderPlan(snapshot, rawView = {}) {
  assertSceneSourceNeutral(snapshot);
  if (snapshot?.protocol_version !== "chaptera.scene.v1") {
    throw new Error("renderer requires BrowserSceneSnapshotV1");
  }
  const view = normalizeView(rawView);
  const pages = [...snapshot.pages].sort((a, b) => a.order - b.order);
  const paints = new Map(snapshot.paints.map((paint) => [paint.paint_id, paint]));
  const stories = new Map(snapshot.stories.map((story) => [story.story_id, story]));
  const storyByNode = new Map(
    snapshot.story_frames.map((frame) => [frame.node_id, stories.get(frame.story_id) ?? null])
  );
  const resources = new Map(snapshot.resources.map((resource) => [resource.resource_id, resource]));
  const diagnosticsByNode = new Map();
  for (const diagnostic of snapshot.diagnostics) {
    if (!diagnostic.origin_node_id) continue;
    const bucket = diagnosticsByNode.get(diagnostic.origin_node_id) ?? [];
    bucket.push(diagnostic);
    diagnosticsByNode.set(diagnostic.origin_node_id, bucket);
  }

  let yCursor = view.page_margin_css_px + view.pan_y_css_px;
  const pagePlans = [];
  const nodeById = new Map();

  for (const page of pages) {
    const pageX = view.page_margin_css_px + view.pan_x_css_px;
    const pageY = yCursor;
    const pageWidth = emuToCss(page.width_emu, view);
    const pageHeight = emuToCss(page.height_emu, view);
    let nodes = snapshot.nodes.filter((node) => node.page_id === page.page_id);
    let stackingAuthority = "snapshot_order";
    if (
      snapshot.stacking_fidelity === "exact" &&
      nodes.every((node) => Number.isInteger(node.z_order) && Number.isInteger(node.paint_order))
    ) {
      nodes = [...nodes].sort(compareExactStacking);
      stackingAuthority = "exact";
    }

    const nodePlans = nodes.map((node) => {
      Object.entries(node.bounds).forEach(([key, value]) => safeInteger(value, "node.bounds." + key));
      const paint = node.paint_id ? paints.get(node.paint_id) ?? null : null;
      const story = storyByNode.get(node.node_id) ?? null;
      const resource = node.resource_id ? resources.get(node.resource_id) ?? null : null;
      const resolvedText = story
        ? resolvedEditorTextPlan(node, resources, { x: pageX, y: pageY }, view)
        : null;
      const plan = Object.freeze({
        node_id: node.node_id,
        page_id: node.page_id,
        kind: node.kind,
        x: pageX + emuToCss(node.bounds.x, view),
        y: pageY + emuToCss(node.bounds.y, view),
        width: emuToCss(node.bounds.width, view),
        height: emuToCss(node.bounds.height, view),
        paint: Object.freeze({
          fill: rgbaCss(paint?.fill ?? null),
          stroke: rgbaCss(paint?.stroke?.color ?? null),
          stroke_width_css_px: emuToCss(paint?.stroke?.width_emu ?? 0, view)
        }),
        story: story ? Object.freeze({
          text: story.text,
          text_fidelity: story.text_fidelity,
          authority: node.visual_authority === "reader_scene"
            ? "reader_scene_preview"
            : "browser_preview_only",
          style: previewTextCss(node.preview_text_style, view),
          resolved_text: resolvedText
        }) : null,
        resource: resource ? Object.freeze({
          resource_id: resource.resource_id,
          kind: resource.kind,
          availability: resource.availability,
          mime: resource.mime,
          content_hash: resource.content_hash,
          fetch_handle: resource.fetch_handle,
          inline_data_url: inlineImageHref(resource)
        }) : null,
        diagnostics: Object.freeze([...(diagnosticsByNode.get(node.node_id) ?? [])]),
        canonical_bounds: Object.freeze({ ...node.bounds })
      });
      nodeById.set(node.node_id, plan);
      return plan;
    });

    pagePlans.push(Object.freeze({
      page_id: page.page_id,
      order: page.order,
      x: pageX,
      y: pageY,
      width: pageWidth,
      height: pageHeight,
      stacking_authority: stackingAuthority,
      nodes: Object.freeze(nodePlans)
    }));
    yCursor += pageHeight + view.page_gap_css_px;
  }

  return Object.freeze({
    protocol_version: "chaptera.render-plan.v1",
    document_id: snapshot.document_id,
    revision_id: snapshot.revision_id,
    snapshot_id: snapshot.snapshot_id,
    view,
    pages: Object.freeze(pagePlans),
    nodeById,
    fidelity: snapshot.fidelity,
    diagnostics: Object.freeze([...snapshot.diagnostics])
  });
}

export function buildOverlayPlan(renderPlan, overlay = {}) {
  const selectedNodeId = overlay.selected_node_id ?? null;
  const selected = selectedNodeId ? renderPlan.nodeById.get(selectedNodeId) ?? null : null;
  let selectedBounds = selected
    ? { x: selected.x, y: selected.y, width: selected.width, height: selected.height }
    : null;

  if (overlay.preview_bounds && selected) {
    const page = renderPlan.pages.find((item) => item.page_id === selected.page_id);
    if (!page) throw new Error("selected node page is missing");
    const b = overlay.preview_bounds;
    Object.entries(b).forEach(([key, value]) => safeInteger(value, "preview_bounds." + key));
    selectedBounds = {
      x: page.x + emuToCss(b.x, renderPlan.view),
      y: page.y + emuToCss(b.y, renderPlan.view),
      width: emuToCss(b.width, renderPlan.view),
      height: emuToCss(b.height, renderPlan.view)
    };
  }

  return Object.freeze({
    selected_node_id: selectedNodeId,
    selected_bounds: selectedBounds ? Object.freeze(selectedBounds) : null
  });
}

function clearHost(host) {
  while (host.firstChild) host.removeChild(host.firstChild);
}

function hostSize(host) {
  return {
    width: Math.max(1, Math.floor(host.clientWidth || 1200)),
    height: Math.max(1, Math.floor(host.clientHeight || 800))
  };
}

function svgNode(tag, attrs) {
  const node = document.createElementNS("http://www.w3.org/2000/svg", tag);
  for (const [key, value] of Object.entries(attrs)) node.setAttribute(key, String(value));
  return node;
}

function appendResolvedSvgText(root, story) {
  const plan = story?.resolved_text;
  if (!plan) return false;
  const local = svgNode("svg", {
    x: plan.viewport.x,
    y: plan.viewport.y,
    width: plan.viewport.width,
    height: plan.viewport.height,
    viewBox: "0 0 " + plan.viewport.width + " " + plan.viewport.height,
    overflow: "hidden",
    "data-text-viewport": "fixed-frame",
  });
  for (const line of plan.lines) {
    const text = svgNode("text", {
      x: line.x - plan.viewport.x,
      y: line.y - plan.viewport.y,
      "font-family": plan.font_family,
      "font-size": plan.font_size_css_px,
      fill: plan.fill,
      "text-rendering": "geometricPrecision",
      "dominant-baseline": "text-before-edge",
      "data-text-authority": plan.authority,
      "data-text-line-index": line.line_index,
      "data-measured-width-css-px": line.measured_width_css_px,
    });
    text.setAttribute("xml:space", "preserve");
    if (line.spans.length) {
      for (const span of line.spans) {
        const tspan = svgNode("tspan", {
          x: span.x - plan.viewport.x,
          "font-family": span.font_family,
          "font-size": span.font_size_css_px,
          "data-text-span-start": span.scalar_start,
          "data-text-span-end": span.scalar_end,
          "data-measured-width-css-px": span.measured_width_css_px,
          "data-font-resource-id": span.font_resource_id,
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
  root.appendChild(local);
  return true;
}

function appendSvgFontFaces(root, plan) {
  const faces = new Map();
  for (const page of plan.pages) {
    for (const node of page.nodes) {
      for (const face of node.story?.resolved_text?.font_faces ?? []) {
        faces.set(face.resource_id, face);
      }
    }
  }
  if (!faces.size) return;
  const style = svgNode("style", { "data-reader-font-resources": faces.size });
  style.textContent = [...faces.values()].map((face) => {
    const format = face.mime === "font/otf" ? "opentype" : "truetype";
    return '@font-face{font-family:"' + face.family + '";src:url("' + face.href
      + '") format("' + format + '");font-style:normal;font-weight:normal;}';
  }).join("\n");
  root.appendChild(style);
}

class SvgRenderer {
  constructor(host, snapshot, view) {
    this.host = host;
    this.snapshot = snapshot;
    this.view = normalizeView(view);
    this.overlay = {};
    this.renderBase();
  }
  renderBase() {
    clearHost(this.host);
    const size = hostSize(this.host);
    this.root = svgNode("svg", { width: size.width, height: size.height, "data-renderer": "svg" });
    this.host.appendChild(this.root);
    this.plan = buildRenderPlan(this.snapshot, this.view);
    appendSvgFontFaces(this.root, this.plan);
    for (const page of this.plan.pages) {
      this.root.appendChild(svgNode("rect", {
        x: page.x, y: page.y, width: page.width, height: page.height,
        fill: "white", stroke: "rgba(0,0,0,0.25)", "data-page-id": page.page_id
      }));
      for (const node of page.nodes) {
        this.root.appendChild(svgNode("rect", {
          x: node.x, y: node.y, width: node.width, height: node.height,
          fill: node.paint.fill ?? "rgba(120,120,120,0.08)",
          stroke: node.paint.stroke ?? (node.diagnostics.length ? "rgba(180,80,0,0.9)" : "rgba(0,0,0,0.15)"),
          "stroke-width": Math.max(0.5, node.paint.stroke_width_css_px || 0.5),
          "data-node-id": node.node_id
        }));
        if (node.resource?.inline_data_url) {
          const image = svgNode("image", {
            x: node.x,
            y: node.y,
            width: node.width,
            height: node.height,
            preserveAspectRatio: "none",
            "data-resource-id": node.resource.resource_id,
            "data-resource-authority": "reader-scene-inline"
          });
          image.setAttribute("href", node.resource.inline_data_url);
          this.root.appendChild(image);
        }
        if (node.story && !appendResolvedSvgText(this.root, node.story)) {
          const label = svgNode("text", {
            x: node.x + 2,
            y: node.y + node.story.style.font_size_css_px,
            "font-size": node.story.style.font_size_css_px,
            fill: node.story.style.fill,
            "data-text-authority": node.story.authority,
            "data-preview-reason": node.visual_authority === "reader_scene"
              ? "shared_plan_or_font_unavailable"
              : "browser_scene_preview"
          });
          label.textContent = node.story.text.slice(0, 120);
          this.root.appendChild(label);
        } else if (node.resource && !node.resource.inline_data_url && !node.story) {
          const label = svgNode("text", { x: node.x + 2, y: node.y + 14, "font-size": 10 });
          label.textContent = "image:" + node.resource.availability;
          this.root.appendChild(label);
        }
      }
    }
    this.overlayGroup = svgNode("g", { "data-layer": "transient-overlay" });
    this.root.appendChild(this.overlayGroup);
    this.renderOverlay();
  }
  renderOverlay() {
    while (this.overlayGroup.firstChild) this.overlayGroup.removeChild(this.overlayGroup.firstChild);
    const overlay = buildOverlayPlan(this.plan, this.overlay);
    if (!overlay.selected_bounds) return;
    const b = overlay.selected_bounds;
    this.overlayGroup.appendChild(svgNode("rect", {
      x: b.x, y: b.y, width: b.width, height: b.height,
      fill: "none", stroke: "rgba(0,80,220,0.95)", "stroke-width": 2,
      "data-selection-node-id": overlay.selected_node_id
    }));
  }
  setView(view) { this.view = normalizeView(view); this.renderBase(); }
  updateOverlay(overlay) { this.overlay = { ...overlay }; this.renderOverlay(); }
  stats() { return { available: true, renderer: "svg", dom_elements: this.host.querySelectorAll("*").length }; }
  destroy() { clearHost(this.host); }
}

function setupCanvas(host, rendererName) {
  clearHost(host);
  const size = hostSize(host);
  host.style.position = "relative";
  const base = document.createElement("canvas");
  const overlay = document.createElement("canvas");
  base.width = overlay.width = size.width;
  base.height = overlay.height = size.height;
  base.dataset.renderer = rendererName;
  overlay.dataset.layer = "transient-overlay";
  [base, overlay].forEach((canvas) => {
    canvas.style.position = "absolute";
    canvas.style.inset = "0";
  });
  overlay.style.pointerEvents = "none";
  host.append(base, overlay);
  return { ...size, base, overlay };
}

function drawOverlay(canvas, plan, overlayState) {
  const context = canvas.getContext("2d");
  context.clearRect(0, 0, canvas.width, canvas.height);
  const overlay = buildOverlayPlan(plan, overlayState);
  if (!overlay.selected_bounds) return;
  const b = overlay.selected_bounds;
  context.strokeStyle = "rgba(0,80,220,0.95)";
  context.lineWidth = 2;
  context.strokeRect(b.x, b.y, b.width, b.height);
}

class Canvas2dRenderer {
  constructor(host, snapshot, view) {
    this.host = host;
    this.snapshot = snapshot;
    this.view = normalizeView(view);
    this.overlay = {};
    this.surface = setupCanvas(host, "canvas2d");
    this.renderBase();
  }
  renderBase() {
    this.plan = buildRenderPlan(this.snapshot, this.view);
    const context = this.surface.base.getContext("2d");
    context.clearRect(0, 0, this.surface.width, this.surface.height);
    for (const page of this.plan.pages) {
      context.fillStyle = "white";
      context.fillRect(page.x, page.y, page.width, page.height);
      context.strokeStyle = "rgba(0,0,0,0.25)";
      context.strokeRect(page.x, page.y, page.width, page.height);
      for (const node of page.nodes) {
        if (node.paint.fill) {
          context.fillStyle = node.paint.fill;
          context.fillRect(node.x, node.y, node.width, node.height);
        }
        context.strokeStyle = node.paint.stroke ?? (node.diagnostics.length ? "rgba(180,80,0,0.9)" : "rgba(0,0,0,0.15)");
        context.lineWidth = Math.max(0.5, node.paint.stroke_width_css_px || 0.5);
        context.strokeRect(node.x, node.y, node.width, node.height);
        if (node.story) {
          context.fillStyle = "rgba(0,0,0,0.9)";
          context.font = "12px sans-serif";
          context.fillText(node.story.text.slice(0, 120), node.x + 2, node.y + 14);
        } else if (node.resource) {
          context.fillStyle = "rgba(0,0,0,0.7)";
          context.font = "10px sans-serif";
          context.fillText("image:" + node.resource.availability, node.x + 2, node.y + 14);
        }
      }
    }
    drawOverlay(this.surface.overlay, this.plan, this.overlay);
  }
  setView(view) { this.view = normalizeView(view); this.renderBase(); }
  updateOverlay(overlay) { this.overlay = { ...overlay }; drawOverlay(this.surface.overlay, this.plan, this.overlay); }
  stats() { return { available: true, renderer: "canvas2d", dom_elements: this.host.querySelectorAll("*").length }; }
  destroy() { clearHost(this.host); }
}

function fillWebGlRect(gl, canvasHeight, rect, color) {
  const x = Math.floor(rect.x);
  const y = Math.floor(canvasHeight - rect.y - rect.height);
  const width = Math.max(0, Math.ceil(rect.width));
  const height = Math.max(0, Math.ceil(rect.height));
  if (!width || !height) return;
  const parts = color?.match(/[\d.]+/g)?.map(Number) ?? [120, 120, 120, 0.12];
  gl.scissor(x, y, width, height);
  gl.clearColor(parts[0] / 255, parts[1] / 255, parts[2] / 255, parts[3] ?? 1);
  gl.clear(gl.COLOR_BUFFER_BIT);
}

class WebGl2HybridRenderer {
  constructor(host, snapshot, view) {
    this.host = host;
    this.snapshot = snapshot;
    this.view = normalizeView(view);
    this.overlay = {};
    this.surface = setupCanvas(host, "webgl2-hybrid");
    this.gl = this.surface.base.getContext("webgl2", { antialias: false, preserveDrawingBuffer: true });
    if (!this.gl) {
      this.available = false;
      this.unavailableReason = "webgl2_context_unavailable";
      return;
    }
    this.available = true;
    this.textLayer = document.createElement("div");
    this.textLayer.dataset.layer = "text-diagnostics";
    this.textLayer.style.position = "absolute";
    this.textLayer.style.inset = "0";
    this.textLayer.style.pointerEvents = "none";
    this.host.insertBefore(this.textLayer, this.surface.overlay);
    this.renderBase();
  }
  renderBase() {
    if (!this.available) return;
    this.plan = buildRenderPlan(this.snapshot, this.view);
    const gl = this.gl;
    gl.enable(gl.SCISSOR_TEST);
    gl.viewport(0, 0, this.surface.width, this.surface.height);
    gl.scissor(0, 0, this.surface.width, this.surface.height);
    gl.clearColor(0.94, 0.94, 0.94, 1);
    gl.clear(gl.COLOR_BUFFER_BIT);
    this.textLayer.replaceChildren();
    for (const page of this.plan.pages) {
      fillWebGlRect(gl, this.surface.height, page, "rgba(255,255,255,1)");
      for (const node of page.nodes) {
        fillWebGlRect(
          gl, this.surface.height, node,
          node.paint.fill ?? (node.diagnostics.length ? "rgba(255,190,90,0.35)" : "rgba(120,120,120,0.12)")
        );
        if (node.story || node.resource || node.diagnostics.length) {
          const label = document.createElement("div");
          label.dataset.nodeId = node.node_id;
          label.dataset.textAuthority = node.story?.authority ?? "none";
          label.style.position = "absolute";
          label.style.left = (node.x + 2) + "px";
          label.style.top = (node.y + 2) + "px";
          label.style.maxWidth = Math.max(1, node.width - 4) + "px";
          label.style.overflow = "hidden";
          label.style.whiteSpace = "nowrap";
          label.style.font = "11px sans-serif";
          label.textContent = node.story?.text.slice(0, 120) ??
            (node.resource ? "image:" + node.resource.availability : "diagnostic");
          this.textLayer.appendChild(label);
        }
      }
    }
    drawOverlay(this.surface.overlay, this.plan, this.overlay);
  }
  setView(view) { this.view = normalizeView(view); this.renderBase(); }
  updateOverlay(overlay) {
    if (!this.available) return;
    this.overlay = { ...overlay };
    drawOverlay(this.surface.overlay, this.plan, this.overlay);
  }
  stats() {
    return {
      available: this.available,
      renderer: "webgl2-hybrid",
      unavailable_reason: this.available ? null : this.unavailableReason,
      dom_elements: this.host.querySelectorAll("*").length
    };
  }
  destroy() { clearHost(this.host); }
}

export function createRenderer(kind, host, snapshot, view = {}) {
  if (!RENDERER_KINDS.includes(kind)) throw new Error("unknown renderer kind: " + kind);
  if (kind === "svg") return new SvgRenderer(host, snapshot, view);
  if (kind === "canvas2d") return new Canvas2dRenderer(host, snapshot, view);
  return new WebGl2HybridRenderer(host, snapshot, view);
}
