# Octos Phase 3 Soak Runbook

This runbook is the executable delivery contract for `M3.1 Soak Closure`.

Use it with:

- `docs/OCTOS_RUNTIME_PHASE3_KICKOFF.md`
- `docs/OCTOS_RUNTIME_PHASE3_CONTRACT.md`
- `scripts/phase3-canary-snapshot.sh`

## Purpose

The goal of M3.1 is not "the canary passed once".

The goal is to gather repeated, comparable evidence that the public canary is
stable enough to support the delivered Phase 3 slice and to separate stable
regressions from test/proxy noise.

## Release Truth

Public canary truth remains:

- `https://dspfac.crew.ominix.io`

Host verification truth remains:

- canary-serving hosts behind that public endpoint

Do not substitute:

- private host URLs
- raw API ports
- `bot.ominix.io`
- `ocean.ominix.io`

## Owners

Integrator:

- owns milestone scope
- decides whether a failure is in-scope or deferred

Kierkegaard:

- runs browser suites
- records user-visible repro notes

Anscombe:

- records host health, deployed version, and log evidence

## Required Snapshot Per Soak Run

Each soak run must capture:

1. Public canary snapshot
   - `./scripts/phase3-canary-snapshot.sh`
2. Browser gate results
   - exact commands
   - exact failing test names if any
3. Host/runtime evidence
   - deployed binary/version
   - service health
   - any relevant runtime log snippets
4. Triage outcome
   - stable regression
   - likely proxy/test flake
   - operator/deploy issue
   - deferred/non-blocking issue

## Snapshot Command

Default:

```bash
./scripts/phase3-canary-snapshot.sh
```

With operator summary:

```bash
OCTOS_AUTH_TOKEN=... ./scripts/phase3-canary-snapshot.sh
```

Outputs land under:

- `artifacts/phase3-soak/<timestamp>/`

## Required Browser Gate

Run against the public canary only:

```bash
cd e2e
OCTOS_TEST_URL=https://dspfac.crew.ominix.io npx playwright test live-slides-site.spec.ts
OCTOS_TEST_URL=https://dspfac.crew.ominix.io npx playwright test live-browser.spec.ts --grep "deep research survives reload without ghost turns|research podcast delivers exactly one audio card after reload"
OCTOS_TEST_URL=https://dspfac.crew.ominix.io npx playwright test runtime-regression.spec.ts --grep "Background task lifecycle"
```

If `M3.2` is active in parallel, also record:

```bash
OCTOS_TEST_URL=https://dspfac.crew.ominix.io npx playwright test coding-hardcases.spec.ts
```

## Required Host Checks

Per soak run, record:

- deployed binary version/SHA on canary-serving hosts
- service status for the active `octos serve` unit
- recent logs covering the failing or passing window
- confirmation that the frontend asset hash served by canary matches the
  intended deploy

## Run Record Template

Each soak run should produce one record with:

- run id
- UTC timestamp
- deployed backend SHA
- deployed frontend asset hash
- snapshot directory
- browser commands
- pass/fail summary
- host evidence summary
- triage summary

## Closure Criteria For M3.1

M3.1 closes only when all are true:

- at least 3 recorded canary runs exist
- the runs are comparable and use the same public truth host
- top recurring failures are triaged into issue comments or child bugs
- no unresolved blocker remains in:
  - slides reload
  - site reload
  - deep research reload
  - research podcast reload
  - background task lifecycle

## Non-Goals

This milestone does not authorize:

- broad harness redesign
- unrelated provider/admin/infra work
- switching truth hosts mid-soak
- closing regressions without command/output evidence
