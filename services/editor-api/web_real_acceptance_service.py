#!/usr/bin/env python3
"""Real pinned-PUB backend for WEB-ACCEPTANCE-LOCAL-PRODUCER-01.

This reuses the public RevisionKernel/HTTP protocol and the already-proven
Rar-owned bounded producers:
- canonical pub-editor operations through the pinned Rust producer;
- LAYOUT-RESOLVED-SCENE-01 for current resolved-graph -> Scene;
- the bounded editable-export authority for IDML/ODG;
- the gated native-PUB writer for proven final Editor states.

It is task-local integration glue, not a second document model.
"""

from __future__ import annotations

import argparse
import copy
import hashlib
import json
import pathlib
import re
import subprocess
import sys
import threading
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from urllib.parse import parse_qs, unquote, urlsplit

ROOT = pathlib.Path(__file__).resolve().parents[2]
TOOLS = ROOT / "tools"
sys.path.insert(0, str(TOOLS))

from adapt_viewer_scene_v1 import adapt_viewer_geometry
from scene_v1 import finalize_snapshot
from resolved_graph_scene_bridge_v1 import (
    apply_project_to_resolved_graph,
    compare_viewer_and_adapter_scene,
    project_resolved_graph_scene,
)
from validate_export_preview import validate_schema as validate_export_preview_schema
from validate_export_preview import validate_semantics as validate_export_preview_semantics
from verify_editable_export_geometry import RectEmu, verify_export

from font_authoring_admission_v1 import issue_font_authoring_admission_v1
from live_font_delivery_v1 import (
    LiveFontDeliveryDenied, bind_font_set_to_scene, build_font_environment,
    issue_current_admission, read_current_exact_font,
)
from pinned_opentype_resource_v1 import ABEL_RESOURCE_ID, ABEL_SHA256, load_pinned_abel
from revision_store import RevisionKernel
from story_range_v1 import replace_story_range_v1
from security.authz_v1 import AuthzDenied, AuthzKernel, CAP_EDIT_TEXT, CAP_EXPORT, CAP_VIEW
from security.authorized_revision_gateway import AuthorizedRevisionGateway

PINNED_SHA = "6a825ba26ba35d6e885acdc62e859591ed37cb0ff7480b554b9cb362b644dfcf"
PINNED_LEN = 291840
SAMPLE3_SHA = "424c69173ff08948c2529c8084b4ac2403f1ff1057146f4edd02fc29b44481fc"
SAMPLE3_LEN = 72192
SAMPLE4_SHA = "42195f7ad23d911219fea3ec88e66e867e9b9a6821a16dd1b535e2aa9d57a11b"
SAMPLE4_LEN = 72192
# This is a source fixture admission list, not a Writer/Publisher approval list.
PINNED_FIXTURE_PROFILES = {
    "newsletter": (PINNED_SHA, PINNED_LEN),
    "newsletter-font": (PINNED_SHA, PINNED_LEN),
    "sample3": (SAMPLE3_SHA, SAMPLE3_LEN),
    "sample4": (SAMPLE4_SHA, SAMPLE4_LEN),
}

# Native Publisher 2019 Open -> SaveAs -> fresh Reopen -> Reader evidence:
# six-unit Story deletion: protected-main #37982376097 (receipt 11641486612);
# one-unit Story deletion: protected-main #37987117653 (receipt 11644202672).
# Both apply to exact Sample3 input bytes; no other Story mutation is approved.
# Sample4 cross-fixture Story candidate was independently accepted by the
# paired original/candidate Publisher 2019 SaveAs + fresh Reopen on
# protected main Actions #38052655117 (source-safe artifact 11669902218).
# This is exact-byte authorization, NEVER arbitrary Reader-green PUB output.
NATIVE_PUBLISHER_ACCEPTED_SHA_PAIRS_V1 = frozenset({
    (
        "424c69173ff08948c2529c8084b4ac2403f1ff1057146f4edd02fc29b44481fc",
        "a92543b6f2b6ac3a8ae2481e15a2188a338ddc2a92832580f8987079fa4f70f8",
    ),
    (
        "424c69173ff08948c2529c8084b4ac2403f1ff1057146f4edd02fc29b44481fc",
        "b9b789f35a34e016faceb27acf50bf0621273a7d612762ecc17c56ca450fe715",
    ),
    (
        SAMPLE4_SHA,
        "2f7795a3c4307716565c7accf4636a80b7947f8921c60a87b51e1a48ee529509",
    ),
})


def publisher_authorizes_exact_bytes(source_sha: str, report: dict) -> bool:
    """Transfer only Publisher-accepted exact bytes; Reader proof is insufficient."""
    return (
        report.get("can_serialize") is True
        and report.get("chaptera_reopen_verified") is True
        and (source_sha, report.get("output_hash"))
        in NATIVE_PUBLISHER_ACCEPTED_SHA_PAIRS_V1
    )

STATE = None


def canonical_json(value) -> bytes:
    return json.dumps(
        value,
        ensure_ascii=False,
        sort_keys=True,
        separators=(",", ":"),
    ).encode("utf-8")


def load_json(path: pathlib.Path):
    value = json.loads(path.read_text(encoding="utf-8"))
    if not isinstance(value, dict):
        raise RuntimeError(f"{path.name} must contain a JSON object")
    return value


def sha256_path(path: pathlib.Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def _scene_node_identity(node: dict, label: str) -> tuple[str, str, str | None]:
    if not isinstance(node, dict):
        raise RuntimeError(f"{label} must be an object")
    origin = node.get("origin")
    parent_origin = node.get("parent_origin")
    instance_id = node.get("instance_id")
    if not isinstance(origin, str) or not isinstance(parent_origin, str):
        raise RuntimeError(f"{label} identity is incomplete")
    if instance_id is not None and not isinstance(instance_id, str):
        raise RuntimeError(f"{label}.instance_id must be a string when present")
    return origin, parent_origin, instance_id


def align_adapter_scene_to_viewer_node_order(
    viewer: dict,
    adapter_scene: dict,
) -> dict:
    """Preserve Viewer-proven paint slots without borrowing Viewer geometry."""

    viewer_scene = viewer.get("scene")
    if not isinstance(viewer_scene, dict):
        raise RuntimeError("Viewer receipt scene is required")
    viewer_nodes = viewer_scene.get("nodes")
    adapter_nodes = adapter_scene.get("nodes")
    if not isinstance(viewer_nodes, list) or not isinstance(adapter_nodes, list):
        raise RuntimeError("Viewer/adapter Scene nodes must be arrays")

    adapter_by_identity: dict[tuple[str, str, str | None], dict] = {}
    for index, node in enumerate(adapter_nodes):
        key = _scene_node_identity(node, f"adapter_scene.nodes[{index}]")
        if key in adapter_by_identity:
            raise RuntimeError("adapter Scene contains duplicate node identity")
        adapter_by_identity[key] = node

    viewer_order: list[tuple[str, str, str | None]] = []
    viewer_seen: set[tuple[str, str, str | None]] = set()
    for index, node in enumerate(viewer_nodes):
        key = _scene_node_identity(node, f"viewer_scene.nodes[{index}]")
        if key in viewer_seen:
            raise RuntimeError("Viewer Scene contains duplicate node identity")
        viewer_seen.add(key)
        viewer_order.append(key)

    if viewer_seen != set(adapter_by_identity):
        raise RuntimeError("Viewer/adapter Scene node identity sets differ")

    aligned = copy.deepcopy(adapter_scene)
    aligned["nodes"] = [copy.deepcopy(adapter_by_identity[key]) for key in viewer_order]
    # #522 restores source-backed paint order only on Scene nodes. The
    # pub-layout origin_mapping remains in canonical projection order and is
    # intentionally left untouched here so exact Viewer Scene equality holds.
    return aligned


class RealAcceptanceState:
    def __init__(
        self,
        *,
        fixture: pathlib.Path,
        resolved_graph: pathlib.Path,
        viewer_receipt: pathlib.Path,
        revision_receipt: pathlib.Path | None,
        exporter: pathlib.Path,
        work_dir: pathlib.Path,
        strict_acceptance: bool = True,
        baseline_project: pathlib.Path | None = None,
        fixture_profile: str = "newsletter",
        pinned_abel_demo: bool = False,
        font_worker: pathlib.Path | None = None,
    ):
        self.fixture = fixture.resolve(strict=True)
        self.resolved_graph_path = resolved_graph.resolve(strict=True)
        self.viewer_receipt_path = viewer_receipt.resolve(strict=True)
        self.revision_receipt_path = (
            revision_receipt.resolve(strict=True) if revision_receipt is not None else None
        )
        self.exporter = exporter.resolve(strict=True)
        self.work_dir = work_dir.resolve()
        self.work_dir.mkdir(parents=True, exist_ok=True)
        self.strict_acceptance = strict_acceptance
        self.font_worker = font_worker.resolve(strict=True) if font_worker is not None else None

        if fixture_profile not in PINNED_FIXTURE_PROFILES:
            raise RuntimeError("unrecognized pinned PUB fixture profile")
        self.fixture_profile = fixture_profile
        if pinned_abel_demo and strict_acceptance:
            raise RuntimeError("pinned Abel is only available in explicit interactive demo mode")
        self.pinned_abel = load_pinned_abel() if pinned_abel_demo else None
        self.pinned_sha, self.pinned_len = PINNED_FIXTURE_PROFILES[fixture_profile]
        if (
            self.fixture.stat().st_size != self.pinned_len
            or sha256_path(self.fixture) != self.pinned_sha
        ):
            raise RuntimeError("pinned PUB source identity mismatch")

        if self.revision_receipt_path is not None:
            if baseline_project is not None or fixture_profile != "newsletter":
                raise RuntimeError("canonical revision receipt requires Newsletter profile")
            self.revision_receipt = load_json(self.revision_receipt_path)
            self.document_id = self.revision_receipt["document_id"]
            self.source_hash = self.revision_receipt["source_hash"]
            if self.source_hash != self.pinned_sha:
                raise RuntimeError("revision receipt is not bound to pinned source")
            self.baseline_project = copy.deepcopy(
                self.revision_receipt["baseline"]["project"]
            )
            self.expected_baseline_revision = self.revision_receipt["baseline"]["revision_id"]
            self.expected_accepted_revision = self.revision_receipt["accepted"]["revision_id"]
            self.canonical_operation = copy.deepcopy(
                self.revision_receipt["accepted"]["canonical_operation"]
            )
            self.canonical_request = copy.deepcopy(self.revision_receipt["request"])
            self.target_node_id = self.canonical_operation["node_id"]
            self.target_after = copy.deepcopy(self.canonical_operation["after"])
        elif fixture_profile == "newsletter-font":
            # Explicit authoring demo: start from a real source-backed Rust
            # identity-bearing EditorProject, not the geometry-only Producer B
            # baseline. The project/Scene document IDs MUST match.
            if (strict_acceptance or not pinned_abel_demo or baseline_project is None
                    or self.font_worker is None):
                raise RuntimeError("font demo requires an independent real Rust baseline and worker")
            self.baseline_project = load_json(baseline_project.resolve(strict=True))
            identity = self.baseline_project.get("identity")
            if (not isinstance(identity, dict)
                    or not isinstance(identity.get("document_id"), str)
                    or self.baseline_project.get("source_hash") != self.pinned_sha
                    or self.baseline_project.get("operations") != []):
                raise RuntimeError("font demo baseline must be an immutable real PUB Project")
            self.document_id = identity["document_id"]
            self.source_hash = self.pinned_sha
            self.revision_receipt = None
            self.expected_baseline_revision = None
            self.expected_accepted_revision = None
            self.canonical_operation = None
            self.canonical_request = None
            self.target_node_id = None
            self.target_after = None
        else:
            # An isolated, exact-source interactive scenario, not a fabricated
            # Producer B move receipt. The canonical baseline is real Rust EditorProject.
            if (
                fixture_profile not in {"sample3", "sample4"}
                or strict_acceptance
                or baseline_project is None
            ):
                raise RuntimeError("pinned Story fixture requires interactive exact baseline Project")
            self.revision_receipt = None
            self.document_id = (
                "c8cb238d-0d7f-4ddd-81cc-6f7a142da066"
                if fixture_profile == "sample4" else
                "a75950c7-cfb5-4b6c-925b-27a8d8b3d102"
            )
            self.source_hash = self.pinned_sha
            self.baseline_project = load_json(baseline_project.resolve(strict=True))
            self.expected_baseline_revision = None
            self.expected_accepted_revision = None
            self.canonical_operation = None
            self.canonical_request = None
            self.target_node_id = None
            self.target_after = None

        self.kernel = RevisionKernel()
        baseline = self.kernel.register_baseline(
            document_id=self.document_id,
            source_hash=self.source_hash,
            project=copy.deepcopy(self.baseline_project),
        )
        if self.expected_baseline_revision is None:
            self.expected_baseline_revision = baseline.revision_id
        elif baseline.revision_id != self.expected_baseline_revision:
            raise RuntimeError("RevisionKernel baseline differs from Producer B")

        self.tenant_id = "web-acceptance-real"
        self.authz = AuthzKernel()
        for principal, role in (
            ("synthetic-editor", "editor"),
            ("synthetic-viewer", "viewer"),
        ):
            self.authz.set_role(
                tenant_id=self.tenant_id,
                document_id=self.document_id,
                principal_id=principal,
                role=role,
            )
        self.gateway = AuthorizedRevisionGateway(
            kernel=self.kernel,
            authz=self.authz,
            tenant_id=self.tenant_id,
        )

        self.commit_requests = 0
        self.history_requests = 0
        self.executor_calls = 0
        self.history_executor_calls = 0
        self.redo_stack = []
        self.last_operation = None
        self.reopen_count = 0
        self.export_cache = {}
        self.native_pub_cache = {}
        # One Editor revision maps to one immutable, receipt-backed PUB candidate.
        # The threaded HTTP server may receive the browser's disclosure preview
        # concurrently with an acceptance/client preview; do not unlink and
        # recreate the same candidate paths in two Rust child processes.
        self.native_pub_lock = threading.Lock()

        baseline_scene = self._scene_from_project(
            self.baseline_project,
            self.expected_baseline_revision,
            require_viewer_equivalence=self.strict_acceptance,
        )
        self.scenes = {self.expected_baseline_revision: baseline_scene}

    def _fresh_graph_and_viewer(self):
        graph = load_json(self.resolved_graph_path)
        viewer = load_json(self.viewer_receipt_path)
        return graph, viewer

    def _scene_from_project(
        self,
        project: dict,
        revision_id: str,
        *,
        require_viewer_equivalence: bool = False,
    ) -> dict:
        graph, viewer = self._fresh_graph_and_viewer()
        projection_project = project
        if self.fixture_profile == "newsletter-font":
            # Rust re-admits the complete resource from disk on every revision
            # reconstruction. Geometry below remains source-only: font glyph
            # reshaping/overset is NOT implemented, never silently projected.
            operations = project.get("operations")
            if not isinstance(operations, list) or any(
                not isinstance(op, dict)
                or op.get("kind") != "set_text_format_property"
                or op.get("property") != "font_resource"
                for op in operations
            ):
                raise ValueError("font-only demo refuses unproven mixed mutations")
            self._run_font_worker("probe", project)
            projection_project = copy.deepcopy(project)
            projection_project["operations"] = []
        current_graph = apply_project_to_resolved_graph(graph, projection_project)
        viewer_pages = viewer.get("document", {}).get("pages")
        if not isinstance(viewer_pages, list):
            raise ValueError("Viewer receipt document.pages must be an array")
        viewer_page_ids = []
        for index, page in enumerate(viewer_pages):
            if not isinstance(page, dict) or not isinstance(page.get("id"), str):
                raise ValueError(f"Viewer receipt document.pages[{index}].id is required")
            viewer_page_ids.append(page["id"])
        if self.fixture_profile in {"sample3", "sample4"}:
            # The legacy Sample3/Sample4 Viewer may have a Page/surface that
            # is not in resolved_graph.document.pages. A Story-only edit cannot
            # legitimately re-project/omit that surface to satisfy the newer
            # mature-0x2C Scene bridge. Keep the actual Reader Viewer geometry,
            # prove it is bound to the immutable source, and only bridge canonical
            # Story text. This path cannot admit a geometry/materialization edit.
            operations = project.get("operations")
            if not isinstance(operations, list) or any(
                not isinstance(op, dict) or op.get("kind") != "replace_story_range"
                for op in operations
            ):
                raise RuntimeError("pinned legacy Viewer Story projection forbids geometry edits")
            if current_graph.get("document", {}).get("source_hash") != self.source_hash:
                raise RuntimeError("resolved graph source identity changed")
            if viewer.get("document", {}).get("source", {}).get("source_hash") != self.source_hash:
                raise RuntimeError("Viewer source identity changed")
            before_stories = graph.get("stories")
            viewer_stories = viewer.get("document", {}).get("stories")
            if not isinstance(before_stories, dict) or not isinstance(viewer_stories, list):
                raise RuntimeError("legacy Story catalogs are not verifiable")
            for item in viewer_stories:
                origin = before_stories.get(item.get("id"))
                if not isinstance(origin, dict) or origin.get("text") != item.get("text"):
                    raise RuntimeError("legacy Viewer Story differs from immutable Reader graph")
            current_viewer = copy.deepcopy(viewer)
        else:
            source_scene = project_resolved_graph_scene(
                current_graph,
                page_ids=viewer_page_ids,
            )
            source_scene = align_adapter_scene_to_viewer_node_order(viewer, source_scene)
            if require_viewer_equivalence:
                compare_viewer_and_adapter_scene(viewer, source_scene)
            current_viewer = copy.deepcopy(viewer)
            current_viewer["scene"] = source_scene

        # Story text in the browser snapshot follows the canonical edited
        # resolved graph. Geometry still comes from the same Viewer/Scene path;
        # this does not introduce browser text layout authority.
        graph_stories = current_graph.get("stories")
        viewer_stories = current_viewer.get("document", {}).get("stories")
        if not isinstance(graph_stories, dict) or not isinstance(viewer_stories, list):
            raise RuntimeError("resolved/viewer Story collections are required")
        for story in viewer_stories:
            if not isinstance(story, dict) or not isinstance(story.get("id"), str):
                raise RuntimeError("Viewer Story identity is required")
            graph_story = graph_stories.get(story["id"])
            if not isinstance(graph_story, dict) or not isinstance(graph_story.get("text"), str):
                raise RuntimeError("Viewer Story is missing from edited resolved graph")
            story["text"] = graph_story["text"]

        browser_scene = adapt_viewer_geometry(current_viewer, self.document_id, revision_id)
        if self.fixture_profile == "newsletter-font" and project["operations"]:
            # Canonical authoring changed, but no glyph metrics or frame flow
            # was recalculated. Make the product fidelity gate explicit in
            # every Scene revision and fresh reopen; never render as complete.
            browser_scene["fidelity"]["state"] = "partial"
            browser_scene["fidelity"]["reasons"] = sorted(set(
                browser_scene["fidelity"]["reasons"]
                + ["font_resource_layout_not_implemented"]
            ))
            browser_scene = finalize_snapshot(browser_scene)
        return bind_font_set_to_scene(browser_scene, self.pinned_abel)

    def _run_font_worker(
        self, mode: str, project: dict, scene: dict | None = None,
        intent: dict | None = None,
        story_id: str | None = None,
    ) -> dict:
        if (self.fixture_profile != "newsletter-font" or self.font_worker is None
                or self.pinned_abel is None or mode not in {"probe", "apply", "glyph-spans", "line-fit"}):
            raise ValueError("pinned real-PUB font worker is not admitted")
        token = hashlib.sha256(canonical_json(
            {"mode": mode, "project": project, "scene": scene, "intent": intent,
             "story_id": story_id}
        )).hexdigest()[:24]
        project_path = self.work_dir / f"{token}.font.project.json"
        project_path.write_bytes(canonical_json(project) + b"\n")
        args = [str(self.font_worker), mode, str(self.fixture), str(project_path)]
        if mode == "apply":
            if not isinstance(scene, dict) or not isinstance(intent, dict):
                raise ValueError("trusted Scene and bounded font intent required")
            scene_path = self.work_dir / f"{token}.trusted-font-scene.json"
            intent_path = self.work_dir / f"{token}.font-intent.json"
            scene_path.write_bytes(canonical_json(scene) + b"\n")
            intent_path.write_bytes(canonical_json(intent) + b"\n")
            args.extend((str(scene_path), str(intent_path)))
        if mode in {"glyph-spans", "line-fit"}:
            if not isinstance(story_id, str) or not re.fullmatch(
                r"[0-9a-f]{8}-(?:[0-9a-f]{4}-){3}[0-9a-f]{12}", story_id,
            ):
                raise ValueError("current physical glyph query requires canonical StoryId")
            args.append(story_id)
        completed = subprocess.run(
            args, cwd=ROOT, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
            text=True, check=False, timeout=30,
        )
        if completed.returncode:
            # Rust error text may include source paths and Story payloads.
            # Never return it over the authenticated HTTP API.
            raise ValueError("pinned real-PUB Rust font command rejected")
        try:
            result = json.loads(completed.stdout)
        except json.JSONDecodeError as error:
            raise RuntimeError("Rust font worker returned invalid JSON") from error
        expected = {
            "probe": "chaptera.local-font-format-probe.v1",
            "apply": "chaptera.local-pinned-font-apply.v1",
            "glyph-spans": "chaptera.local-current-exact-glyph-spans.v1",
            "line-fit": "chaptera.local-current-physical-line-fit.v1",
        }[mode]
        if (not isinstance(result, dict)
                or result.get("protocol_version") != expected
                or result.get("source_hash") != self.source_hash):
            raise RuntimeError("Rust font worker returned wrong physical/source authority")
        return result

    def font_editing_scope(self) -> dict:
        current = self.kernel.current_revision(self.document_id)
        scene = self.scenes[current.revision_id]
        stories = []
        if self.fixture_profile == "newsletter-font":
            result = self._run_font_worker("probe", current.project)
            stories = result["stories"]
        return {
            "protocol_version": "chaptera.font-format-edit-scope.v1",
            "document_id": self.document_id,
            "revision_id": scene["revision_id"],
            "scene_snapshot_id": scene["snapshot_id"],
            "stories": stories,
        }

    def current_physical_glyph_spans(
        self, story_id: str, expected_revision: str, expected_snapshot: str,
    ) -> dict:
        """Trusted current-Project glyph stream, not a text-flow or PDF grant."""
        if self.fixture_profile != "newsletter-font":
            raise ValueError("physical glyph projection unavailable outside pinned authoring demo")
        current = self.kernel.current_revision(self.document_id)
        scene = self.scenes[current.revision_id]
        if (expected_revision != scene["revision_id"]
                or expected_snapshot != scene["snapshot_id"]):
            raise ValueError("stale_font_glyph_scene")
        if not isinstance(story_id, str) or not re.fullmatch(
            r"[0-9a-f]{8}-(?:[0-9a-f]{4}-){3}[0-9a-f]{12}", story_id,
        ):
            raise ValueError("invalid StoryId for physical glyph projection")
        probe = self._run_font_worker("probe", current.project)
        current_story = next(
            (entry for entry in probe["stories"] if entry["story_id"] == story_id),
            None,
        )
        if current_story is None:
            raise ValueError("target Story lacks admitted canonical format overlay")
        result = self._run_font_worker(
            "glyph-spans", current.project, story_id=story_id,
        )
        if (result.get("project_state_id") != probe["project_state_id"]
                or result.get("story_id") != story_id
                or result.get("story_scalar_len") != current_story["story_scalar_len"]
                or result.get("story_format_state_hash") != current_story["expected_state_hash"]
                or result.get("authoritative_line_breaks") is not False
                or result.get("fixed_pdf_allowed") is not False):
            raise RuntimeError("real Rust glyph stream lacks current Project/source authority")
        parts = result.get("spans")
        if not isinstance(parts, list):
            raise RuntimeError("Rust glyph stream missing bounded segments")
        cursor = 0
        admitted = unresolved = glyphs = 0
        for span in parts:
            if (not isinstance(span, dict)
                    or type(span.get("start_scalar")) is not int
                    or type(span.get("end_scalar")) is not int
                    or span["start_scalar"] != cursor
                    or span["end_scalar"] <= cursor):
                raise RuntimeError("Rust glyph span partition is invalid")
            cursor = span["end_scalar"]
            if span.get("kind") == "source_unresolved":
                binding = span.get("source_font_binding_id")
                if not isinstance(binding, str) or not binding:
                    raise RuntimeError("unknown source font was concealed")
                unresolved += span["end_scalar"] - span["start_scalar"]
            elif span.get("kind") == "admitted_exact":
                identity = span.get("identity")
                shaped = span.get("shaped")
                if (not isinstance(identity, dict)
                        or identity.get("resource_id") != ABEL_RESOURCE_ID
                        or identity.get("content_hash") != ABEL_SHA256
                        or not isinstance(shaped, dict)
                        or not isinstance(shaped.get("glyphs"), list)):
                    raise RuntimeError("glyph spans no longer have admitted full physical bytes")
                if any(
                    type(glyph.get("cluster")) is not int
                    or not span["start_scalar"] <= glyph["cluster"] < span["end_scalar"]
                    for glyph in shaped["glyphs"]
                    if isinstance(glyph, dict)
                ) or any(not isinstance(glyph, dict) for glyph in shaped["glyphs"]):
                    raise RuntimeError("shaped glyph escaped its canonical scalar interval")
                admitted += span["end_scalar"] - span["start_scalar"]
                glyphs += len(shaped["glyphs"])
            else:
                raise RuntimeError("unknown source/physical glyph distinction")
        if (cursor != result["story_scalar_len"]
                or admitted != result.get("admitted_scalar_count")
                or unresolved != result.get("source_unresolved_scalar_count")
                or glyphs != result.get("shaped_glyph_count")
                or admitted + unresolved != cursor
                or result.get("all_scalars_shaped") is not (unresolved == 0)):
            raise RuntimeError("physical-glyph totals do not match canonical Story")
        # Never infer Publisher line breaks from measured spans. Retain exact
        # glyph positions for future real frame placement consumer.
        return {
            "protocol_version": "chaptera.current-physical-glyph-spans.v1",
            "document_id": self.document_id,
            "source_hash": self.source_hash,
            "revision_id": scene["revision_id"],
            "scene_snapshot_id": scene["snapshot_id"],
            "story_id": story_id,
            "project_state_id": result["project_state_id"],
            "story_format_state_hash": result["story_format_state_hash"],
            "story_scalar_len": cursor,
            "spans": copy.deepcopy(parts),
            "admitted_scalar_count": admitted,
            "source_unresolved_scalar_count": unresolved,
            "shaped_glyph_count": glyphs,
            "all_scalars_shaped": result["all_scalars_shaped"],
            "authoritative_line_breaks": False,
            "fixed_pdf_allowed": False,
        }

    def current_physical_line_fit(
        self, story_id: str, expected_revision: str, expected_snapshot: str,
    ) -> dict:
        """Bounded Unicode line-fit, never Publisher/native PDF authority."""
        physical = self.current_physical_glyph_spans(
            story_id, expected_revision, expected_snapshot,
        )
        current = self.kernel.current_revision(self.document_id)
        # The exact current project is the only source of physical spans,
        # authoring overrides and reciprocal linked-frame geometry. No client
        # supplied font metrics, line advance, Project or frame layout.
        result = self._run_font_worker(
            "line-fit", current.project, story_id=story_id,
        )
        preview = result.get("flow")
        if (result.get("project_state_id") != physical["project_state_id"]
                or result.get("story_id") != story_id
                or result.get("story_scalar_len") != physical["story_scalar_len"]
                or result.get("story_format_state_hash") != physical["story_format_state_hash"]
                or not isinstance(preview, dict)
                or preview.get("protocol_version") != "chaptera.current-mixed-font-line-fit.v1"
                or preview.get("story_id") != story_id
                or preview.get("story_format_state_hash") != physical["story_format_state_hash"]
                or preview.get("story_scalar_len") != physical["story_scalar_len"]
                or preview.get("native_publisher_layout_authoritative") is not False
                or preview.get("fixed_pdf_allowed") is not False):
            raise RuntimeError("Rust line-fit differs from admitted current glyph history")
        gaps = preview.get("source_gaps")
        if not isinstance(gaps, list):
            raise RuntimeError("Rust current line-fit omitted source font gaps")
        original_gaps = [
            {
                "start_scalar": part["start_scalar"],
                "end_scalar": part["end_scalar"],
                "source_font_binding_id": part["source_font_binding_id"],
            }
            for part in physical["spans"] if part["kind"] == "source_unresolved"
        ]
        if gaps != original_gaps:
            raise RuntimeError("line-fit concealed original font source gap")
        state = preview.get("state")
        if state == "source_font_unresolved":
            if (not gaps or physical["all_scalars_shaped"] is not False
                    or preview.get("lines") != []
                    or preview.get("overset_start_scalar") is not None
                    or preview.get("unicode_breaks_evaluated") is not False):
                raise RuntimeError("unresolved original font cannot produce preview lines")
        elif state == "physical_line_fit_preview":
            if (gaps or physical["all_scalars_shaped"] is not True
                    or preview.get("unicode_breaks_evaluated") is not True
                    or not isinstance(preview.get("lines"), list)
                    or not (preview["lines"] or
                            preview.get("overset_start_scalar") is not None)):
                raise RuntimeError("unadmitted font silently marked full line fit")
        else:
            raise RuntimeError("unsupported current physical line-fit state")
        return {
            "protocol_version": "chaptera.current-physical-line-fit.v1",
            "document_id": self.document_id,
            "source_hash": self.source_hash,
            "revision_id": expected_revision,
            "scene_snapshot_id": expected_snapshot,
            "story_id": story_id,
            "project_state_id": physical["project_state_id"],
            "story_format_state_hash": physical["story_format_state_hash"],
            "flow": copy.deepcopy(preview),
        }

    def _run_editor_command(self, mode: str, project: dict, command: dict) -> dict:
        token = hashlib.sha256(
            canonical_json({"mode": mode, "project": project, "command": command})
        ).hexdigest()[:20]
        project_path = self.work_dir / f"{token}.editor-project.json"
        command_path = self.work_dir / f"{token}.editor-command.json"
        project_path.write_bytes(canonical_json(project) + b"\n")
        command_path.write_bytes(canonical_json(command) + b"\n")
        completed = subprocess.run(
            [
                str(self.exporter),
                mode,
                str(self.fixture),
                str(project_path),
                str(command_path),
            ],
            cwd=ROOT,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
            check=False,
        )
        if completed.returncode != 0:
            raise ValueError(f"canonical editor command rejected ({mode})")
        try:
            result = json.loads(completed.stdout)
        except json.JSONDecodeError as error:
            raise RuntimeError("canonical editor command returned invalid JSON") from error
        if not isinstance(result, dict) or result.get("source_hash") != self.source_hash:
            raise RuntimeError("canonical editor command source identity mismatch")
        if not isinstance(result.get("project"), dict):
            raise RuntimeError("canonical editor command project missing")
        return result

    def executor(self, project: dict, command: dict):
        self.executor_calls += 1
        if not isinstance(command, dict):
            raise ValueError("editor command must be an object")

        kind = command.get("kind")
        if self.fixture_profile == "newsletter-font" and kind != "set_admitted_font_resource":
            # Geometry and native Story text consumers cannot replay mixed
            # font-resource authoring. Reject BEFORE RevisionKernel mutates.
            raise ValueError("font-only authoring mode forbids unproved mixed edits")
        if kind == "set_admitted_font_resource":
            if self.fixture_profile != "newsletter-font":
                raise ValueError("only the exact real-PUB font demo can commit")
            current = self.kernel.current_revision(self.document_id).revision_id
            scene = self.scenes[current]
            intent = {
                "protocol_version": "chaptera.local-pinned-font-intent.v1",
                "story_id": command["story_id"],
                "start_scalar": command["start_scalar"],
                "end_scalar": command["end_scalar"],
                "expected_state_hash": command["expected_state_hash"],
                "candidate": command["candidate"],
            }
            result = self._run_font_worker("apply", project, scene, intent)
            operation = result["canonical_operation"]
            authored = result["project"]
            if (result.get("fixed_output_eligible") is not False
                    or result.get("layout_authority") != "partial_not_reshaped"
                    or result.get("fresh_reopen_with_exact_bytes") is not True
                    or authored.get("operations", []) != project.get("operations", []) + [operation]
                    or authored.get("identity") != project.get("identity")
                    or authored.get("source_hash") != self.source_hash):
                raise RuntimeError("Rust font operation lacks durable independent replay proof")
            self.redo_stack.clear()
            self.last_operation = copy.deepcopy(operation)
            return (
                copy.deepcopy(operation), copy.deepcopy(authored),
                [{"key": "story.font_resource", "state": "partial",
                  "note": "actual_font_change_unshaped_fixed_output_blocked"}],
            )
        if kind == "move_node_to":
            result = self._run_editor_command("editor-move-node", project, command)
            operation = result.get("operation")
            if not isinstance(operation, dict) or operation.get("kind") != "move_node":
                raise RuntimeError("Rust editor returned non-MoveNode operation")
            self.redo_stack.clear()
            self.last_operation = copy.deepcopy(operation)
            return (
                copy.deepcopy(operation),
                copy.deepcopy(result["project"]),
                [{"key": "node.geometry.position", "state": "supported", "note": None}],
            )

        if kind == "replace_story_range":
            result = self._run_editor_command("editor-story-range", project, command)
            before_text = result.get("before_text")
            after_text = result.get("after_text")
            if not isinstance(before_text, str) or not isinstance(after_text, str):
                raise RuntimeError("Rust editor Story result is incomplete")

            canonical = replace_story_range_v1(
                story_id=command.get("story_id"),
                story_text=before_text,
                start_scalar=command.get("start_scalar"),
                end_scalar=command.get("end_scalar"),
                expected_before=command.get("expected_before"),
                replacement_text=command.get("replacement_text"),
            )
            if canonical.after_text != after_text:
                raise RuntimeError("Rust/Python canonical Story result differs")

            rust_operation = result.get("operation")
            if not isinstance(rust_operation, dict):
                raise RuntimeError("Rust editor Story operation missing")
            for field in (
                "kind",
                "story_id",
                "start_scalar",
                "end_scalar",
                "expected_before",
                "replacement_text",
                "before_story_state_id",
                "after_story_state_id",
            ):
                if rust_operation.get(field) != canonical.operation.get(field):
                    raise RuntimeError(
                        f"Rust/Python canonical Story operation differs at {field}"
                    )

            self.redo_stack.clear()
            self.last_operation = copy.deepcopy(canonical.operation)
            return (
                copy.deepcopy(canonical.operation),
                copy.deepcopy(result["project"]),
                [{"key": "story.text", "state": "supported", "note": None}],
            )

        raise ValueError("unsupported real editor command")

    def history_executor(self, base_project: dict, transition_kind: str):
        self.history_executor_calls += 1
        project = copy.deepcopy(base_project)
        operations = list(project.get("operations", []))
        if transition_kind == "undo":
            if not operations:
                raise ValueError("nothing to undo")
            self.redo_stack.append(copy.deepcopy(operations.pop()))
        elif transition_kind == "redo":
            if not self.redo_stack:
                raise ValueError("nothing to redo")
            operations.append(self.redo_stack.pop())
        else:
            raise ValueError("unsupported history transition")
        project["operations"] = operations
        if self.fixture_profile == "newsletter-font":
            self._run_font_worker("probe", project)
        if project.get("identity") is None:
            # Legacy no-identity Projects (both pinned Newsletter Producer B
            # and interactive Newsletter smoke) normalize to v0.2 with no
            # MoveNode, and v0.4 when MoveNode survives. This is required for
            # the existing immutable public Undo/Redo history state hashes.
            project["schema_version"] = (
                "pub-editor-v0.4"
                if any(
                    isinstance(operation, dict)
                    and operation.get("kind") == "move_node"
                    for operation in operations
                )
                else "pub-editor-v0.2"
            )
        else:
            # Identity-bearing canonical Rust EditorProjects cannot be
            # downgraded below v0.11. Preserve schema across history changes.
            # In this fixture-constrained acceptance service these are the
            # exact-source interactive Sample3 and Sample4 projects.
            if self.fixture_profile == "newsletter":
                raise RuntimeError("Newsletter unexpectedly gained a project identity")
        return project, [{"key": "history." + transition_kind, "state": "supported", "note": None}]

    def commit(self, request: dict, principal_id: str) -> dict:
        protocol = request.get("protocol_version")
        if protocol in {
            "chaptera.commit-request.v1",
            "chaptera.story-range-intent.v1",
            "chaptera.font-resource-intent.v1",
        }:
            result = self.gateway.commit(
                request,
                principal_id=principal_id,
                executor=self.executor,
            )
        elif protocol == "chaptera.history-transition-intent.v1":
            result = self.gateway.commit(
                request,
                principal_id=principal_id,
                history_executor=self.history_executor,
            )
        else:
            raise ValueError("unsupported commit protocol")

        self.commit_requests += 1
        if protocol == "chaptera.history-transition-intent.v1":
            self.history_requests += 1

        if result.get("protocol_version") in {
            "chaptera.commit-accepted.v1",
            "chaptera.history-transition-accepted.v1",
        }:
            record = self.kernel.read_revision(
                document_id=self.document_id,
                revision_id=result["revision_id"],
            )
            self.scenes[result["revision_id"]] = self._scene_from_project(
                record.project,
                result["revision_id"],
            )
            if (
                self.strict_acceptance
                and protocol == "chaptera.commit-request.v1"
                and self.executor_calls == 1
            ):
                if self.last_operation != self.canonical_operation:
                    raise RuntimeError("browser MoveNode differs from canonical Producer B operation")
                if result["revision_id"] != self.expected_accepted_revision:
                    raise RuntimeError("browser accepted revision differs from Producer B")
        return copy.deepcopy(result)

    def _export_for_revision(self, revision_id: str, target: str):
        if self.fixture_profile == "newsletter-font":
            raise ValueError("font_layout_unverified")
        key = (revision_id, target)
        if key in self.export_cache:
            return self.export_cache[key]
        if target not in {"idml", "odg"}:
            raise ValueError("export target must be idml or odg")

        record = self.kernel.read_revision(
            document_id=self.document_id,
            revision_id=revision_id,
        )
        token = revision_id.removeprefix("sha256:")[:20]
        project_path = self.work_dir / f"{token}.project.json"
        artifact_path = self.work_dir / f"{token}.{target}"
        report_path = self.work_dir / f"{token}.export-report.json"
        project_path.write_bytes(canonical_json(record.project) + b"\n")

        completed = subprocess.run(
            [
                str(self.exporter),
                "editable-export",
                str(self.fixture),
                str(project_path),
                target,
                str(artifact_path),
                str(report_path),
            ],
            cwd=ROOT,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
            check=False,
        )
        if completed.returncode != 0:
            raise RuntimeError(
                "editable export producer failed: " + completed.stderr.strip()
            )

        report = load_json(report_path)
        if report.get("schema_version") != "0.1":
            raise RuntimeError("unexpected export report schema")
        if report.get("source", {}).get("source_hash") != self.source_hash:
            raise RuntimeError("export report source identity mismatch")

        fence = report.get("conversion_fence")
        preview = {
            "protocol_version": "chaptera.export-preview.v1",
            "report_schema_version": report["schema_version"],
            "document_id": self.document_id,
            "source_hash": self.source_hash,
            "revision_id": revision_id,
            "target": copy.deepcopy(report["target"]),
            "conversion_fence_sha256": (
                fence.get("digest_sha256") if isinstance(fence, dict) else None
            ),
            "can_serialize": report["can_serialize"],
            "counts": copy.deepcopy(report["counts"]),
            "items": copy.deepcopy(report["items"]),
        }
        validate_export_preview_schema(preview)
        validate_export_preview_semantics(preview)

        value = {
            "preview": preview,
            "artifact": artifact_path,
            "report": report,
        }
        self.export_cache[key] = value
        return value

    def export_preview(self, target: str) -> dict:
        current = self.kernel.current_revision(self.document_id)
        return copy.deepcopy(
            self._export_for_revision(current.revision_id, target)["preview"]
        )

    def export_proof(self, target: str = "idml") -> dict:
        current = self.kernel.current_revision(self.document_id)
        value = self._export_for_revision(current.revision_id, target)
        rect = RectEmu(
            self.target_after["x"],
            self.target_after["y"],
            self.target_after["width"],
            self.target_after["height"],
        )
        return verify_export(
            value["artifact"],
            target,
            self.target_node_id,
            rect,
        )

    def font_environment(self) -> dict:
        revision_id = self.kernel.current_revision(self.document_id).revision_id
        return build_font_environment(self.scenes[revision_id], self.pinned_abel)

    def font_authoring_admission(self) -> dict:
        """Server-side exact resource grant, default-empty without opt-in."""
        revision_id = self.kernel.current_revision(self.document_id).revision_id
        return issue_current_admission(
            scene=self.scenes[revision_id],
            tenant_id=self.tenant_id,
            resource=self.pinned_abel,
        )

    def font_resource_bytes(self, fetch_handle: str) -> bytes:
        revision = self.kernel.current_revision(self.document_id).revision_id
        scene = self.scenes[revision]
        return read_current_exact_font(
            scene=scene, tenant_id=self.tenant_id,
            resource=self.pinned_abel,
            resource_id=ABEL_RESOURCE_ID,
            revision_id=scene["revision_id"], snapshot_id=scene["snapshot_id"],
            fetch_handle=fetch_handle,
        )

    def editor_capabilities(self) -> dict:
        current = self.kernel.current_revision(self.document_id)
        if self.fixture_profile == "newsletter-font":
            # Source-geometry Scene cannot certify arbitrary text mutation
            # after adding resource overrides. Font-range editing uses a
            # separate exact resource probe with an authoritative state hash.
            return {
                "protocol_version": "chaptera.editor-capabilities.v1",
                "document_id": self.document_id,
                "source_hash": self.source_hash,
                "revision_id": current.revision_id,
                "editable_story_ids": [],
            }
        record = self.kernel.read_revision(
            document_id=self.document_id,
            revision_id=current.revision_id,
        )
        token = current.revision_id.removeprefix("sha256:")[:20]
        project_path = self.work_dir / f"{token}.capabilities.project.json"
        project_path.write_bytes(canonical_json(record.project) + b"\n")
        completed = subprocess.run(
            [
                str(self.exporter),
                "editor-capabilities",
                str(self.fixture),
                str(project_path),
            ],
            cwd=ROOT,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
            check=False,
        )
        if completed.returncode != 0:
            raise RuntimeError("editor capabilities producer failed")
        try:
            result = json.loads(completed.stdout)
        except json.JSONDecodeError as error:
            raise RuntimeError("editor capabilities producer returned invalid JSON") from error
        editable = result.get("editable_story_ids")
        if (
            not isinstance(result, dict)
            or result.get("protocol_version") != "chaptera.editor-capabilities.v1"
            or result.get("source_hash") != self.source_hash
            or not isinstance(editable, list)
            or any(not isinstance(story_id, str) for story_id in editable)
            or editable != sorted(set(editable))
        ):
            raise RuntimeError("editor capabilities result is invalid")
        return {
            "protocol_version": "chaptera.editor-capabilities.v1",
            "document_id": self.document_id,
            "source_hash": self.source_hash,
            "revision_id": current.revision_id,
            "editable_story_ids": editable,
        }

    def _native_pub_for_revision(self, revision_id: str) -> dict:
        # Serialize cache lookup, write-once native materialization and receipt
        # validation. The lock is per document instance and never guards other
        # unrelated Reader/Editor/HTTP operations.
        with self.native_pub_lock:
            return self._native_pub_for_revision_serialized(revision_id)

    def _native_pub_for_revision_serialized(self, revision_id: str) -> dict:
        if revision_id in self.native_pub_cache:
            return self.native_pub_cache[revision_id]
        if self.fixture_profile == "newsletter-font":
            self._run_font_worker(
                "probe", self.kernel.read_revision(
                    document_id=self.document_id, revision_id=revision_id,
                ).project,
            )
            blocked = {
                "preview": {
                    "protocol_version": "chaptera.native-pub-save-preview.v1",
                    "document_id": self.document_id, "source_hash": self.source_hash,
                    "revision_id": revision_id, "can_serialize": False,
                    "can_download": False, "native_publisher_authorized": False,
                    "download_blocker_code": "font_layout_unverified",
                    "blocker_code": "font_layout_unverified",
                    "output_hash": None, "byte_len": None,
                    "chaptera_reopen_verified": False,
                    "native_publisher_acceptance": "not_evaluated",
                },
                "artifact": None, "report": None,
            }
            self.native_pub_cache[revision_id] = blocked
            return blocked

        record = self.kernel.read_revision(
            document_id=self.document_id,
            revision_id=revision_id,
        )
        token = revision_id.removeprefix("sha256:")[:20]
        project_path = self.work_dir / f"{token}.native-pub.project.json"
        artifact_path = self.work_dir / f"{token}.edited.pub"
        report_path = self.work_dir / f"{token}.native-pub.report.json"
        project_path.write_bytes(canonical_json(record.project) + b"\n")
        artifact_path.unlink(missing_ok=True)
        report_path.unlink(missing_ok=True)

        completed = subprocess.run(
            [
                str(self.exporter),
                "native-pub-save",
                str(self.fixture),
                str(project_path),
                str(artifact_path),
                str(report_path),
            ],
            cwd=ROOT,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
            check=False,
        )
        if completed.returncode != 0:
            # The real service is reachable over HTTP. Never echo the Rust
            # stderr here: it can include private source paths or document
            # content. Classify only fixed, source-independent producer stages
            # so hosted acceptance can diagnose a failure without leaking it.
            signatures = (
                ("native PUB save requires two distinct, unused output paths", "output_paths_occupied"),
                ("read source PUB fixture", "source_read"),
                ("parse canonical EditorProject", "project_parse"),
                ("source SHA-256 does not match EditorProject", "source_hash_mismatch"),
                ("open bounded native PUB editor", "editor_open"),
                ("replay canonical EditorProject", "project_replay"),
                ("write source-safe blocked native PUB save report", "blocked_receipt_write"),
                ("write new native PUB candidate", "candidate_write"),
                ("write source-safe native PUB save report", "candidate_receipt_write"),
                ("unexpected extra arguments", "cli_args"),
            )
            failure_code = next(
                (code for message, code in signatures if message in completed.stderr),
                "unclassified_nonzero",
            )
            # Keep only the OS error number and path-state booleans, not
            # untrusted CLI stderr, source paths, or candidate bytes.
            errno_match = re.search(r"\(os error (\d+)\)", completed.stderr)
            errno_code = errno_match.group(1) if errno_match else "none"
            artifact_exists = artifact_path.exists()
            parent_exists = artifact_path.parent.is_dir()
            raise RuntimeError(
                "native PUB save producer failed "
                f"[{failure_code};os_errno={errno_code};"
                f"artifact_exists={str(artifact_exists).lower()};"
                f"parent_exists={str(parent_exists).lower()}]"
            )
        report = load_json(report_path)
        if (
            report.get("protocol_version") != "chaptera.native-pub-save.v1"
            or report.get("source_hash") != self.source_hash
            or not isinstance(report.get("can_serialize"), bool)
        ):
            raise RuntimeError("native PUB save report identity/schema mismatch")

        can_serialize = report["can_serialize"]
        if can_serialize:
            if not artifact_path.is_file():
                raise RuntimeError("native PUB save report is ready but artifact is missing")
            output_hash = sha256_path(artifact_path)
            if report.get("output_hash") != output_hash:
                raise RuntimeError("native PUB artifact hash differs from save report")
            if report.get("byte_len") != artifact_path.stat().st_size:
                raise RuntimeError("native PUB artifact length differs from save report")
        elif artifact_path.exists():
            raise RuntimeError("blocked native PUB save emitted an artifact")

        # An arbitrary CFB accepted by Chaptera Reader is not a product Save PUB
        # permission. This separate policy is pinned to real Publisher evidence.
        native_authorized = publisher_authorizes_exact_bytes(self.source_hash, report)
        if native_authorized and not can_serialize:
            raise RuntimeError("Publisher authority without materialized candidate")
        preview = {
            "protocol_version": "chaptera.native-pub-save-preview.v1",
            "document_id": self.document_id,
            "source_hash": self.source_hash,
            "revision_id": revision_id,
            "can_serialize": can_serialize,
            "can_download": native_authorized,
            "native_publisher_authorized": native_authorized,
            "download_blocker_code": (
                None if native_authorized else
                "publisher_exact_sha_evidence_missing" if can_serialize else
                report.get("blocker_code")
            ),
            "blocker_code": report.get("blocker_code"),
            "output_hash": report.get("output_hash"),
            "byte_len": report.get("byte_len"),
            "chaptera_reopen_verified": report.get("chaptera_reopen_verified") is True,
            "native_publisher_acceptance": report.get("native_publisher_acceptance"),
        }
        value = {
            "preview": preview,
            "artifact": artifact_path if can_serialize else None,
            "report": report,
        }
        self.native_pub_cache[revision_id] = value
        return value

    def native_pub_preview(self) -> dict:
        current = self.kernel.current_revision(self.document_id)
        return copy.deepcopy(
            self._native_pub_for_revision(current.revision_id)["preview"]
        )

    def native_pub_artifact(self) -> tuple[pathlib.Path, dict]:
        current = self.kernel.current_revision(self.document_id)
        value = self._native_pub_for_revision(current.revision_id)
        if (
            not value["preview"]["can_download"]
            or not value["preview"]["native_publisher_authorized"]
            or value["artifact"] is None
        ):
            raise ValueError("native_pub_download_not_authorized")
        return value["artifact"], copy.deepcopy(value["preview"])

    def reopen(self) -> dict:
        current = self.kernel.current_revision(self.document_id)
        record = self.kernel.read_revision(
            document_id=self.document_id,
            revision_id=current.revision_id,
        )
        # Re-read immutable inputs and rebuild from the persisted canonical project.
        if (
            self.fixture.stat().st_size != self.pinned_len
            or sha256_path(self.fixture) != self.pinned_sha
        ):
            raise RuntimeError("immutable source changed before reopen")
        fresh = self._scene_from_project(record.project, current.revision_id)
        previous = self.scenes[current.revision_id]
        if fresh != previous:
            raise RuntimeError("fresh reopen Scene differs from persisted revision Scene")
        self.scenes[current.revision_id] = fresh
        self.reopen_count += 1
        return copy.deepcopy(fresh)

    def state(self) -> dict:
        current = self.kernel.current_revision(self.document_id)
        return {
            "receipt_class": "real_pub_browser",
            "real_pub": True,
            "product_acceptance": self.strict_acceptance,
            "interactive": not self.strict_acceptance,
            "document_id": self.document_id,
            "source_hash": self.source_hash,
            "fixture_sha256": sha256_path(self.fixture),
            "fixture_byte_len": self.fixture.stat().st_size,
            "current_revision_id": current.revision_id,
            "current_snapshot_id": self.scenes[current.revision_id]["snapshot_id"],
            "commit_requests": self.commit_requests,
            "history_requests": self.history_requests,
            "executor_calls": self.executor_calls,
            "history_executor_calls": self.history_executor_calls,
            "reopen_count": self.reopen_count,
            "last_operation": copy.deepcopy(self.last_operation),
        }


class Handler(BaseHTTPRequestHandler):
    server_version = "ChapteraRealAcceptance/1"

    def _headers(self, status=200):
        self.send_response(status)
        self.send_header("content-type", "application/json; charset=utf-8")
        self.send_header("cache-control", "no-store")
        self.send_header("access-control-allow-origin", "*")
        self.send_header("access-control-allow-methods", "GET, POST, OPTIONS")
        self.send_header(
            "access-control-allow-headers",
            "content-type, x-chaptera-principal-id, x-chaptera-trace-version, "
            "x-chaptera-trace-id, x-chaptera-interaction-id, "
            "x-chaptera-session-incarnation, x-chaptera-operation-class, "
            "x-chaptera-browser-family",
        )
        self.end_headers()

    def _json(self, value, status=200):
        self._headers(status)
        self.wfile.write(
            json.dumps(value, ensure_ascii=False, sort_keys=True).encode("utf-8")
        )

    def _font_file(self, data: bytes):
        self.send_response(200)
        self.send_header("content-type", "font/ttf")
        self.send_header("content-length", str(len(data)))
        self.send_header("x-content-type-options", "nosniff")
        self.send_header("x-chaptera-font-content-sha256", ABEL_SHA256)
        self.send_header("cache-control", "no-store")
        self.send_header("access-control-allow-origin", "*")
        self.send_header("access-control-expose-headers", "x-chaptera-font-content-sha256")
        self.end_headers()
        self.wfile.write(data)

    def _pub_file(self, path: pathlib.Path):
        data = path.read_bytes()
        self.send_response(200)
        self.send_header("content-type", "application/x-mspublisher")
        self.send_header("content-length", str(len(data)))
        self.send_header(
            "content-disposition",
            'attachment; filename="chaptera-edited.pub"',
        )
        self.send_header("cache-control", "no-store")
        self.send_header("access-control-allow-origin", "*")
        self.end_headers()
        self.wfile.write(data)

    def _principal_id(self):
        value = self.headers.get("x-chaptera-principal-id")
        if not value:
            raise AuthzDenied("principal_missing")
        return value

    def _authorize(self, capability):
        STATE.authz.authorize(
            tenant_id=STATE.tenant_id,
            document_id=STATE.document_id,
            principal_id=self._principal_id(),
            capability=capability,
        )

    def do_OPTIONS(self):
        self._headers(204)

    def do_GET(self):
        parsed = urlsplit(self.path)
        path = unquote(parsed.path)
        query = parse_qs(parsed.query)
        try:
            if path == "/health":
                self._json({
                    "ok": True,
                    "receipt_class": "real_pub_browser",
                    "interactive": not STATE.strict_acceptance,
                    "product_acceptance": STATE.strict_acceptance,
                })
                return
            if path == "/v1/pub-save/preview":
                self._authorize(CAP_EXPORT)
                self._json(STATE.native_pub_preview())
                return
            if path == "/v1/pub-save/download":
                self._authorize(CAP_EXPORT)
                try:
                    artifact, _preview = STATE.native_pub_artifact()
                except ValueError:
                    self._json({"error": "native_pub_download_not_authorized"}, 409)
                    return
                self._pub_file(artifact)
                return
            if path == "/v1/export/preview":
                self._authorize(CAP_EXPORT)
                if STATE.fixture_profile == "newsletter-font":
                    self._json({"error": "font_layout_unverified"}, 409)
                    return
                target = query.get("target", [""])[0]
                self._json(STATE.export_preview(target))
                return
            if path == "/v1/editor/font-format-scope":
                self._authorize(CAP_EDIT_TEXT)
                self._json(STATE.font_editing_scope())
                return
            if path == "/v1/editor/font-glyph-spans":
                self._authorize(CAP_EDIT_TEXT)
                if (set(query) != {"story_id", "revision_id", "snapshot_id"}
                        or any(len(values) != 1 for values in query.values())):
                    self._json({"error": "invalid_font_glyph_scope"}, 400)
                    return
                try:
                    result = STATE.current_physical_glyph_spans(
                        query["story_id"][0], query["revision_id"][0],
                        query["snapshot_id"][0],
                    )
                except ValueError:
                    # Do not leak opaque resource or legacy font internals
                    # to stale or differently authorized browser scopes.
                    self._json({"error": "font_glyph_projection_not_available"}, 409)
                    return
                self._json(result)
                return
            if path == "/v1/editor/font-line-fit":
                self._authorize(CAP_EDIT_TEXT)
                if (set(query) != {"story_id", "revision_id", "snapshot_id"}
                        or any(len(values) != 1 for values in query.values())):
                    self._json({"error": "invalid_font_line_fit_scope"}, 400)
                    return
                try:
                    result = STATE.current_physical_line_fit(
                        query["story_id"][0], query["revision_id"][0],
                        query["snapshot_id"][0],
                    )
                except ValueError:
                    self._json({"error": "font_line_fit_not_available"}, 409)
                    return
                self._json(result)
                return
            if path == "/v1/editor/font-environment":
                self._authorize(CAP_EDIT_TEXT)
                self._json(STATE.font_environment())
                return
            if path.startswith("/v1/editor/font-resource/"):
                self._authorize(CAP_EDIT_TEXT)
                fetch_handle = path.removeprefix("/v1/editor/font-resource/")
                try:
                    font_bytes = STATE.font_resource_bytes(fetch_handle)
                except LiveFontDeliveryDenied:
                    self._json({"error": "font_resource_not_available_or_stale"}, 409)
                    return
                self._font_file(font_bytes)
                return
            if path == "/v1/editor/font-authoring-admission":
                # Capability is not derived from font delivery/visibility.
                # Viewers cannot request authoring options; no client-supplied
                # resource IDs or bytes influence this read-only response.
                self._authorize(CAP_EDIT_TEXT)
                self._json(STATE.font_authoring_admission())
                return
            if path == "/v1/editor/capabilities":
                self._authorize(CAP_VIEW)
                self._json(STATE.editor_capabilities())
                return
            if path == "/v1/scenes/current":
                self._authorize(CAP_VIEW)
                current = STATE.kernel.current_revision(STATE.document_id).revision_id
                self._json(STATE.scenes[current])
                return
            if path.startswith("/v1/scenes/"):
                self._authorize(CAP_VIEW)
                revision_id = path.removeprefix("/v1/scenes/")
                scene = STATE.scenes.get(revision_id)
                if scene is None:
                    self._json({"error": "scene_not_found"}, 404)
                else:
                    self._json(scene)
                return
            if path == "/v1/harness/state":
                self._json(STATE.state())
                return
            if path == "/v1/harness/export-proof":
                self._json(STATE.export_proof(query.get("target", ["idml"])[0]))
                return
            self._json({"error": "not_found"}, 404)
        except AuthzDenied as exc:
            self._json({"error": "forbidden", "code": exc.code}, 403)
        except Exception as exc:
            self._json({"error": "real_acceptance_error", "detail": str(exc)}, 500)

    def do_POST(self):
        path = unquote(self.path.split("?", 1)[0])
        try:
            if path == "/v1/harness/reopen":
                self._json(STATE.reopen())
                return
            if path != "/v1/commit":
                self._json({"error": "not_found"}, 404)
                return
            length = int(self.headers.get("content-length", "0"))
            if length <= 0 or length > 1024 * 1024:
                raise ValueError("invalid request size")
            request = json.loads(self.rfile.read(length).decode("utf-8"))
            result = STATE.commit(request, self._principal_id())
            self._json(result)
        except AuthzDenied as exc:
            self._json({"error": "forbidden", "code": exc.code}, 403)
        except (ValueError, KeyError, TypeError) as exc:
            self._json({"error": "invalid_request", "detail": str(exc)}, 400)
        except Exception as exc:
            self._json({"error": "real_acceptance_error", "detail": str(exc)}, 500)

    def log_message(self, format, *args):
        return


def main():
    global STATE
    parser = argparse.ArgumentParser()
    parser.add_argument("--port", type=int, default=8765)
    parser.add_argument("--fixture", required=True, type=pathlib.Path)
    parser.add_argument("--resolved-graph", required=True, type=pathlib.Path)
    parser.add_argument("--viewer-receipt", required=True, type=pathlib.Path)
    parser.add_argument("--revision-receipt", type=pathlib.Path)
    parser.add_argument("--baseline-project", type=pathlib.Path)
    parser.add_argument(
        "--fixture-profile", choices=("newsletter", "newsletter-font", "sample3", "sample4"), default="newsletter"
    )
    parser.add_argument("--exporter", required=True, type=pathlib.Path)
    parser.add_argument("--font-worker", type=pathlib.Path,
                        help="exact pinned real PUB Rust authoring binary (demo-only)")
    parser.add_argument("--pinned-abel-demo", action="store_true",
                        help="explicit dev-only admitted full-font delivery; never enabled in acceptance mode")
    parser.add_argument("--work-dir", required=True, type=pathlib.Path)
    parser.add_argument(
        "--interactive",
        action="store_true",
        help="allow arbitrary capability-approved MoveNode edits for local product use",
    )
    args = parser.parse_args()

    STATE = RealAcceptanceState(
        fixture=args.fixture,
        resolved_graph=args.resolved_graph,
        viewer_receipt=args.viewer_receipt,
        revision_receipt=args.revision_receipt,
        exporter=args.exporter,
        work_dir=args.work_dir,
        strict_acceptance=not args.interactive,
        baseline_project=args.baseline_project,
        fixture_profile=args.fixture_profile,
        pinned_abel_demo=args.pinned_abel_demo,
        font_worker=args.font_worker,
    )
    server = ThreadingHTTPServer(("127.0.0.1", args.port), Handler)
    print(
        json.dumps(
            {
                "ready": True,
                "port": server.server_address[1],
                "receipt_class": "real_pub_browser",
                "real_pub": True,
                "product_acceptance": not args.interactive,
                "interactive": args.interactive,
            }
        ),
        flush=True,
    )
    try:
        server.serve_forever()
    finally:
        server.server_close()


if __name__ == "__main__":
    main()
