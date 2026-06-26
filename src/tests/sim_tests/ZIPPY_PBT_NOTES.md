# Zippychord property-based testing — coverage & triage

Zippychord is a state machine, so the main property test is a
`proptest-state-machine` test in `kanata_proptest.rs`. The reference
state machine (`KanataRef`) generates the config + chord dictionary AND the
transition stream; the SUT is a real `Kanata`. After every transition the SUT's
reconstructed net visible text is asserted equal to the reference's predicted
`visible`.

The oracle is **split**: a `ChordExpansion` transition carries which chord it
activates (so the expansion text is known by construction); the reference state
only does *coarse placement* (fresh append / followup replace / disabled
passthrough) from engine state. The reference does NOT reimplement the
keystroke-level eager/overlap/backspace accounting — that is the code under test.

Philosophy: generate as broadly as possible; narrow only for invalid input
(`prop_filter`/`preconditions`) or for a dimension not yet modeled by the oracle
(each such narrowing is listed below and is meant to be lifted, not permanent).

## Tests

- `kanata_proptest::zippychord_state_machine` — the stateful PBT. **GREEN** (the
  backspace under-count it once reproduced is fixed in `zippychord.rs`; its two
  shrunk cases are pinned as regressions). Seed persisted in
  `proptest-regressions/state-machine/kanata_state_machine__tests__sim_tests__kanata_proptest__Sut.jsonl`.
- `reference_tests::*` — reference self-consistency unit tests (GREEN); validate
  the oracle's placement logic against hand-computed expectations.
- `zippychord_sim_tests::repro_overlap_underdelete` — minimal deterministic
  reproduction of the (now-fixed) under-count bug (**GREEN**).

## Framework generalisation (prototype)

`kanata_proptest.rs` is being grown from a zippychord-only PBT into a general
harness that hosts one feature module per Kanata construct. The governing
principle is the same oracle split as zippychord, stated generally:

> **The transition carries the "what"; the reference carries the "where/whether".**

Three oracle tiers, cheapest/broadest first:
1. **Construction oracle** (generator-carried): generate the gesture in the
   *unambiguous interior* of a feature's behaviour so the expected output is
   known by construction, no timing/decision model needed.
2. **Invariant oracle** (universal, no prediction): holds for every feature and
   every gesture, so it still applies to compositions we can't predict —
   `check_no_double_press`, modifier balance, clean-release, determinism.
3. **Reference-state oracle** (model-based): reserved for features whose output
   depends on accumulated history with a *simple* model (zippychord placement,
   layer stack, oneshot, caps-word).

Observable: the normalised **output event stream** (ordered ↓/↑ of output keys,
`event_seq`), with net visible text (`net_text`) as a derived helper for the
text-expander features.

Members so far:
- `zippychord_state_machine` (`KanataRef`/`KanataModel`/`Sut`) — the COUPLED
  model (tier 1 + tier 3). Hosts the zippy dictionary AND optional tap-hold keys
  over one shared reference state: a tap-hold key's resolved tap/hold output feeds
  the SAME zippy enable-state + visible buffer via `type_literal` (the atomic
  cross-cutting coupling — a tap-hold tap disables zippy exactly like a literal).
  Observable: net visible text. Tap-hold inputs are a disjoint alphabet from chord
  keys, so the chord×tap-hold *overlap* (the deadline race) is NOT generated here
  — it stays in the ignored determinism test below.
- `taphold_state_machine` (`ThRef`/`ThModel`/`ThSut`) — tap-hold in ISOLATION,
  a pure construction oracle (tier 1) on the lower-level **event stream**
  (`event_seq`), with tap output ≠ input. **KEPT** (not retired into the coupled
  model): per the convergence rule a narrow slice earns its place when it provides
  something the wide test can't — here the event-stream observable (preferred over
  net-text; see observables-and-pitfalls.md) and the tap≠self coverage region,
  plus speed/isolation (256 cases, no zippy parse). The catalog-coverage half of
  the deletion safety-gate is asserted by
  `catalog_selection_tests::coupled_slice_subsumes_standalone_taphold_catalog`; if
  the observable + tap≠self teeth are ever folded into the coupled model, that gate
  licenses deletion.
- `interaction_taphold_zippy_order_independent` — the chord×tap-hold *overlap* (a
  key that is BOTH a tap-hold action AND a chord participant); the construction
  oracles can't predict the combined output, so a tier-2 *determinism* oracle
  judges it (a chord is a set ⇒ press order must not change the text). `#[ignore]`d
  because it is RED against current behaviour: it auto-shrinks to
  `deadline=10, hold_gap=10` and reproduces the documented press-order bug
  (space-first `no ` vs n-first `n `). Un-ignore once fixed.

### Capability-selected invariant catalog (the shared spine)

Decomposition discipline: features are modelled as the real runtime *components*
(`Cap`: `OutputKeyState`, `VisibleText`, `ZippyState`, `LayoutState`) — never one
cap per feature, never a cap named after a property. Each tier-2 invariant is
authored once in a single `INVARIANTS` catalog with a need-set; a slice declares
which components its keymap exercises, and `run_invariants` runs exactly the
selected subset over each tick's event stream.

- Selection rule: `runs ⟺ needs_pos ⊆ present ∧ needs_neg ∩ present = ∅`. The
  negative half is reserved for degraded-mode twins (smart-space full vs none).
- `OutputKeyState` is universal, so legality invariants run over every slice for
  free — coverage is multiplicative.
- `catalog_selection_tests` guards against vacuous greening: every invariant has
  a non-empty positive footprint, each slice selects a non-empty expected set,
  selection is monotone under adding components, and a *planted* double-press is
  caught when selected yet honestly skipped (not stubbed) when `OutputKeyState`
  is absent.

Both `Sut` (zippy) and `ThSut` (tap-hold) now run this one catalog, selected by
their component sets — the behavior-identical first extraction step.

### Next steps toward full interaction coverage

DONE: the shared keymap is folded into a single coupled reference
(`KanataModel`); tap-hold and zippy are projections of one shared state (not a
union). Remaining:

1. **Lift the chord×tap-hold overlap into the coupled model.** Today chord keys
   and tap-hold inputs are disjoint alphabets, so the deadline-race overlap is
   only exercised by the ignored `interaction_*` determinism test. To bring it
   into the coupled model the reference must predict (or metamorphically judge)
   that overlap — and it is RED until the press-order bug is fixed.
2. **Lift the deferred tap-hold narrowings** (see the list below): tap output is
   currently always = the input key, and `FreeType`/`Literal` exclude tap-hold
   input keys (a tap-hold delays its output, reordering it past a plain key in the
   same gesture — which the naive append oracle can't place).
3. Add layers, oneshot, tap-dance, caps-word as further construction-oracle
   modules over the same coupled model.

## Covered now
- Top-level chords; key-set semantics; optional leading space (swallowed).
- Overlap within a hold via randomized press/release order (the bug lives here).
- Multi-word sequences (fresh activations append).
- Single-key followups (incl. multi-level).
- Outputs: lowercase + uppercase letters + space.
- smart-space none / add-space-only (full is generated but behaves like
  add-space-only here since no punctuation is typed).
- Enable/disable timing: a literal keystroke disables (→ WaitEnable); a "full"
  idle re-enables; "tiny" idles keep it disabled. `idle-reactivate-time` is
  fixed large so the countdown only crosses on a full idle, never mid-hold.
- Free typing (`Tr::FreeType`): hold an arbitrary set of keys, NOT targeted to a
  chord, predicted as naive literal append. NOTE: the PBT first ran free typing
  over the FULL alphabet (chord keys included) and *discovered* that free-typed
  chord keys incidentally trigger chords (the naive oracle mispredicts, e.g.
  free-typing `b` fires `b→a`). Per that feedback, the free-typing alphabet now
  excludes every configured chord key (`chord_keys()` of the current dict), so
  free typing can no longer form a chord. `preconditions` re-checks this (and
  that `ChordExpansion` press/release sets still match their target) so dictionary
  shrinking can't resurrect an inconsistent transition and cause a spurious fail.

## Deferred dimensions (generate-and-narrow TODO — lift as the oracle grows)
1. `⌫` (backspace) in output (suffix-chord pattern) — interacts with committed
   text; "tail length owned by an activation" is ambiguous. (`OutItem::Backspace`
   exists but isn't generated.)
2. Fresh root pressed while a followup is pending — press order can trigger the
   pending followup mid-hold (erasing the prior word); intended semantics
   unsettled. Generation currently offers only followup targets while a followup
   is pending.
3. smart-space `full` punctuation auto-erase (no punctuation keys generated yet).
4. AltGr / ShiftAltGr / no-erase / single-output outputs and
   `output-character-mappings` (only reachable via that config).
5. Multi-key followup groups and double-space (Washington-style) sequences.
6. Held modifiers (shift/altgr) and caps-word context during typing.
7. Key-repeat events; non-letter chord input keys; punctuation/digit output chars
   (would need `net_text` extended to decode them).
8. Tap-hold (coupled model): tap output is always = the input key; hold output is
   a distinct non-chord key. Arbitrary tap/hold outputs and tap-hold flavours
   (`tap-hold-press`/`-release`, layer holds) are not generated in the coupled
   model (the standalone `taphold_state_machine` covers tap≠input on the event
   stream).
9. `FreeType`/`Literal` exclude tap-hold input keys: a tap-hold DELAYS its output,
   so it lands out of press order relative to a plain key in the same gesture
   (SUT `wv`, naive append oracle `vw`), and a `Literal`'s brief settle leaves the
   tap half-resolved. The single-key `TapHoldTap`/`TapHoldHold` transitions cover
   tap-hold output; multi-key free typing over tap-hold keys is deferred.
10. The chord×tap-hold *overlap* (a key that is both a chord participant and a
    tap-hold action) — the deadline race. Generated only by the ignored
    `interaction_taphold_zippy_order_independent` test, not the coupled model;
    RED until the press-order bug is fixed.

## Triage workflow
On a `kanata_proptest::zippychord_state_machine` failure:
1. Reproduce via the persisted seed.
2. Classify:
   - **impl bug** — the reference matches hand-verified behavior (e.g. the
     under-count family below).
   - **reference gap** — a not-yet-modeled construct leaked into generation; fix
     the reference or tighten the generator and note it here.
   - **ambiguous** — intended semantics genuinely unspecified; document & decide.
3. The failure message prints the transition, cfg, dict and raw event log.

## Bug surfaced & FIXED: incomplete reset (`zchd_same_hold_activation_count`)
The stateful test (which reconfigures zippychord on every case and can panic
mid-scenario during shrinking) revealed that `zchd_reset()` — called by
`zch_configure` on every config load — did not reset
`zchd_same_hold_activation_count` (it was only cleared on a clean all-keys
release). A reset mid-activation therefore left it stale, and since it gates the
common-prefix logic it leaked across tests (flaky `sim_zippychord_smartspace_overlap`).
Fixed in `zippychord.rs` `zchd_reset` by zeroing it. (`simulate_with_file_content`
also now clears the global `PRESSED_KEYS` defensively.)

## Confirmed bug: backspace under-count (RED, not yet fixed)
When an activation reuses characters from a prior eager activation via the
common-prefix optimization (`zippychord.rs` ~lines 344/394), those reused chars
are not counted toward deletion, so a following overlapping/followup activation
backspaces too few characters and leaves stray text.
- Minimal deterministic repro (`repro_overlap_underdelete`): dict `b`→"c",
  ` b`→"cfbcc", ` bd`→"fee "; pressing b,SPACE,d yields "cfee " instead of "fee ".
- The state machine independently shrinks to e.g. roots `d`→"bBA", ` d`→"ba",
  ` cd`→" AA "; pressing d,SPACE,c yields "b AA " instead of " AA ".
A one-line fix (seed the delete counter with the common-prefix length) fixes the
minimal case but NOT all manifestations — the accounting needs a broader rework.
Fix deliberately deferred until the regression base is solid (this is it).
