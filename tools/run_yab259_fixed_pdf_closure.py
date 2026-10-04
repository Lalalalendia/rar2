#!/usr/bin/env python3
"""Run repaired Yab #259 through the canonical rar2 fixed-PDF closure runner."""

from __future__ import annotations

import argparse
import hashlib
import pathlib
import subprocess
import sys
import tempfile

ROOT = pathlib.Path(__file__).resolve().parents[1]
YAB259_HEAD = "4f5d8a57abc089d1ed8eb1326cf9bed602281f7e"
EXPECTED_REPAIR_FILES = {
    "crates/pub-viewer/src/lib.rs",
    "crates/pub-layout/src/shaped_flow.rs",
    "crates/pub-cli/src/fixed_pdf.rs",
    "crates/pub-pdf/src/text.rs",
}


class Yab259ClosureError(RuntimeError):
    pass


def run_checked(
    command: list[str],
    *,
    cwd: pathlib.Path | None = None,
    label: str,
) -> str:
    completed = subprocess.run(
        command,
        cwd=cwd,
        stdin=subprocess.DEVNULL,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        check=False,
        text=True,
    )
    if completed.returncode != 0:
        detail = completed.stderr.strip()
        raise Yab259ClosureError(label + (f": {detail}" if detail else ""))
    return completed.stdout


def bind_yab_repository(path: pathlib.Path) -> pathlib.Path:
    repository = path.expanduser().resolve()
    if not (repository / "Cargo.toml").is_file():
        raise Yab259ClosureError(f"Yab repository has no Cargo.toml: {repository}")
    inside = run_checked(
        ["git", "-C", str(repository), "rev-parse", "--is-inside-work-tree"],
        label="cannot inspect Yab repository",
    ).strip()
    if inside != "true":
        raise Yab259ClosureError(f"not a Git work tree: {repository}")
    run_checked(
        ["git", "-C", str(repository), "cat-file", "-e", f"{YAB259_HEAD}^{{commit}}"],
        label=f"Yab repository does not contain required donor {YAB259_HEAD}",
    )
    return repository


def replace_once(path: pathlib.Path, old: str, new: str, *, label: str) -> None:
    text = path.read_text(encoding="utf-8")
    count = text.count(old)
    if count != 1:
        raise Yab259ClosureError(
            f"{label} anchor mismatch in {path}: expected=1 actual={count}"
        )
    path.write_text(text.replace(old, new, 1), encoding="utf-8")


def changed_files(checkout: pathlib.Path) -> set[str]:
    output = run_checked(
        ["git", "diff", "--name-only"],
        cwd=checkout,
        label="cannot enumerate bounded Yab donor changes",
    )
    return {line.strip() for line in output.splitlines() if line.strip()}


def prepare_repaired_donor(repository: pathlib.Path, checkout: pathlib.Path) -> str:
    run_checked(
        [
            "git",
            "clone",
            "--quiet",
            "--no-hardlinks",
            "--no-checkout",
            str(repository),
            str(checkout),
        ],
        label="cannot create isolated Yab donor checkout",
    )
    run_checked(
        ["git", "checkout", "--detach", "--quiet", YAB259_HEAD],
        cwd=checkout,
        label="cannot checkout exact Yab #259 donor",
    )
    actual_head = run_checked(
        ["git", "rev-parse", "HEAD"],
        cwd=checkout,
        label="cannot verify isolated Yab donor HEAD",
    ).strip()
    if actual_head != YAB259_HEAD:
        raise Yab259ClosureError(
            f"isolated Yab donor HEAD mismatch: expected={YAB259_HEAD} actual={actual_head}"
        )

    # The historical PR head has one rustfmt-only drift on the current toolchain.
    run_checked(
        ["cargo", "fmt", "--all"],
        cwd=checkout,
        label="cannot normalize bounded Yab #259 formatting drift",
    )
    formatting_changes = changed_files(checkout)
    if formatting_changes != {"crates/pub-viewer/src/lib.rs"}:
        raise Yab259ClosureError(
            "unexpected Yab #259 formatting drift: "
            + ",".join(sorted(formatting_changes))
        )

    replace_once(
        checkout / "crates/pub-layout/src/shaped_flow.rs",
        """        let evaluated = evaluate_candidate(
            &scalars,
            &full.glyphs,
            0,
            true,
            &candidate,
            &runtime.shaping,
        )""",
        """        let evaluated = evaluate_candidate(
            &scalars,
            &full.glyphs,
            full.units_per_em,
            0,
            true,
            &candidate,
            &runtime.shaping,
        )""",
        label="Yab #259 shaped-flow first compile repair",
    )
    replace_once(
        checkout / "crates/pub-layout/src/shaped_flow.rs",
        """        let evaluated = evaluate_candidate(
            &scalars,
            &full.glyphs,
            4,
            false,
            &candidate,
            &runtime.shaping,
        )""",
        """        let evaluated = evaluate_candidate(
            &scalars,
            &full.glyphs,
            full.units_per_em,
            4,
            false,
            &candidate,
            &runtime.shaping,
        )""",
        label="Yab #259 shaped-flow second compile repair",
    )

    replace_once(
        checkout / "crates/pub-cli/src/fixed_pdf.rs",
        """fn sha256_json_id(value: &Value) -> Result<String> {
    let bytes = serde_json::to_vec(value).context("serialize fixed-flow hash payload")?;
    Ok(format!("sha256:{:x}", Sha256::digest(bytes)))
}""",
        """fn sha256_json_id(value: &Value) -> Result<String> {
    let bytes = serde_json::to_vec(value).context("serialize fixed-flow hash payload")?;
    let digest = Sha256::digest(bytes);
    const HEX: &[u8; 16] = b"0123456789abcdef";

    let mut out = String::with_capacity("sha256:".len() + 64);
    out.push_str("sha256:");
    for byte in digest {
        out.push(HEX[usize::from(byte >> 4)] as char);
        out.push(HEX[usize::from(byte & 0x0f)] as char);
    }
    Ok(out)
}""",
        label="Yab #259 sha2 compile repair",
    )

    replace_once(
        checkout / "crates/pub-pdf/src/text.rs",
        """fn prepare_run_glyphs(
    run: &FixedTextRun,
) -> Result<Option<(Vec<PreparedGlyph>, BTreeMap<u16, String>)>, PdfTextPreparationError> {""",
        """type PreparedRunGlyphs = (Vec<PreparedGlyph>, BTreeMap<u16, String>);

fn prepare_run_glyphs(
    run: &FixedTextRun,
) -> Result<Option<PreparedRunGlyphs>, PdfTextPreparationError> {""",
        label="Yab #259 clippy repair",
    )

    run_checked(
        ["cargo", "fmt", "--all"],
        cwd=checkout,
        label="cannot format repaired Yab #259 donor",
    )
    run_checked(
        ["git", "diff", "--check"],
        cwd=checkout,
        label="repaired Yab #259 donor has whitespace errors",
    )
    repairs = changed_files(checkout)
    if repairs != EXPECTED_REPAIR_FILES:
        raise Yab259ClosureError(
            "unexpected Yab #259 repair file set: " + ",".join(sorted(repairs))
        )

    patch = run_checked(
        ["git", "diff", "--binary"],
        cwd=checkout,
        label="cannot fingerprint Yab #259 repair bundle",
    )
    return hashlib.sha256(patch.encode("utf-8")).hexdigest()


def engine_command(checkout: pathlib.Path) -> list[str]:
    return [
        "cargo",
        "run",
        "--quiet",
        "--manifest-path",
        str(checkout / "Cargo.toml"),
        "-p",
        "pub-cli",
        "--bin",
        "pub",
        "--",
        "convert",
        "{fixture}",
        "--to",
        "pdf",
        "--output",
        "{pdf}",
        "--fallback-font",
        "{font}",
        "--json",
    ]


def closure_command(
    checkout: pathlib.Path,
    *,
    fixture: pathlib.Path,
    pdf_output: pathlib.Path,
    receipt_output: pathlib.Path,
    fallback_font: pathlib.Path | None,
) -> list[str]:
    command = [
        sys.executable,
        str(ROOT / "tools" / "run_local_fixed_pdf_shaped_flow.py"),
        "--fixture",
        str(fixture),
        "--pdf-output",
        str(pdf_output),
        "--receipt-output",
        str(receipt_output),
    ]
    if fallback_font is not None:
        command.extend(["--fallback-font", str(fallback_font)])
    command.append("--")
    command.extend(engine_command(checkout))
    return command


def main() -> int:
    parser = argparse.ArgumentParser(
        description=(
            "Materialize an isolated exact Yab #259 donor, apply the bounded "
            "compile/lint repair set proven by the rar2 donor oracle, and run it "
            "behind tools/run_local_fixed_pdf_shaped_flow.py"
        )
    )
    parser.add_argument("--yab-checkout", required=True, type=pathlib.Path)
    parser.add_argument("--fixture", required=True, type=pathlib.Path)
    parser.add_argument("--pdf-output", required=True, type=pathlib.Path)
    parser.add_argument("--receipt-output", required=True, type=pathlib.Path)
    parser.add_argument("--fallback-font", type=pathlib.Path)
    args = parser.parse_args()

    try:
        repository = bind_yab_repository(args.yab_checkout)
        with tempfile.TemporaryDirectory(prefix="chaptera-yab259-donor-") as temporary:
            donor = pathlib.Path(temporary) / "donor"
            repair_sha256 = prepare_repaired_donor(repository, donor)
            print(
                f"prepared_yab259_repair_sha256={repair_sha256}",
                file=sys.stderr,
            )
            completed = subprocess.run(
                closure_command(
                    donor,
                    fixture=args.fixture,
                    pdf_output=args.pdf_output,
                    receipt_output=args.receipt_output,
                    fallback_font=args.fallback_font,
                ),
                cwd=ROOT,
                check=False,
            )
            return completed.returncode
    except Yab259ClosureError as error:
        print(str(error), file=sys.stderr)
        return 2


if __name__ == "__main__":
    raise SystemExit(main())
