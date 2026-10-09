#!/usr/bin/env python3
"""Isolated exact-SHA Publisher-approved HTTP download acceptance using the production Handler.

This test harness supplies a verified real Rust producer candidate to the existing HTTP
route. It does not claim that the pinned Newsletter UI can edit Sample3.
"""
import argparse
import hashlib
import importlib.util
import json
import pathlib
import subprocess
import sys
import threading
from http.server import ThreadingHTTPServer

ROOT = pathlib.Path(__file__).resolve().parents[1]
SERVICE = ROOT / "services/editor-api/web_real_acceptance_service.py"
SAMPLE3 = "424c69173ff08948c2529c8084b4ac2403f1ff1057146f4edd02fc29b44481fc"
ACCEPTED = "a92543b6f2b6ac3a8ae2481e15a2188a338ddc2a92832580f8987079fa4f70f8"

def sha(data):
    return hashlib.sha256(data).hexdigest()

def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--source", required=True, type=pathlib.Path)
    parser.add_argument("--baseline", required=True, type=pathlib.Path)
    parser.add_argument("--edited", required=True, type=pathlib.Path)
    parser.add_argument("--producer", required=True, type=pathlib.Path)
    parser.add_argument("--out", required=True, type=pathlib.Path)
    args = parser.parse_args()
    args.out.mkdir(parents=True, exist_ok=True)
    source = args.source.read_bytes()
    assert len(source) == 72192 and sha(source) == SAMPLE3
    # Import the real service with the same module search root it receives as a script.
    sys.path.insert(0, str(SERVICE.parent))
    spec = importlib.util.spec_from_file_location("chaptera_real_service", SERVICE)
    service = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(service)

    def produce(project, label):
        out = args.out / (label + ".pub")
        report_path = args.out / (label + ".json")
        completed = subprocess.run([
            str(args.producer), "native-pub-save", str(args.source), str(project),
            str(out), str(report_path),
        ], cwd=ROOT, text=True, capture_output=True)
        if completed.returncode:
            raise RuntimeError("Rust native-pub-save producer failed: " + completed.stderr)
        report = json.loads(report_path.read_text())
        assert report["source_hash"] == SAMPLE3
        if report["can_serialize"]:
            assert out.is_file()
            assert report["output_hash"] == sha(out.read_bytes())
            assert report["byte_len"] == out.stat().st_size
        else:
            assert not out.exists()
        return out, report

    blocked_file, blocked_report = produce(args.baseline, "baseline")
    assert not blocked_report["can_serialize"]
    assert not service.publisher_authorizes_exact_bytes(SAMPLE3, blocked_report)
    accepted_file, accepted_report = produce(args.edited, "accepted")
    assert accepted_report["can_serialize"] and accepted_report["chaptera_reopen_verified"]
    assert accepted_report["output_hash"] == ACCEPTED
    assert service.publisher_authorizes_exact_bytes(SAMPLE3, accepted_report)
    assert not service.publisher_authorizes_exact_bytes("0" * 64, accepted_report)
    tampered = dict(accepted_report, output_hash="0" * 64)
    assert not service.publisher_authorizes_exact_bytes(SAMPLE3, tampered)
    assert sha(args.source.read_bytes()) == SAMPLE3

    class LocalAuthz:
        def authorize(self, *, principal_id, **_):
            if principal_id != "synthetic-editor":
                raise service.AuthzDenied("no_export")

    class State:
        authz = LocalAuthz()
        tenant_id = "single-source-test"
        document_id = "sample3-test"
        strict_acceptance = True

        @staticmethod
        def native_pub_preview():
            return {
                "protocol_version": "chaptera.native-pub-save-preview.v1",
                "source_hash": SAMPLE3, "can_serialize": True,
                "can_download": True, "native_publisher_authorized": True,
                "output_hash": ACCEPTED, "byte_len": accepted_file.stat().st_size,
            }

        @staticmethod
        def native_pub_artifact():
            if not service.publisher_authorizes_exact_bytes(SAMPLE3, accepted_report):
                raise ValueError("not authorized")
            if sha(accepted_file.read_bytes()) != ACCEPTED:
                raise RuntimeError("candidate byte identity changed")
            return accepted_file, State.native_pub_preview()

    service.STATE = State()
    server = ThreadingHTTPServer(("127.0.0.1", 0), service.Handler)
    thread = threading.Thread(target=server.serve_forever, daemon=True)
    thread.start()
    port = server.server_address[1]
    print(json.dumps({"ready": True, "port": port, "source_sha256": SAMPLE3,
                      "candidate_sha256": ACCEPTED}), flush=True)
    try:
        thread.join()
    finally:
        server.server_close()

if __name__ == "__main__":
    main()
