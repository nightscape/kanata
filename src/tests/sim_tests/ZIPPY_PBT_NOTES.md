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
- `interaction_taphold_zippy_order_independent` and
  `zippychord_sim_tests::sim_zippy_taphold_chord_press_order_dependent` —
  **GREEN**: the chord×tap-hold press-order bug, asserted as the intended
  order-independence invariant and now fixed (see the fixed-bug section below).

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
  judges it (a chord is a set ⇒ press order must not change the text). **GREEN**:
  it was RED (reproducing space-first `no ` vs n-first `n `, auto-shrinking to
  `deadline=10, hold_gap=10`) until the press-order bug was fixed — see the
  fixed-bug section below. Its deterministic sibling is
  `zippychord_sim_tests::sim_zippy_taphold_chord_press_order_dependent` (asserts
  the same invariant on a fixed config).

## Testing policy: a real bug is a RED test, never `#[ignore]`d
A known, unfixed bug MUST surface as a failing test. Do not hide it behind
`#[ignore]`, and do not write a "characterization" test that pins the *buggy*
output as if it were correct (that is a green test guarding a bug — the same false
comfort). Assert the *intended* behaviour and let it fail. The suite is therefore
RED whenever an open bug is in scope; that is the signal working as designed.

### Capability-selected invariant catalog (the shared spine)

Decomposition discipline: features are modelled as the real runtime *components*
(`Cap`: `OutputKeyState`, `VisibleText`, `ZippyState`, `LayoutState`) — never one
cap per feature, never a cap named after a property. Each tier-2 invariant is
authored once in a single `INVARIANTS` catalog with a need-set; a slice declares
which components its keymap exercises, and `run_invariants` runs exactly the
selected subset over each tick's event stream.

Invariants run with an `InvCtx` (the raw event stream + a `quiescent` flag = all
physical keys released at end of step), so an invariant can hold only at rest.
Current catalog:
- `no_double_press` (`OutputKeyState`) — a key pressed twice with no release between
  (OS coalesces → the second press is lost). Always.
- `clean_release` (`OutputKeyState`) — at rest, no output key left held (every `↓`
  has a `↑`); catches dangling held keys and subsumes modifier-balance-at-rest.
  Quiescent-only.
- `no_delete_into_void` (`OutputKeyState` + `VisibleText`) — a backspace never
  deletes past the start of the reconstructed buffer (into the user's pre-existing
  text / the prompt). Replays *this step's* output delta on the prior visible text
  (`InvCtx.step_events` / `prev_text`), the per-activation granularity the
  cumulative-stream invariants don't need.

- `no_redundant_prefix_delete` (`OutputKeyState` + `VisibleText`) — efficiency: no
  backspace deletes *below the common prefix* the impl could have preserved across an
  eager/echo/followup replacement. **Live and currently RED** — it surfaces the OPEN
  finding below across every generated instance. (Measuring the *common prefix*, not
  per-position char identity, is load-bearing: an early version flagged a coincidental
  tail-match `b` shared by `bcbbc` and ` aab`, but in a backspace-only model you cannot
  keep index 3 without keeping 0..2 — pinned by `coincidental_tail_match_is_not_redundant`.)

The catalog observable was raised from net-text to the **event stream** here: these
invariants judge the keystrokes net-text collapses. The motivating win is a bug class
net-text is structurally blind to — *net-neutral* keystroke waste (delete-then-retype
that nets to the correct text); that is exactly the OPEN finding below.

**Disabling invariants per run.** `run_invariants` honors a comma-separated
`KANATA_PBT_DISABLE_INVARIANTS` env var (parsed by `parse_disabled_list`,
`run_invariants_filtered` does the skipping). The whole suite is RED by default
because `no_redundant_prefix_delete` is an open finding; to run the **correctness-only
suite green** (e.g. to verify another dimension, or check nothing else regressed),
disable it:
`KANATA_PBT_DISABLE_INVARIANTS=no_redundant_prefix_delete cargo test --lib --features "zippychord simulated_output"`.
This keeps full PBT breadth on the finding (default run flags every instance) while
leaving an escape hatch that avoids masking other regressions. `disabling_an_invariant_skips_it`
guards the mechanism.

Rejected (kept as a lesson): `no_release_without_press` was added and immediately
caught zippychord's **capitalize idiom** — `↑X ↓X` to re-assert a key under shift,
which emits a *phantom* `↑X` when the eager `↓X` was never sent (harmless on a real
OS). That is intended behaviour, so the invariant is unsound as stated and was
dropped rather than weakened to model the idiom. The catalog process rejecting an
unsound invariant *is* the mechanism working.

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
   only exercised by the `interaction_*` determinism test. To bring it into the
   coupled model the reference must predict (or metamorphically judge) that
   overlap. The press-order bug it would have hit is now fixed (deadline freeze),
   so this is a pure coverage lift rather than a blocked-on-bug one.
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
- Followups (incl. multi-level): single- AND multi-key components, each with an
  optional leading space (the double-space "Washington" `␣w␣␣a` form). Multi-key
  followups whose key-by-key formation would transiently match a sibling followup
  or a root are pruned at generation — see deferred dimension 5.
- Outputs: lowercase + uppercase letters + space.
- smart-space none / add-space-only / full. Full is now **non-vacuous**: a
  punctuation literal (`.`, the default smart-space-punctuation `Dot`) is generated,
  and after a full-mode trailing space it triggers the **auto-erase** path (`word .`
  → `word.`), modelled in `type_literal` (mirrors `zippychord.rs` `Sent` state). The
  model recomputes `smart_space_sent` per activation so a prior `Sent` can't go stale
  across a suppressed/no-space activation. Inverting the erase makes the SM RED
  (non-vacuous); `ref_smart_space_full_punct_erases_trailing_space` pins it
  deterministically. (`,`/`;` are in the default set but deferred pending their
  output-key-name decode — see deferred dimension 3.)
- `suppress-space not-first-def-key` (generated only with an active smart-space):
  the trailing space is kept iff the first pressed key equals the chord's first
  definition key (leading space for a leading-space chord, else the min of the
  sorted key set — same rule for roots and followups). The placement oracle predicts it
  exactly from the already-shuffled press order — no new keystroke modelling. A
  deliberate inversion of the predicate (`p == d`) makes the SM RED, confirming
  the dimension is non-vacuous.
- Enable/disable timing: a literal keystroke disables (→ WaitEnable); a "full"
  idle re-enables; "tiny" idles keep it disabled.
- Generated timers (the timers are no longer fixed): `on-first-press-chord-deadline`
  (50–100), `followup-chord-deadline` (50–150, generated *independently* so the
  followup-vs-initial divergence is exercised, not assumed equal), and
  `idle-reactivate-time` (300–600). Bounds are chosen so every gesture still seats:
  the chord deadline stays ≥ 50 (the proven floor for the ≤30-tick gesture span), and
  `wait` stays above the tap-hold settle (250ms) so a tap can't re-enable zippy
  mid-gesture. The dimension-12 `idle_cross` and the WaitEnable `idle_full` are sized
  off the *generated* `followup_deadline` / `wait` (range floors `fd+10` / `wait+20`).
- Followup-deadline expiry (dimension 12, the *crossing* half): while a followup is
  pending an `idle_cross` (range floor `DEADLINE+10`, so it can never shrink below
  the deadline) advances time past the followup deadline. The model arms a
  `followup_until_clear` budget (= `DEADLINE`) exactly when `prioritized` becomes
  `Some`, decrements it on `Idle`, and on crossing clears the followup but stays
  **Enabled** — the cleared-but-enabled state, which is just `prioritized: None`
  (no new enum state was needed; the original deferral over-stated the cost). The
  generator then offers fresh roots again, so the *gesture after the expiry*
  exercises the delete-accounting / common-prefix bookkeeping in that region — the
  same family the backspace under-count bug lived in. Non-vacuous: deleting the
  `prioritized = None` clear (so the model predicts the followup still fires) makes
  the SM RED within ~2 cases. The sub-deadline "still pending" half is still
  deferred — see dimension 12.
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
3. smart-space `full` punctuation auto-erase — *mostly lifted*. The `.` (Dot)
   punctuation literal is generated and its auto-erase modelled (see "Covered now").
   Remaining: `,`/`;` (also in the default set) need their output-key-name decoded in
   `key_to_char`/`net_text`; and a punctuation char as *chord output* (not just a
   literal) is still not generated.
4. AltGr / ShiftAltGr / no-erase / single-output outputs and
   `output-character-mappings` (only reachable via that config).
5. Multi-key followups whose formation is *non-atomic*: a proper subset of the
   followup's keys that exactly matches a sibling followup or a root. Because the
   keys are pressed in shuffled order, that subset is transiently held and the
   smaller chord activates eagerly mid-press — changing which followups are
   pending (or firing a fresh root), so the gesture no longer resolves to the
   intended followup. The oracle assumes atomic formation, so
   `prune_unmodelable_followups` drops these at generation (and on every shrink,
   since it runs inside the `prop_map`). Multi-key followups with no such subset
   overlap ARE covered now. (A subset that only *partially* matches the pending
   followup stays `IsSubset` and keeps accumulating — fine; this was the
   subset-clobber bug fixed below.)
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
    tap-hold action) — the deadline race. Generated only by the
    `interaction_taphold_zippy_order_independent` test, not the coupled model. The
    press-order bug here is now fixed (deadline freeze); lifting it into the
    coupled model remains a coverage TODO (#1 above).
11. `suppress-space (key X)` mode — a held flag key that suppresses the trailing
    space. Orthogonal to press order; would require threading an extra held
    non-chord key through every gesture (and decoding its passthrough in the
    oracle). Covered by the deterministic `sim_zippychord_suppress_space_key` sim
    test instead. Only the `not-first-def-key` mode is generated by the SM.
12. Followup-deadline expiry — *partially lifted*. While a followup is pending its
    deadline counts down (armed when the preceding chord's keys are released).
    - **Crossing (LIFTED, see "Covered now"):** an idle longer than the deadline
      cancels the followup, leaving zippy enabled with no continuation. This is now
      generated (`idle_cross`) and modelled by the `followup_until_clear` budget; the
      cleared-but-enabled state is just `prioritized: None` (no new enum state), and
      the post-expiry fresh-root gesture exercises the delete-accounting region.
    - **Sub-deadline keep-pending (STILL DEFERRED):** a *shorter* idle that leaves the
      followup pending. Modelling it exactly requires tracking the residual budget vs.
      the duration of the *following* gesture (the gesture's own presses keep counting
      down the same deadline), and the deadline *freezes* while the layout is deferring
      output (`layout_pending`, same freeze as the chord-deadline press-order fix) —
      so a tap-hold key in the gesture changes the time-base. The within-deadline
      "still fires" behaviour is already pinned deterministically by
      `sim_zippychord_followup_fires_within_idle_deadline` (and the past-deadline
      cancel by `sim_zippychord_followup_expires_past_idle_deadline`). Lifting this
      half means threading gesture-duration + the freeze flag into the budget model.
    Because the SM never sets `followup-chord-deadline`, the deadline is
    `on-first-press-chord-deadline` (= DEADLINE); `idle_cross`'s range floor
    (`DEADLINE+10`) guarantees it crosses even under shrinking, so the precondition
    `ms >= budget` holds and a sub-deadline idle is never applied while pending (which
    would otherwise leak into the unmodeled "fresh root while a followup is pending"
    corner, dimension 2).

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

## Bug surfaced & FIXED: redundant echo/prefix delete
Raising the coupled observable from net-text to the **event stream** (the
`no_redundant_prefix_delete` invariant) surfaced a real inefficiency the net-text
oracle is structurally blind to. When a chord's output *extends its own echoed input
keys*, zippychord backspaced the echoed prefix and retyped it instead of preserving
it. For `ab`→"abc" typed `a,b`: it echoed `a`, then on chord completion emitted `⌫`
and retyped `abc` — net text correct ("abc"), but the backspace redundant. Almost
every mnemonic chord hit this (`th`→"the", `b`→"by…"); harmless in a reliable pipe but
the redundant deletes corrupt the result when dropped under load / on a laggy remote
(the user-observed failure).
- Distinct from the eager 2-key-inside-3-key case, which zippychord already handled
  *optimally* — `ab`→"XY", `abc`→"XYZ" pressed `a,b,c` preserves "XY" and only appends
  "Z" (the common-prefix optimization worked expansion-to-expansion). The gap was that
  the optimization did not cover the **echoed input keys** vs the output (the *first*
  activation of a hold).
- FIXED in `zippychord.rs`: track the echoed-through keys (`zchd_typed_input`) and, on
  the first activation, reuse `common_output_prefix_len(zchd_typed_input, output)` —
  the same common-prefix optimization that already ran between activations, now also
  between the echoed input and the output. `sim_zippychord_redundant_echo_delete` is
  GREEN; the 9 golden keystroke tests were updated to the new (fewer-backspace) streams
  (net text verified identical before/after — the fix is purely an efficiency change).
- **Residual (still flagged): smart-space delete-and-readd across a followup.** A
  followup's trailing smart-space is added *separately* from `zch_output`, so it falls
  outside the common-prefix: replacing `foo `→`food ` deletes the trailing space and
  re-adds it. `no_redundant_prefix_delete` still fires on this (so the SM is RED by
  default; disable the invariant to run correctness-only green). It is a *distinct*
  inefficiency from the echo delete — fixing it means extending the common-prefix to
  include the trailing smart-space, which touches the delicate delete-accounting; left
  as a separate follow-up.

## Bug surfaced & FIXED: incomplete reset (`zchd_same_hold_activation_count`)
The stateful test (which reconfigures zippychord on every case and can panic
mid-scenario during shrinking) revealed that `zchd_reset()` — called by
`zch_configure` on every config load — did not reset
`zchd_same_hold_activation_count` (it was only cleared on a clean all-keys
release). A reset mid-activation therefore left it stale, and since it gates the
common-prefix logic it leaked across tests (flaky `sim_zippychord_smartspace_overlap`).
Fixed in `zippychord.rs` `zchd_reset` by zeroing it. (`simulate_with_file_content`
also now clears the global `PRESSED_KEYS` defensively.)

## Bug surfaced & FIXED: backspace under-count (common-prefix optimization)
When an activation reuses characters from a prior eager activation via the
common-prefix optimization (`zippychord.rs` ~lines 344/394), those reused chars
were not counted toward deletion, so a following overlapping/followup activation
backspaced too few characters and left stray text.
- Minimal deterministic repro (`zippychord_sim_tests::repro_overlap_underdelete`,
  **GREEN**): dict `b`→"c", ` b`→"cfbcc", ` bd`→"fee "; pressing b,SPACE,d once
  yielded "cfee " instead of "fee ".
- The state machine independently shrank to e.g. roots `d`→"bBA", ` d`→"ba",
  ` cd`→" AA "; pressing d,SPACE,c once yielded "b AA " instead of " AA ".
FIXED in `zippychord.rs` (the delete accounting now includes the common-prefix
length); both shrunk cases are pinned as regressions and `zippychord_state_machine`
is GREEN. The first attempt (seed the delete counter with the common-prefix length
only) fixed the minimal case but not all manifestations, so the accounting was
reworked more broadly.

## Bug surfaced & FIXED: chord×tap-hold press-order (deadline race)
When a chord participant is also a `tap-hold` (or layer) key, the layout *delays*
that key's output until the tap-hold resolves. The `on-first-press-chord-deadline`
counts from the first chord key reaching zippychord, so the outcome was
press-order dependent: pressing the tap-hold key first queues the other key (both
arrive together → chord fires), but pressing the plain key first lets it race
ahead and start the deadline, which then expires before the delayed key arrives
(chord lost). For ` n`→`no` with space as a 200ms tap-hold: space-first `no `,
n-first `n `.
- Reproductions (**GREEN** since the fix):
  `interaction_taphold_zippy_order_independent` (PBT over deadline/hold-gap) and
  `zippychord_sim_tests::sim_zippy_taphold_chord_press_order_dependent`
  (deterministic), both asserting press-order independence (`no ` either way).
FIXED by **freezing the chord deadline while the layout is still deferring
output** — `zchd_tick` only counts the deadline down when `layout_pending` is
false, where `layout_pending = layout.waiting.is_some() || !layout.queue.is_empty()`
is read in `Kanata::tick_states` and threaded through `zippy_tick`/`zch_tick`. The
deadline thus measures how fast the *user* pressed the chord keys, not how long the
*layout* deferred a key's output. Plain-key gestures (no `waiting`) are unaffected,
so the deadline still disables zippy for deliberately-held-then-extended typing.
This is the cross-layer signal the rx-brief / [[zippy-pbt-layout-blindspot]] noted
zippychord lacks when it sees only the post-layout output stream.

## Bug surfaced & FIXED: multi-key followup never activates (subset clobber)
Extending the SM to generate **multi-key followup components** (`xy ab`→…, not
just `xy a`) immediately failed: a followup with >1 key never fired — its keys
were typed literally. Root cause in `zippychord.rs` (`zch_press_key`): when a
followup is pending, the code looked up the input keys in the prioritized
(followup) map, and only *kept* that result if it was `HasValue`; otherwise it
overwrote it with the main-chord lookup. Pressing the first key of a multi-key
followup yields `IsSubset` against the pending followup (a partial match) — not
`HasValue` — so it was discarded in favor of the main-chord lookup, which was
`Neither`, triggering `zchd_soft_reset()` and wiping the pending followup before
the remaining keys could arrive. Single-key followups dodged this because the
first (only) press is an exact `HasValue` match.
- Repro (`zippychord_sim_tests::sim_zippychord_multikey_followup`, **GREEN**):
  dict `xy`→"foo", `xy ab`→"BAR"; pressing x,y then a,b now yields "BAR".
FIXED by only letting the main-chord lookup override the prioritized result when
it is *at least as strong* (`HasValue`), or when the prioritized result was
`Neither`. A prioritized `IsSubset` is now preserved, so a multi-key followup
keeps accumulating its keys until it completes. The SM then surfaced the
non-atomic-formation corner now deferred as dimension 5 above.

## Feature & bug FIXED: separate `followup-chord-deadline`; followup deadline ignored across idle
A new optional `defzippy` config `followup-chord-deadline` gives followup chords a
deadline distinct from the initial `on-first-press-chord-deadline` (it falls back to
that value when unset). The motivation: a tight initial deadline avoids accidental
activations during normal typing, but the same tight value makes followups —
especially multi-key ones — hard to land; the followup is a deliberate continuation
of an already-activated chord, so it can safely use a longer window.

Implementing it exposed a pre-existing bug: the followup deadline was effectively
**ignored across an idle gap**. After a chord activated and its keys were released,
`zchd_release_key` set `ticks_until_disable = 0` and the pending followup persisted
until the next keypress (or the 10s force-reset) — so typing `do` then `s` *seconds*
later still produced `does`. Two root causes:
1. The release path zeroed the deadline instead of arming the followup deadline.
2. `zchd_is_idle()` returned true whenever enabled with no keys held, so the idle
   optimization (`can_block_update_idle_waiting`) froze the countdown anyway.
FIXED by (1) arming `ticks_until_disable = followup_deadline` on the final release
when a followup is pending, (2) making `zchd_is_idle()` false while a deadline is
live (`ticks_until_disable > 0`), and (3) on deadline expiry, distinguishing a
pending-followup expiry (clear the followup, stay **enabled** for a fresh chord) from
an initial-deadline expiry (soft-reset to **disabled**, the accidental-typing guard).
- Repros (**GREEN**): `sim_zippychord_followup_fires_within_idle_deadline` (200ms <
  500ms → fires) and `sim_zippychord_followup_expires_past_idle_deadline` (600ms >
  500ms → cancelled, key passes through). The pre-existing
  `sim_zippychord_non_followup_subsequent_with_potential_followups_available` pins
  that a fresh chord still fires after the followup is cancelled (regression guard
  for over-aggressive disabling).
