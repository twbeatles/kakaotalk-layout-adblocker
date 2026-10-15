# Audit runtime fixes

## User scenarios
- US1 P1: Ads are hidden promptly without relaxing ad classification or touching normal KakaoTalk UI.
- US2 P1: Disabling blocking restores previous changes even after a transient failure; control requests wake the worker.
- US3 P2: A transient event-hook failure recovers automatically and its degraded state is observable.
- US4 P2: Diagnostics distinguish process discovery, event delivery, evaluation and mutation latency without logging titles.
- US5 P2: Updates retain the previous executable until the new app reports readiness; failure rolls back.

## Requirements
- Preserve core actions/golden contracts, PID/identity/title guards and two distinct weak-confirmation ticks.
- Snapshot all affected windows before hiding parents; hide/resize precede slow close requests; bound close work per tick.
- OFF only retries pending restores with bounded backoff; reset requests an immediate retry with truthful status.
- Retry failed hooks with capped backoff, no retries while disabled. Wake controls without idle busy polling.
- Apply the first event scan immediately and schedule a finite follow-up burst. Offer configurable cold-start latency/power policy.
- Expose monotonic diagnostic timings and actual per-frame elapsed times. Do not log user text.
- Preserve bounded shutdown; disclose nonpersistent UI recovery limits. Never restore unverified stale HWNDs across processes.
- Correct documentation promises and keep manual platform/visual measurements explicitly NOT RUN.

## Success criteria
- Existing contract/oracle/golden suite stays green; new deterministic failure/recovery tests pass.
- A slow close never precedes the first hide; OFF recovery and same-PID hook retry require no user restart.
- Update launch failure/readiness failure preserves the previous binary.
- fmt/clippy/workspace tests pass; no dependencies installed and no live user data changed.

## Assumptions
- Windows-only layout control remains the product boundary. Zero-frame visibility cannot be guaranteed.
- Real screen latency, Windows 10/ARM64/DPI matrix, and soak require a dedicated manual session.
