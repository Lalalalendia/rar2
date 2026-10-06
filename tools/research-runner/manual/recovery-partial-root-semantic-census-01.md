# RECOVERY-PARTIAL-ROOT-SEMANTIC-CENSUS-01

This is the local-byte breadth census for READER-PARTIAL-ROOT-CONTENTS-SALVAGE-01.

It does not use Microsoft Publisher and does not modify any PUB source.

## What it measures

For every .pub under the selected local recovery corpus:

1. attempts the new bounded direct-root regular /Contents prefix recovery;
2. keeps only Partial results;
3. binds source SHA-256, prefix SHA-256, declared length, available prefix length, exact physical ranges and typed truncation reason;
4. classifies the prefix as useful_semantic_prefix / forensic_only / no_safe_fact;
5. emits no filename, recovered bytes, text or images.

For mature 0x2C, useful_semantic_prefix requires:
- complete physical 0x2C header;
- complete trailer + directory inside the available prefix;
- at least one unambiguous directory reference;
- the referenced chunk is entirely inside the prefix;
- the chunk parses with existing confirmed grammar.

A header/preamble alone is not useful semantic recovery.
Old 0x22 remains a separate grammar path and is currently reported forensic_only.

## Run

Use the branch/head carried by PR #1555.

PowerShell:
Set-Location C:\Users\User\rar2
git fetch origin
git switch reader/partial-root-contents-salvage-01
git pull --ff-only

$Corpus = "C:\Users\User\rar2\realtest"
$Out = "C:\Users\User\rar2\out\pub-research\recovery-partial-root-semantic-census-01.json"

cargo run --locked --manifest-path vendor\producer-a\Cargo.toml -p pub-reader --bin partial-root-contents-census -- $Corpus $Out
Get-Content -LiteralPath $Out -Raw

Prefer the narrowest retained recovery root containing the historical malformed-CFB corpus rather than all unrelated realtest data when its exact path is known.

## Expected comparison

Historical recovery authority:
malformed exact hashes = 69
recovery salvageable   = 68
root Contents: complete = 21 / partial = 37 / missing = 10 / rejected = 1
37 partial rows -> 28 exact (prefix hash, declared length) contexts

The new current-main-oriented primitive is intentionally stricter than historical recovery tooling. Therefore the local census must report both:
- how many historical partial sources are admitted by the new typed prefix primitive;
- among admitted partial prefixes, how many are useful_semantic_prefix.

Do not force the new count to equal 37 or 28.
A lower count means the new primitive still lacks one or more historical physical-recovery modes.

## Promotion law

Only useful_semantic_prefix rows are candidates for future Reader OPEN_PARTIAL.
forensic_only remains Rescue/diagnostic evidence.
no_safe_fact remains fail-closed.
Even useful_semantic_prefix does not become a product Reader claim until the exact source passes the integrated Reader salvage path with explicit gaps and source immutability.