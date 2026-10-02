import json
import urllib.error
import urllib.request

ROOT = "https://msdl.microsoft.com/download/symbols"
IDENTITIES = [
    ("wwlib.pdb", "DD4E67580AC5481C9F58F09C641067EC2", "word2016_kb5002323"),
    ("mspub.pdb", "97DAC08130634944B9A0424ED2248B1B2", "publisher_15601_20088"),
    ("morph9.pdb", "1AF3B00E30134E0CA83503D6CBC7C5C12", "publisher_15601_20088"),
    ("ptxt9.pdb", "DE6DC716B94B4A0FB01D9650C09D52672", "publisher_15601_20088"),
    ("pubconv.pdb", "D06CA34A3B3548AC94C8801E201EC8E12", "publisher_15601_20088"),
]

def get(url):
    req = urllib.request.Request(url, method="GET", headers={
        "User-Agent": "chaptera-symbol-store-protocol-probe/1",
        "Range": "bytes=0-4095",
    })
    try:
        with urllib.request.urlopen(req, timeout=30) as r:
            data = r.read(4096)
            return {
                "status": int(getattr(r, "status", 200)),
                "final_url": r.geturl(),
                "content_length": r.headers.get("Content-Length"),
                "content_type": r.headers.get("Content-Type"),
                "prefix_hex": data[:64].hex(),
                "text_prefix": data[:1024].decode("utf-8", "replace"),
            }
    except urllib.error.HTTPError as exc:
        return {"status": int(exc.code), "content_length": exc.headers.get("Content-Length")}
    except Exception as exc:
        return {"status": None, "error": f"{type(exc).__name__}: {exc}"}

def main():
    root_index2 = get(f"{ROOT}/index2.txt")
    records = []
    for pdb, index, role in IDENTITIES:
        compressed = pdb[:-1] + "_"
        one_tier = f"{ROOT}/{pdb}/{index}"
        two_tier = f"{ROOT}/{pdb[:2]}/{pdb}/{index}"
        probes = {}
        for layout, base in (("one_tier", one_tier), ("two_tier", two_tier)):
            probes[layout] = {
                pdb: get(f"{base}/{pdb}"),
                compressed: get(f"{base}/{compressed}"),
                "file.ptr": get(f"{base}/file.ptr"),
                "refs.ptr": get(f"{base}/refs.ptr"),
            }
        records.append({"pdb": pdb, "index": index, "role": role, "probes": probes})

    result = {
        "schema": "symbol-store-protocol-probe.v1",
        "root": ROOT,
        "index2": root_index2,
        "records": records,
    }
    with open("out/symbol-store-protocol.json", "w", encoding="utf-8") as f:
        json.dump(result, f, indent=2)
    print(json.dumps(result, indent=2))

if __name__ == "__main__":
    main()
