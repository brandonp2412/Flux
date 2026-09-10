# Flux Progress

This file is the persistent overall-progress ledger used by every Flux development session.

## Current baseline

- Baseline date: 2026-09-10
- Baseline commit: `7d219b1`
- Baseline roadmap units: 739 checkbox items
- Completed baseline units: 374
- Overall progress: **51%**
- Scope growth after baseline: 0 items recorded

## Reporting rule

1. Read this file together with `ROADMAP.MD` before reporting an overall percentage.
2. Do not recalculate the percentage from the current roadmap checkbox count. The baseline denominator is fixed at 739 so newly discovered work cannot make progress appear to go backwards.
3. Increase `Completed baseline units` only when work corresponding to a baseline roadmap item is verified and committed to `main`.
4. Newly discovered roadmap items are scope growth. Record them separately; they do not change the 739-unit denominator.
5. Never decrease the reported overall percentage. If previously completed work needs repair or reopening, record it as follow-up/scope growth rather than rewriting history.
6. Report the rounded value of `Completed baseline units / 739 * 100`.
7. Update this ledger in the same commit as the roadmap change whenever practical.

This metric is intentionally a stable project-management indicator, not a claim that every roadmap checkbox represents equal engineering effort. Milestone-specific detail should come from `ROADMAP.MD`.
