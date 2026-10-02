import json
import sys
import urllib.parse
import urllib.request

URLS = [
    "https://msdl.microsoft.com/download/symbols/wwlib.pdb/DD4E67580AC5481C9F58F09C641067EC2/wwlib.pdb",
    "http://msdl.microsoft.com/download/symbols/wwlib.pdb/DD4E67580AC5481C9F58F09C641067EC2/wwlib.pdb",
    "https://msdl.microsoft.com/download/symbols/wwlib.pdb/DD4E67580AC5481C9F58F09C641067EC2/wwlib.pd_",
    "http://msdl.microsoft.com/download/symbols/wwlib.pdb/DD4E67580AC5481C9F58F09C641067EC2/wwlib.pd_",
]

def cdx(url):
    q = urllib.parse.urlencode({
        "url": url,
        "output": "json",
        "fl": "timestamp,original,statuscode,mimetype,digest",
        "filter": "statuscode:200",
        "collapse": "digest",
    })
    endpoint = "https://web.archive.org/cdx/search/cdx?" + q
    req = urllib.request.Request(endpoint, headers={"User-Agent":"chaptera-symbol-archive-probe/1"})
    try:
        with urllib.request.urlopen(req, timeout=45) as r:
            body = r.read().decode("utf-8", "replace")
            return {"endpoint": endpoint, "status": r.status, "body": body}
    except Exception as exc:
        return {"endpoint": endpoint, "status": None, "error": f"{type(exc).__name__}: {exc}"}

def main():
    out = []
    for url in URLS:
        rec = {"url": url, "cdx": cdx(url)}
        body = rec["cdx"].get("body", "")
        try:
            rows = json.loads(body) if body else []
        except Exception:
            rows = []
        rec["capture_count"] = max(0, len(rows)-1) if isinstance(rows, list) else 0
        rec["captures"] = rows[1:] if isinstance(rows, list) and len(rows) > 1 else []
        out.append(rec)
    result = {
        "schema":"wwlib-kb5002323-wayback.v1",
        "pdb":"wwlib.pdb",
        "symbol_index":"DD4E67580AC5481C9F58F09C641067EC2",
        "results":out,
        "total_captures":sum(x["capture_count"] for x in out),
    }
    path = sys.argv[1] if len(sys.argv) > 1 else "wayback.json"
    with open(path,"w",encoding="utf-8") as f:
        json.dump(result,f,indent=2)
    print(json.dumps(result,indent=2))

if __name__ == "__main__":
    main()
