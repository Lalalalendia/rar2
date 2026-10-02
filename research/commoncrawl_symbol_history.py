import json
import urllib.parse
import urllib.request
from pathlib import Path

INDEXES = [
    "CC-MAIN-2022-33",
    "CC-MAIN-2022-40",
    "CC-MAIN-2022-49",
    "CC-MAIN-2023-06",
    "CC-MAIN-2023-14",
    "CC-MAIN-2023-23",
    "CC-MAIN-2023-40",
    "CC-MAIN-2023-50",
    "CC-MAIN-2024-10",
    "CC-MAIN-2024-30",
    "CC-MAIN-2024-51",
    "CC-MAIN-2025-05",
    "CC-MAIN-2025-30",
    "CC-MAIN-2025-51",
    "CC-MAIN-2026-04",
    "CC-MAIN-2026-21",
    "CC-MAIN-2026-39",
]

TARGETS = {
    "wwlib": {
        "prefix": "https://msdl.microsoft.com/download/symbols/wwlib.pdb/",
        "exact": "https://msdl.microsoft.com/download/symbols/wwlib.pdb/DD4E67580AC5481C9F58F09C641067EC2/wwlib.pdb",
    },
    "mspub": {
        "prefix": "https://msdl.microsoft.com/download/symbols/mspub.pdb/",
        "exact": "https://msdl.microsoft.com/download/symbols/mspub.pdb/97DAC08130634944B9A0424ED2248B1B2/mspub.pdb",
    },
    "morph9": {
        "prefix": "https://msdl.microsoft.com/download/symbols/morph9.pdb/",
        "exact": "https://msdl.microsoft.com/download/symbols/morph9.pdb/1AF3B00E30134E0CA83503D6CBC7C5C12/morph9.pdb",
    },
    "ptxt9": {
        "prefix": "https://msdl.microsoft.com/download/symbols/ptxt9.pdb/",
        "exact": "https://msdl.microsoft.com/download/symbols/ptxt9.pdb/DE6DC716B94B4A0FB01D9650C09D52672/ptxt9.pdb",
    },
    "pubconv": {
        "prefix": "https://msdl.microsoft.com/download/symbols/pubconv.pdb/",
        "exact": "https://msdl.microsoft.com/download/symbols/pubconv.pdb/D06CA34A3B3548AC94C8801E201EC8E12/pubconv.pdb",
    },
}

def query(index, url, match_type=None):
    args = {"url": url, "output": "json"}
    if match_type:
        args["matchType"] = match_type
    endpoint = "https://index.commoncrawl.org/" + index + "-index?" + urllib.parse.urlencode(args)
    req = urllib.request.Request(endpoint, headers={"User-Agent": "chaptera-symbol-archive-probe/1"})
    try:
        with urllib.request.urlopen(req, timeout=10) as r:
            body = r.read().decode("utf-8", "replace")
            rows = []
            for line in body.splitlines():
                line = line.strip()
                if not line:
                    continue
                try:
                    rows.append(json.loads(line))
                except Exception:
                    rows.append({"raw": line})
            return {"status": int(getattr(r, "status", 200)), "endpoint": endpoint, "rows": rows}
    except urllib.error.HTTPError as exc:
        body = exc.read().decode("utf-8", "replace") if hasattr(exc, "read") else ""
        return {"status": int(exc.code), "endpoint": endpoint, "body": body[:1000], "rows": []}
    except Exception as exc:
        return {"status": None, "endpoint": endpoint, "error": f"{type(exc).__name__}: {exc}", "rows": []}

def main():
    result = {"schema": "commoncrawl-symbol-history.v1", "indexes": INDEXES, "targets": {}}
    for name, target in TARGETS.items():
        exact_hits = []
        prefix_hits = []
        for index in INDEXES:
            ex = query(index, target["exact"])
            if ex["rows"]:
                exact_hits.append({"index": index, **ex})
            # Prefix query once per index. Limit retained rows to avoid an unbounded artifact.
            pr = query(index, target["prefix"], "prefix")
            if pr["rows"]:
                pr["rows"] = pr["rows"][:100]
                prefix_hits.append({"index": index, **pr})
        result["targets"][name] = {
            "exact_url": target["exact"],
            "prefix": target["prefix"],
            "exact_hit_indexes": len(exact_hits),
            "prefix_hit_indexes": len(prefix_hits),
            "exact_hits": exact_hits,
            "prefix_hits": prefix_hits,
        }
    Path("out").mkdir(exist_ok=True)
    Path("out/commoncrawl-symbol-history.json").write_text(json.dumps(result, indent=2), encoding="utf-8")
    print(json.dumps(result, indent=2))

if __name__ == "__main__":
    main()
