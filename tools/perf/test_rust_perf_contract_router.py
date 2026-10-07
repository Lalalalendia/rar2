#!/usr/bin/env python3
from __future__ import annotations

import json
import pathlib
import tempfile
import unittest

import rust_perf_contract_router as router


class RustPerfContractRouterTests(unittest.TestCase):
    def setUp(self) -> None:
        self.contracts = router.load_registry(router.DEFAULT_REGISTRY)

    def selected_ids(self, paths: list[str]) -> list[str]:
        return [c.contract_id for c in router.select_contracts(paths, self.contracts)]

    def test_pub_layout_source_change_selects_work_contract(self) -> None:
        self.assertEqual(
            self.selected_ids(["vendor/producer-a/crates/pub-layout/src/lib.rs"]),
            ["layout-projection-work"],
        )

    def test_benchmark_definition_change_selects_work_contract(self) -> None:
        self.assertEqual(
            self.selected_ids(
                ["vendor/producer-a/crates/pub-layout/tests/projection_index_benchmark.rs"]
            ),
            ["layout-projection-work"],
        )

    def test_unrelated_rust_change_does_not_select_layout_contract(self) -> None:
        self.assertEqual(
            self.selected_ids(["apps/chaptera-server/src/main.rs"]),
            [],
        )

    def test_directory_glob_is_bounded_to_owned_subtree(self) -> None:
        self.assertFalse(
            router.path_matches(
                "vendor/producer-a/crates/pub-layout-extra/src/lib.rs",
                "vendor/producer-a/crates/pub-layout/src/**",
            )
        )

    def test_unknown_command_id_fails_closed(self) -> None:
        payload = {
            "version": 1,
            "contracts": {
                "bad": {
                    "description": "must fail",
                    "paths": ["**/*.rs"],
                    "command_id": "shell-from-registry"
                }
            }
        }
        with tempfile.TemporaryDirectory() as tmp:
            path = pathlib.Path(tmp) / "contracts.json"
            path.write_text(json.dumps(payload), encoding="utf-8")
            with self.assertRaisesRegex(ValueError, "unknown trusted command_id"):
                router.load_registry(path)


if __name__ == "__main__":
    unittest.main()
