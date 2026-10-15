# CI-LOCAL-DEV-BOOTSTRAP-01 — isolated Windows runner onboarding

Task: https://app.notion.com/p/3f932a84beec81d3a870e3ee790cf5b3

This is a bootstrap wrapper for the existing trusted-main Windows transport in
[PR #2757](https://github.com/Lalalalendia/rar2/pull/2757), not a new CI
engine or an AI inference agent. Outbound local feature handoff remains owned by
[PR #2759](https://github.com/Lalalalendia/rar2/pull/2759).

## Host separation — mandatory

Install as a **separate standard Windows user**, never as administrator, the
Publisher oracle account, or an account containing private PUB documents,
licensed tools, customer files, GitHub admin sessions, or deployment credentials.
A second user on the *same Windows kernel is not a strong sandbox*; use a
separately isolated host/VM if trust or confidentiality demands it. GitHub warns
against letting untrusted PR code run on persistent self-hosted runners; this
repository's workflow is main-only and PRs are gated on GitHub-hosted machines.

On a Windows 10 laptop, OS support/security updates may be expired. Prefer a
supported Windows version before leaving a runner running unattended.

## Owner-owned initial preparation

1. Sign in to a separate **non-admin** Windows user, intended solely for
   Chaptera's trusted-main runner. Do not authenticate gh or a GitHub PAT here.
2. Install Git, Python 3.12+, Rust/Cargo stable, Node, and PowerShell 7 *for
   this user*. They are required by the existing fast loop; installation is not
   performed silently by the bootstrap helper.
3. Check out the public main of this repository in that user account.
4. **Preflight without side effects** from inside the clone:

   py -3 tools/ci/bootstrap_local_dev_runner.py

   The JSON report lists missing tools, incorrect OS/architecture, admin
   privileges, pre-existing install and readiness. Do not proceed until ready.

5. As the GitHub repository owner, open
   https://github.com/Lalalalendia/rar2/settings/actions/runners, choose
   **New self-hosted runner**, select Windows x64 and copy only the short-lived
   **registration token** from the official setup commands. **Do not paste
   it into ChatGPT or commit it.**
6. Still inside the non-admin user clone, run:

   py -3 tools/ci/bootstrap_local_dev_runner.py --install --confirm-separate-user --autostart

   The script downloads the latest official actions/runner Windows x64
   release, verifies its published SHA-256, rejects ZIP traversal/symlinks,
   prompts for the token invisibly, configures just the distinct
   chaptera-dev-unprivileged label and starts run.cmd.

   --autostart registers an **HKCU user-logon** startup only. It does not
   install an admin Windows service and **requires that standard user to be
   logged in**. Without --autostart, the installed runner starts now but
   must be launched manually on subsequent sessions.

7. Return to GitHub Actions → Runners and verify this new runner is **Online**
   and carries [self-hosted, Windows, X64, chaptera-dev-unprivileged].
   Only then set repository Actions variable:
   CHAPTERA_LOCAL_DEV_RUNNER_ENABLED=true.
8. Run the CI-LOCAL-DEV-TRANSPORT-01 Windows trusted main fast feedback
   workflow manually from main. Verify the local-dev job completed on
   your new runner and inspect the chaptera-local-dev-<run_id> JSON artifact.
   A passing Ubuntu contract alone is not evidence that the laptop ran.

## Boundaries, cleanup and known limitations

- The bootstrap never calls gh, uploads documents, creates a PAT, changes
  the GitHub repository variable, or touches an existing Publisher runner.
  Its only non-GET remote interaction is the short-lived registration action
  performed by the official GitHub config.cmd on your host.
- GitHub returns a short-lived registration token; this helper asks for it
  via a hidden prompt. The official config.cmd necessarily receives the
  token as a process argument while registering; the bootstrap discards
  its captured stdout/stderr and never includes it in an artifact.
- A missing/invalid SHA-256 digest from the official release metadata
  **blocks installation** rather than downloading unchecked executables.
  An existing install also blocks overwrites. The installer makes no
  attempt to repair or silently replace a previous runner.
- If registration fails after extracting verified binaries, the install
  root may remain partially configured. Review the state locally; do
  not blindly retry with a new token until cleaning up that incomplete
  directory and unregistering any GitHub-side partial runner.
- The repository variable is deliberately off by default. Runner
  code being merged to GitHub is not proof of operational activation.
- For disabling, set CHAPTERA_LOCAL_DEV_RUNNER_ENABLED=false and stop
  the run.cmd console. Remove Chaptera Dev Runner from HKCU Run if
  startup was enabled, and remove the registered runner from GitHub.
- This runner **does not independently edit files or generate code**.
  Actual local AI agents use separate developer accounts/worktrees and
  opt in to tools/ci/local_dev_handoff.py when a task is assigned.
- This is a code-side onboarding path; a real Windows registration,
  authenticated first run and returned JSON artifact are required before
  marking the implementation task DONE.

## Offline validation

python -m unittest discover -s tools/ci -p test_bootstrap_local_dev_runner.py

CI executes the same suite under the existing hosted fast-loop self-test;
CI never installs or registers an Actions runner.
