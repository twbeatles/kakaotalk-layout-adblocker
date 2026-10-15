# Implementation plan

Use the existing Rust workspace and standard library, with no new dependencies. Stay on main.
1. Add failure/order regression tests and small worker runtime seams (process/hook/wait), keeping engine caches worker-owned.
2. Pre-capture hide/zero-size snapshots, suppress windows before bounded close processing; revalidate control/identity before every mutation.
3. Extract OFF recovery and hook retry scheduling into tested state helpers. Provide interruptible native/message waits.
4. Immediate event reconciliation plus finite follow-up scans. Add an explicit low-latency setting and DEBUG timing/frame timestamps.
5. Add update readiness handshake/rollback tests, correct docs and audit action status, execute all local checks.

Avoid persistent HWND restore journals: process reuse makes blind recovery unsafe. Explain KakaoTalk restart recovery instead.
Manual visual/platform evidence remains a separate validation gate, not a claim of automated success.

Measured decision: live Toolhelp scan averaged 18.0ms. Keep low_latency_mode opt-in/default false to preserve the low-CPU purpose; at 200ms fixed polling the scan alone could cost approximately 9% of one core. Event-path latency fixes remain enabled in both modes.
