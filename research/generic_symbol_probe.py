import json
import sys
from pathlib import Path

from c2r_pub_symbol_probe import codeview_records, probe_symbol, sha256


def main():
    if len(sys.argv) < 3:
        raise SystemExit("usage: generic_symbol_probe.py OUTPUT_JSON ROLE=PATH [ROLE=PATH ...]")

    output = Path(sys.argv[1])
    records = []

    for spec in sys.argv[2:]:
        if "=" not in spec:
            raise SystemExit(f"invalid target spec: {spec}")
        role, raw_path = spec.split("=", 1)
        path = Path(raw_path)
        base = {
            "module": path.name,
            "role": role,
            "path": str(path),
        }
        if not path.exists():
            records.append({**base, "symbol_result": "MODULE_NOT_FOUND"})
            continue

        base.update({
            "sha256": sha256(path),
            "size": path.stat().st_size,
        })
        cvs = codeview_records(path)
        if not cvs:
            records.append({**base, "codeview": "NO_CODEVIEW", "symbol_result": "NO_CODEVIEW"})
            continue

        for cv in cvs:
            index, verdict, hit, probes = probe_symbol(cv)
            records.append({
                **base,
                **cv,
                "codeview": cv["kind"],
                "symbol_index": index,
                "symbol_result": verdict,
                "symbol_hit": hit,
                "probes": probes,
            })

    summary = {
        "schema": "microsoft-public-symbol-calibration.v1",
        "symbol_present": sum(r.get("symbol_result") == "SYMBOL_PRESENT" for r in records),
        "symbol_missing": sum(r.get("symbol_result") == "CODEVIEW_PRESENT_SYMBOL_NOT_FOUND" for r in records),
        "records": records,
    }
    output.parent.mkdir(parents=True, exist_ok=True)
    output.write_text(json.dumps(summary, indent=2), encoding="utf-8")
    print(json.dumps(summary, indent=2))


if __name__ == "__main__":
    main()
