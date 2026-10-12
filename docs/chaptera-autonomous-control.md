# Chaptera Work / Actions control preparation

The enabled hourly Work Scheduled Task is the current executor. It reads the
existing [autonomous queue](https://app.notion.com/p/3f632a84beec812cb90bd683acfd99eb),
Notion task cards, GitHub issues/PRs and `AGENTS.md` on each run. Notion and GitHub
remain authoritative; this module is neither a task database nor an AI driver.

Existing fast loops, `required_merge_gate.py`, latest-head cancellation and
`obsolete_actions_recovery.py` retain their authority. The document Editor
agent-control catalog does not implement an engineering inference executor.
No duplicate CI gate, health classifier, task database or merge bypass is added.

`tools/ci/chaptera_autonomous_control.py` prepares deterministic controls:

- require fresh authenticated snapshots and all eight evidenced admission checks;
- require complete research planning fields without manufacturing epochs/claims;
- resume one current owner and reject moved candidate heads;
- advance closure only with actual merge/exact-head CI and verified Notion update;
- reserve the complete verified API charge bound against run/day/month limits,
  idempotently before dispatch and conservatively after a crash; an attempt with
  unknown charge cannot dispatch again without a new budget reservation;
- save checksummed checkpoints atomically and refuse corrupt/missing cost state.

Snapshots are normalized by a verified authenticated bridge. A boolean or this
module's green unit tests do not prove a real task, permission, price or credential.
Planning field names/types must follow the current canonical Notion protocol.

The scoped Actions workflow tests these controls. Its hourly schedule is disabled
unless the repository activation variable is explicitly enabled; no activation
variable or secret is created by this change. Manual/scheduled preflight always
records `blocked` and `ai_executed=false`. Secret values never enter artifacts.

Full persistent AI execution remains unimplemented/unactivated until an available
authenticated Notion/inference bridge, verified model charge bound and write scopes
are present. Before any future API call, publish and verify the conservative cost
reservation in durable GitHub state; an end-of-job artifact alone cannot protect
against runner loss. Persist `begin_api_attempt` before dispatch too. The charge
bound must cover all permitted model/tool calls and retries; no numeric model
price is assumed here. Claim through the existing task/branch ownership protocol,
compose the existing required merge gate, and prove a complete task cycle before
allowing the Actions executor to run concurrently with the Work scheduler.

Test locally: `python -m unittest discover -s tools/ci -p test_chaptera_autonomous_control.py`.
