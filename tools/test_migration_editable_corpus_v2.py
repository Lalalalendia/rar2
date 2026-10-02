#!/usr/bin/env python3
from __future__ import annotations

import importlib.util
from collections import Counter
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
MODULE_PATH = ROOT / "tools" / "migration_editable_corpus_v2.py"

spec = importlib.util.spec_from_file_location("migration_editable_corpus_v2", MODULE_PATH)
module = importlib.util.module_from_spec(spec)
assert spec.loader is not None
spec.loader.exec_module(module)


def main() -> int:
    core = module.build_entries(ROOT, "core65")
    extended = module.build_entries(ROOT, "extended86")

    assert len(core) == 65
    assert len(extended) == 86
    assert len({row["sha256"] for row in core}) == 65
    assert len({row["sha256"] for row in extended}) == 86
    assert {row["sha256"] for row in core}.issubset(
        {row["sha256"] for row in extended}
    )

    core_strata = Counter(row["stratum"] for row in core)
    assert core_strata["apache_poi"] == 22
    assert core_strata["lalamu_github"] == 14
    assert sum(
        count
        for stratum, count in core_strata.items()
        if stratum.startswith("microsoft_press_")
    ) == 29

    extended_strata = Counter(row["stratum"] for row in extended)
    assert extended_strata["lalamu_historical-web"] == 1
    assert extended_strata["lalamu_institutional-web"] == 20

    for row in extended:
        assert len(row["sha256"]) == 64
        assert row["byte_len"] > 0
        assert row["url"].startswith(("https://", "http://"))
        assert row["kind"] in {"direct", "zip_member"}
        if row["kind"] == "zip_member":
            assert row["member"]

    print(
        {
            "core65": len(core),
            "extended86": len(extended),
            "core_strata": dict(sorted(core_strata.items())),
        }
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
