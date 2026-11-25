#import "/bindings/aod/aod_schema.typ": *

= Compile-Time Firewalls and Negative Controls <sec:firewalls>

#aod-paragraph("Setup.")
Five compile-time firewalls enforce AOD ontology separation statically before any IR, native object file,
or JIT machine code is emitted (#mainappendix("main:app:closure-role-separation"),
#mainappendix("main:app:path-complexity-quarantine")):
1. *Transitive Read-Only Observer Firewall:* every `observe` block and every `dec` function transitively
   called inside a probe checks under `Effect::ReadOnlyObserver` with `ReadOnly<ChamberView>`. Requesting a
   non-`ReadOnly` `ChamberView`, stripping `ReadOnly<T>` via positional constructors (`Struct(ro)` /
   `Enum::Var(ro)`), or invoking `simulate` fails with `ObserverMutationForbidden`.
2. *One-Way `quarantine Q_ext` Firewall:* external decimal parsers, SI constants, post-freeze energy maps,
   and half-to-even rounding/display helpers (`qext_round_half_even`, `qext_format_fixed`) reside exclusively
   in `aod::qext` or `quarantine Q_ext` submodules (#mainequation("main:eq:m3-observation-map"),
   #mainequation("main:eq:m3-conditional-si-anchor"), #mainequation("main:eq:m3-cs133-tau-interface")).
   A `quarantine Q_ext:` sink block may read immutable native values, emit quarantined renderings, and
   perform comparisons, yet can never return, assign outward, or leak `QExt<T>` back into native AOD code.
3. *Odd-Length `_yro` Support Firewall:* odd support lengths without `_yro` (such as `3:3:7`) and even
   support lengths carrying `_yro` (such as `3:3:6_yro`) fail at compile time with `OddSupportRequiresYro`.
4. *Zero Implicit Coercions and Zero Floats Firewall:* mixed-type arithmetic (`int + t32`, `int + ratio`)
   and float literals fail at compile time.
5. *Exhaustive Checking and Hermetic `DecIR v2` Verification:* unreachable branches are still exhaustively
   checked, and `aodc exec` rebuilds and verifies embedded source SHA-256 digests without consulting the
   ambient filesystem or `AOD_STDLIB_DIR`.

#aod-paragraph("Derivation.")
Both 2-segment (`SiMetrology::DELTA_NU_CS`) and 3-segment (`metrology::SiMetrology::DELTA_NU_CS`)
cross-module `quarantine Q_ext` paths resolve under identical effect checks across `checker.rs`, `eval.rs`,
and `codegen.rs`. Attempting to read any quarantined SI constant outside `Effect::QExt` fails statically:

```dec-fail QExtLeakageForbidden
use stdlib::aod::metrology

dec main:
    () -> int:
        let leaked = metrology::SiMetrology::DELTA_NU_CS
        emit leaked
        return 0
```

Likewise, mutating a read-only observer snapshot or omitting `_yro` on an odd support fails statically:

```dec-fail ObserverMutationForbidden
use aod::core::list_push

observe HostileProbe:
    tracer mutate_view:
        (view: ReadOnly<ChamberView>) -> Null:
            let _bad = list_push(view.carriers, 1)
            return null

dec main:
    () -> int:
        return 0
```

```dec-fail OddSupportRequiresYro
dec main:
    () -> int:
        let bad = 1:2:9
        emit bad
        return 0
```

Runtime invariant audits also preserve deterministic failure across Exact VM, AOT `-O2`, and ORC LLJIT `-O2`:

```dec-fail AssertionFailed
use stdlib::aod::wavelet::require_seated_duad

dec main:
    () -> int:
        let _bad = require_seated_duad(1, 1, false)
        return 0
```

#aod-paragraph("Falsification.")
All seven repository negative controls (`nc01`..`nc07`) are directly bound to this chapter and live-verified
by `aodc manual-verify` across compile-time rejection (`nc01`..`nc03`, `nc07`) and 3-mode runtime failure
(`nc04`..`nc06`):

// @dec-file: manual_examples/negative_controls/nc01_qext_delta_nu_leak.dec
// @dec-file: manual_examples/negative_controls/nc02_observer_mutates_chamber.dec
// @dec-file: manual_examples/negative_controls/nc03_odd_support_missing_yro.dec
// @dec-file: manual_examples/negative_controls/nc04_reversed_sweep_sign.dec
// @dec-file: manual_examples/negative_controls/nc05_origin_j0_as_beat.dec
// @dec-file: manual_examples/negative_controls/nc06_duad_drops_hinge_0h.dec
// @dec-file: manual_examples/negative_controls/nc07_display_quarantine_leak.dec
