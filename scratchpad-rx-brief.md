# Revisiting reactive/streaming architecture for kanata's core — a 2026 follow-up to Discussion #1323

> Context: In [Discussion #1323](https://github.com/jtroo/kanata/discussions/1323) (Nov 2024), nightscape proposed an "imperative shell, functional core" rewrite using push-based reactive streams (rxRust / pushgen). jtroo declined, on two grounds: (1) the core is a state machine driven by key events + 1ms ticks, not a natural stream pipeline; and (2) Rx-style libraries sit on async runtimes whose overhead is unacceptable in a single-threaded hot path. This brief re-scores those objections against the current Rust ecosystem and proposes what (if anything) is worth doing.

## TL;DR

- **jtroo's objection #2 was correct and is *still* correct for rxRust specifically.** rxRust's single-threaded `Local` path is not runtime-free — it still needs a local executor (`futures::executor::LocalPool` or tokio `LocalSet`) to drive scheduled work. So "rxRust sits on an async framework" was an accurate read, not a misconception, and nothing in 2025–26 changed that.
- **But the async-runtime tax was never the real question.** You don't need *any* runtime for synchronous reactive composition. The honest reframing isn't "is there a better async runtime now" — it's "is there a zero-cost *synchronous* dataflow abstraction that fits a tick-driven state machine." There is, and there always sort of was (iterators); the new-ish entrants are `pushgen` (push-based, `no_std`) and the sans-io pattern.
- **jtroo's objection #1 is the load-bearing one, and it survives.** The hard cases — `tap-hold-release`, `tap-hold-opposite-hand`, zippychord — are not "transform a stream of events." They are "defer a decision across an unknown number of future ticks until either a timeout fires *or* a specific future input arrives." That's an inherent property of the *problem*, not of the old runtimes. No 2026 library makes it disappear.
- **Recommendation:** Don't pursue a wholesale reactive rewrite of the core — the evidence in the codebase backs jtroo. There *is* a smaller, real win available: the **sans-io framing** (which kanata already half-implements) and pushgen-style synchronous combinators for the genuinely pipeline-shaped edges (input decode, output emission, sequences/chords matching). Pitch that, not Rx.

---

## 1. What kanata's hot path actually is (grounded in the code)

This matters because the proposal has to survive contact with the real loop, not an idealized one.

- **Single-threaded state machine + input thread over a bounded channel.** Core loop is `start_processing_loop()` (`src/kanata/mod.rs:2209`); OS input runs on a separate thread feeding a `std::sync::mpsc::sync_channel(100)`. No async anywhere in the tree.
- **The 1ms tick is the heartbeat.** `handle_time_ticks` → `tick_ms` (`src/kanata/mod.rs:947`/`:978`) loops `tick_states()` once per elapsed millisecond, tracking a nanosecond remainder for precision. Every timing decision is "how many ticks since press."
- **keyberon is the inner state machine.** `Layout::tick() -> CustomEvent` (`keyberon/src/layout.rs:1468`) and `Layout::event(Event)` (`:1815`). All hot structures are **heapless / fixed-capacity**: `states: Vec<State, 64>`, `queue: ArrayDeque<Queued, 32>`, `extra_waiting: ArrayDeque<WaitingState, 8>`. kanata's own `cur_keys`/`prev_keys` Vecs are cleared-and-reused, not reallocated. **The hot path is allocation-free today.** Any abstraction that introduces per-event heap traffic is a regression, full stop.
- **The edges are already pipeline-shaped.** Input decode (scancode → keycode), sequences (trie accumulator + timeout, `src/kanata/sequences.rs`), chords v2 (`keyberon/src/chord.rs`), and output emission look like `input → transform → output`. Zippychord (`src/kanata/output_logic/zippychord.rs`) is the opposite — ~15 interacting timing/gate fields, deeply procedural.

So the system is genuinely two-natured: a **stateful timing core** wrapped by **stream-shaped edges**. That split is the whole story.

## 2. Scoring jtroo's two objections against 2026

| jtroo's claim (Nov 2024) | Verdict in 2026 | Why |
|---|---|---|
| "rxRust likely sits on an async framework with too much overhead." | **Still true.** | rxRust advertises a single-threaded `Local` context, but its scheduled operators still require a local executor (`futures` `LocalPool` / tokio `LocalSet`). It's "WASM & Tokio ready," not runtime-free. For a 1ms hot path that's the wrong dependency. |
| "The core is a state machine of key-events + 1ms ticks; not a natural stream pipeline; tap-hold-release can't be expressed cleanly." | **Still true, and this is the real blocker.** | The defer-until-timeout-or-future-input pattern is intrinsic. See §4. |

The thing that *has* changed since 2024 is the surrounding ecosystem narrative: "async everywhere" has visibly cooled (RustConf 2025 talks, corrode's "State of Async Rust," WyeWorks' "when to avoid async"), and **sans-io** has become the blessed pattern for exactly this shape of problem — protocol/state logic decoupled from I/O, no runtime baked in. That's the door that opened, not a faster Rx.

## 3. The option landscape

Three categories, only one of which is a genuine fit.

### A. Rx / observable libraries — ❌ don't
- **rxRust** — single-threaded `Local` exists but needs a local executor; async dependency jtroo objected to is real. Heap-allocates subscriptions. Wrong tool for the hot path.
- **Fluxion 0.7** — explicitly a multi-*runtime* stream processor (Tokio/async-std/smol/threads/WASM). Even further from runtime-free. For throughput pipelines, not 1ms keyboard ticks.
- Verdict: these confirm jtroo's instinct. Re-proposing them would re-lose the same argument.

### B. Synchronous push/pull combinators — ✅ for the *edges*
- **`pushgen`** — `no_std`, push-based generators with `map`/`filter`/`for_each` that compile down like iterators; zero runtime, no executor, no heap. This is the library you actually gestured at in #1323, and it's the one that defeats objection #2 — because it has no async to object to. Good fit for input-decode and output-emission chains, and arguably for sequence/chord matching.
- **Plain `Iterator` combinators** — the boring baseline. For pure transforms, std iterators already give you the "functional core" composability with zero new deps. Worth saying out loud: a lot of the "imperative shell / functional core" benefit is reachable without *any* library, just by isolating the pure transforms behind iterator/closure boundaries.
- Verdict: real, modest, low-risk wins — but only on the parts that were never the problem.

### C. sans-io state-machine framing — ✅ the strategically interesting one
- **The pattern** (Firezone's writeups; `docs.rs/sansio`): the state machine *expresses* state and consumes events + time, but performs no I/O itself; an outer loop owns sockets/timers and feeds it. **kanata already does a weak version of this** — keyberon is sans-io-ish (events in, `CustomEvent` out, time via `tick()`), and the OS I/O lives in separate threads. The opportunity is to make that boundary *explicit and total*, which buys the testability/composability nightscape actually wanted.
- **`statig`** — `no_std`, ROM-resident, zero-heap hierarchical state machines; events read from a queue and submitted via `handle()`. Architecturally the closest off-the-shelf match to keyberon's needs. Realistically a *reference*, not a drop-in: keyberon's layer/queue/waiting-state machinery is too domain-specific to hand to a generic HSM lib without losing the fast-paths (e.g. the `prev_queue_len` early-exit in `handle_hold_tap`, `layout.rs:556`).
- Verdict: the right *vocabulary* for a future refactor. Adopt the discipline; probably don't adopt the dependency.

## 4. The litmus test: can any of these express `tap-hold-release`?

This is where every streaming abstraction breaks, and it's worth making concrete because it's the crux of jtroo's #1.

`WaitingState` (`keyberon/src/layout.rs:482`) holds `timeout`, `ticks`, the candidate `hold`/`tap` actions, and a `config`. Each tick, `handle_hold_tap` (`:555`) must decide **Hold / Tap / keep-waiting** based on a combination that no `map`/`filter`/`scan` expresses cleanly:

1. **elapsed time** (countdown `timeout`), AND
2. **inspection of a buffer of *future* events** that may or may not have arrived (`HoldOnOtherKeyPress`, `PermissiveHold`, `Order{buffer}`), AND
3. **out-of-order emission** — the decision can fire from the timeout branch *before* the triggering release ever arrives.

`tap-hold-opposite-hand` (`parser/src/cfg/custom_tap_hold.rs:237`) adds a coordinate→hand lookup over those future presses. This is a **defer-and-join over an unbounded future window**, not a transform. A stream operator that did this would essentially *be* `WaitingState` wearing a combinator costume — you'd reimplement the state machine inside the operator and gain nothing.

The one abstraction that genuinely models "suspend until timeout-or-input" is `async`/`await` (a hold-tap is literally `select! { _ = timeout => Hold, ev = other_key => decide(ev) }`). But that reintroduces exactly the runtime/overhead/allocation cost jtroo is right to refuse on a 1ms path. So the await-shaped problem and the no-runtime constraint are in direct tension, and the current code resolves it the correct way: a hand-rolled, heapless, tick-polled state machine. That *is* the polled-future-as-state-machine pattern, just written by hand for zero cost.

## 5. Recommendation — what to actually propose to jtroo

Don't reopen "let's make the core reactive." The codebase evidence backs his refusal. Instead, pitch the narrower, defensible thing:

1. **Adopt the sans-io framing explicitly** as a documented architectural boundary: "core = pure `(events, ticks) → output_events`, no I/O; shell = threads owning OS input/output." This is ~80% of the testability/composability win nightscape wanted, costs no dependency, and changes no hot-path performance. It's also a precondition for better fuzz/property testing of tap-hold corner cases.
2. **Use `pushgen` or plain iterator combinators on the edges only** — input decode and output emission, possibly sequence/chord matching — where the data genuinely is a stream and there's no defer-until-future-input. Measure against the current code; only land if allocation-free and not slower.
3. **Leave the timing core (keyberon waiting-states, zippychord) as a hand-rolled polled state machine.** It is already the optimal shape for the constraint. Borrow `statig`'s *vocabulary* (hierarchical states, explicit `handle`) for readability if refactoring, not its runtime.
4. **Explicitly concede the async/Rx path.** Leading with "you were right about rxRust's runtime overhead, here's the runtime-free framing instead" is far more likely to land than relitigating 2024.

The honest one-liner: *the Rust ecosystem got better runtimes since 2024, but kanata's core never needed a runtime — it needed a clean sans-io boundary, and that's available today without giving up the single-threaded, allocation-free hot path.*

---

### Sources
- [kanata Discussion #1323](https://github.com/jtroo/kanata/discussions/1323)
- [rxRust](https://github.com/rxRust/rxRust) · [rxrust scheduler docs](https://docs.rs/rxrust/latest/rxrust/scheduler/)
- [Fluxion 0.7 announcement](https://users.rust-lang.org/t/i-just-released-fluxion-0-7-0-a-reactive-stream-processing-library-for-rust-with-something-i-havent-seen-elsewhere-true-multi-runtime-support-out-of-the-box/137371)
- [pushgen docs](https://docs.rs/pushgen/latest/pushgen/) · [pushgen crate](https://crates.io/crates/pushgen)
- [sans-IO pattern (Firezone)](https://www.firezone.dev/blog/sans-io) · [sansio crate](https://docs.rs/sansio) · [HN discussion](https://news.ycombinator.com/item?id=40872020)
- [statig](https://github.com/mdeloof/statig) · [statig docs](https://docs.rs/statig)
- [The State of Async Rust (corrode)](https://corrode.dev/blog/async/) · [When to avoid async (WyeWorks)](https://www.wyeworks.com/blog/2025/02/25/async-rust-when-to-use-it-when-to-avoid-it/)
- [Rust async functions as polled state machines (Jeff McBride, 2025)](https://jeffmcbride.net/blog/2025/05/16/rust-async-functions-as-state-machines/)
