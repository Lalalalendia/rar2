# Chaptera Reader — portable technical preview

Contract: `chaptera.reader-portable-readme.v1`

This package contains the read-only Chaptera Reader application.

## Boundary

- opens supported Publisher files through the current Viewer path;
- keeps the source PUB read-only;
- exposes Reader/view/diagnostic behavior only;
- does not enable Editor controls, Editor Project save/replay, editable export, or native Save PUB;
- does not require Cargo, a repository checkout, or a Chaptera server to launch.

This is a bounded technical preview, not a claim of full Microsoft Publisher visual or format compatibility.

<!-- chaptera-windows-support:start -->
## Windows system requirements

Public V0 target: **Windows 11 25H2 x64**.

- Consumer Windows support is not claimed until the corresponding matrix row has its physical/VM product receipts.
- The first required edition receipt is **Windows 11 25H2 Pro x64**; other editions require independent receipts.
- Fixed display-scale acceptance is **100% and 150%**. Live movement across mixed-DPI monitors is not claimed.
- Native ARM64 support is not claimed; hosted ARM64 work is compile-only preflight.
- GitHub windows-latest / Windows Server 2025 is **CI mechanics evidence only**, not consumer-Windows support evidence.
- Windows 10 22H2 is outside the default public support promise.
<!-- chaptera-windows-support:end -->
