"""Fail-closed source-safe guard for the Publisher native receipt log label.

Hosted-only: no native Publisher, no private fixture and no PowerShell process.
"""
from pathlib import Path
import re

WORKFLOW = Path(__file__).resolve().parents[2] / ".github/workflows/pub-re-native.yml"
EXPECTED = {
    "cfb-receipt.json",
    "control-native-receipt.json",
    "control-native-stage-receipt.json",
    "mutation-native-receipt.json",
    "mutation-native-stage-receipt.json",
    "native-receipt.json",
    "native-stage-receipt.json",
    "officeart-receipt.json",
    "rotation-pair-blast-radius.json",
    "rotation-pair-cfb-receipt.json",
    "rotation-pair-officeart-receipt.json",
    "rotation-pair-summary.json",
}


def main() -> None:
    text = WORKFLOW.read_text(encoding="utf-8")
    start = text.index("      - name: Enforce source-safe evidence boundary")
    end = text.index("      - name: Upload source-safe receipts only", start)
    guard = text[start:end]

    match = re.search(
        r"\$trustedReceiptNames\s*=\s*@\((.*?)\)", guard, re.DOTALL
    )
    assert match is not None, "literal receipt-name allowlist missing"
    names = re.findall(r"^\s*'([^']+)'[,]?\s*$", match.group(1), re.MULTILINE)
    assert len(names) == len(EXPECTED), "unexpected receipt-name count"
    assert len(names) == len(set(names)), "duplicate allowlist name"
    assert set(names) == EXPECTED, "receipt-name allowlist widened or narrowed"

    # Static proof that the actual PowerShell guard consults this exact list,
    # rather than merely validating an arbitrary ASCII basename.
    assert "$trustedReceiptNames -ccontains $file.Name" in guard
    assert 'else { "untrusted-name" }' in guard
    assert 'if ($file.Name -match' not in guard
    assert guard.count('$file.Name') == 2
    assert 'source-safe evidence contains an absolute filesystem path in receipt: $receiptName' in guard

    # Old unsafe regex admitted both names; the checked-in list does not.
    for untrusted in (
        "client-PRIVATE-CASE.json",
        "customer-Jane-PrivateCase.json",
        "unlisted-ascii.json",
    ):
        assert re.fullmatch(r"[A-Za-z0-9_.-]{1,120}", untrusted)
        assert untrusted not in names
    assert "rotation-pair-blast-radius.json" in names
    assert "mutation-native-receipt.json" in names

    # Existing hard rejection and no-upload placement must remain unchanged.
    assert "if ($files.Count -lt 1)" in guard
    assert 'if ($text -match' in guard
    assert 'raw_document_bytes_emitted' in guard
    assert 'raw_stream_bytes_emitted' in guard
    assert 'ConvertFrom-Json' in guard
    assert "throw" in guard
    print("PASS native receipt-name privacy guard (finite labels, hostile ASCII controls)")


if __name__ == "__main__":
    main()
