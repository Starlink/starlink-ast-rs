# Frame family: C-parity audit

Date: 2026-10-05.
Scope: `Frame`, `Axis`, `SkyAxis`, `SkyFrame`, `SpecFrame`, `DSBSpecFrame`, `TimeFrame`, `FluxFrame`, `SpecFluxFrame`, `CmpFrame`, and `FrameSet` in its role as a `Frame`.
Regions are out of scope (see `region-*.md` in this directory).
`ast-capi` is out of scope: it is unstarted, so no string-attribute dispatch (`astGet`/`astSet` by name) exists for any class.

C line numbers refer to the `reference/starlink-ast` checkout in the working tree on this date (the submodule showed local modifications).
Rust line numbers refer to branch `tickets/DM-54793` at `7a32e13`.

## Method

1. Every `vtab->X = X` assignment in each class's C source was listed and mapped onto a Rust trait method, inherent method, or "no counterpart".
2. Every C `GetAttrib` attribute name was checked for typed get/set/test/clear accessors in Rust.
3. Each C method with a Rust counterpart was compared by reading both implementations, in four class groups.
4. Most HIGH findings were confirmed with paired probes: a C program linked against the built reference `libast`, and a scratch Rust crate depending on `ast-core`, run on the same inputs.
5. The findings marked **re-verified** below were additionally checked by reading the code a second time, independently of the group audit.

Findings marked UNVERIFIED were established by reading only, and say what would settle them.

## Summary

Attribute *accessors* are essentially complete (closed 2026-09-30); the gaps are in *behaviour*.
Seven root causes account for most findings:

| Theme | What is wrong | Findings |
|---|---|---|
| A. FrameSet integrity | C restores integrity with one `astConvert(old, current)` (frameset.c:9370-9440). Rust asks each class for `map_from`, and Flux, SpecFlux and BasicFrame return a constant `UnitMap`. SkyFrame aligns in the wrong System, and CmpFrame inherits all of these. | FS-1 to FS-4, SK-4 |
| B. Overlay dispatch | C dispatches `astOverlay` on the *template's* class. Rust dispatches on the *result's* class and returns early on a class mismatch, so attributes are lost. SubFrame also never overlays the *target's* attributes onto the result. | FR-2, FR-3, SK-10, DSB-1, FX-3, FX-4 |
| C. Conversion arithmetic | Some conversions give wrong numbers: swapped arguments, a missing default, missing steps, and an unused AlignTimeScale. | SK-1, SK-2, SK-3, SP-1, DSB-1, TF-1, FX-1 |
| D. System side effects | Setting or clearing System does not re-express SkyRef, restore Unit, or clear Label/Symbol/Title, as C does. | SK-6, SK-7, SP-4, TF-3 |
| E. Non-class sub-frames | Picking axes into a non-class result carries the wrong Label, Unit, Domain and Symbol, and the selection is validated differently. | FR-7, SP-5, TF-4, SFX-3 |
| F. Native load | Shared Frame keys, deprecated synonyms, and the SpecFluxFrame Frame part are not read. | FR-8, LD-1, LD-2, SFX-1 |
| G. UseDefs | The Object attribute is absent. Rust always behaves as UseDefs=1. | XC-1 |

**Why the tests missed these.** The SkyFrame conversion tests are round-trip only, and a swapped-argument pair round-trips perfectly (SK-1). The FrameSet tests change attributes that SkyFrame handles but no others (theme A). The DSB tests use templates with the same System as the target (DSB-1).
A single-point oracle assertion per conversion would have caught every theme-C finding.

## Progress

| IDs | Status | Evidence |
|---|---|---|
| SK-1, SK-2, SK-3, SK-8, SK-9 | FIXED 2026-10-05: `make_sky_mapping` follows `MakeSkyMapping` branch for branch (AMP argument order, J2000 via `J2000H`/`HJ2000`, HELIOECLIPTIC via `HEEQ`/`EQHE`, the `EQREF` equinox substitution, and `None` for an UNKNOWN step) | `tests/test_sky_convert.rs`: 1584 `astConvert` cases over every System pair and ten AlignSystems, exact except three AZEL cases licensed in `SINCOS_MERGE_LICENSED` |
| FS-1, FS-2, FS-3, FS-4, SK-4, SK-5 | FIXED 2026-10-05: every Frame class restores FrameSet and Region integrity through one port of `RestoreIntegrity` (`find_frame::integrity_mapping`): the recorded Frame takes the current Domain, both carry the `INTFLAG` frame flag, `convert_frames` finds the Mapping, and no conversion raises `AST__ILOST`. The per-class `map_from` overrides are gone. SpecFrame, DSBSpecFrame and SkyFrame read the flag as C does, which ports the SkyFrame AlignOffset shortcut (SK-5) and the DSBSpecFrame sideband composition around the parent SubFrame's Mapping | `tests/test_frameset_integrity.rs`: 23 cases across every Frame class; three last-bit cases excluded pending the GHz unit-zoom rounding (`UNIT_ZOOM_ROUNDING_EXCLUDED`) |
| SK-6, SK-7 | FIXED 2026-10-05: `set_system` and `clear_system` re-express a set SkyRef and SkyRefP in the new System (skyframe.c `SetSystem`, `ClearSystem`), and `clear_system` reverts the System to ICRS | `sky_alignoffset_galactic` in `tests/test_frameset_integrity.rs` |
| SP-1, SP-2, SP-3, DSB-1, DSB-2 | FIXED 2026-10-05: `make_spec_mapping_full` follows `MakeSpecMapping`: RestFreq defaults to 1.0E14 Hz and is compared exactly, steps 2 and 7 are skipped only for a relative system with an unchanged RestFreq, the SpecMap is always bracketed by unit Mappings with C's no-match rule and report flag, and the result is simplified. A DSBSpecFrame is a SpecFrame wherever C asks `astIsASpecFrame` (`as_spec_frame`), its Match is the SpecFrame Match, and its SubFrame result is always a DSBSpecFrame carrying the overlaid SpecFrame state | `tests/test_spec_convert.rs`: 208 cases, every System pair with and without RestFreq, the units checks, and DSBSpecFrame templates including one found in a (sky, DSB) cube |
| TF-1, TF-5 | FIXED 2026-10-05: `make_time_mapping` ports `MakeTimeMapping`: when the Frames differ in clock position, DUT1, DTAI or LTOffset and a conversion to or from the AlignTimeScale depends on it, the Mapping is made in two parts through MJD in the alignment timescale; SubFrame takes the alignment Frame from a TimeFrame template. `make_map` uses one Frame's observer throughout, as C's `MakeMap` does, compares origins with `astEQUALS(..., 1.0E3)`, and adds MJDTOMJD on an exact difference | `tests/test_time_convert.rs`: 500 cases. 147 differ because of the TimeMap port (a constant offset in TCG, about 0.5 ms in TT/TDB, last bits elsewhere), in identical Frames too; they are excluded by name in `TIMEMAP_EXCLUDED` |
| FR-1 | FIXED 2026-10-05: an unset MinAxes defaults to Naxes but no more than a set MaxAxes, an unset MaxAxes to Naxes but no less than a set MinAxes, and setting either moves a set value of the other (frame.c `DefaultMinAxes`, `DefaultMaxAxes`, `ConsistentMinAxes`, `ConsistentMaxAxes`); CmpFrame keeps its 0 / 1000000 defaults. This made reachable a PermMap that the simplifier never finished simplifying, fixed alongside (permmap.c `NullPerm` treats an unstored array as null) | `tests/test_min_max_axes.rs`: 30 attribute cases and 40 FindFrame cases; `tests/test_permmap.rs` |
| SK-11 | PARTLY FIXED 2026-10-05: LAST is computed only when a step needs it, and truncated and normalised as C serves it; C's history-dependent cache is a documented divergence | `docs/issues/c-library-quirks.md`, "SkyFrame LAST depends on which LAST values the process computed earlier" |

## Recommended order

1. **SK-1, SK-3, SK-2.** These are argument-order and step-selection fixes in `sky_convert.rs`, each exposed by a one-point oracle test.
2. **Theme A.** Port C's `RestoreIntegrity` as one `convert`-based routine with the Domain copy and `AST__ILOST`, rather than patching each `map_from`. This fixes FS-1 to FS-4 and SK-4 together.
3. **SP-1** (RestFreq default) and **TF-1** (AlignTimeScale).
4. **Theme B.** Make Overlay template-dispatched, and add the target overlay in SubFrame. This fixes DSB-1, which is a HIGH finding.
5. **FX-1, FR-1.**
6. Themes D, E and F, then the MED and LOW tail.

## Findings by class

Ratings: HIGH means wrong coordinates or conversion, or silent data loss. MED means a wrong default, a missing error, or a wrong dump. LOW means cosmetic, latent, or internal-only.
"Probed" means confirmed by paired C/Rust probes.

### Frame base and Axis

| ID | Rating | C | Rust | Finding | Exposed by |
|---|---|---|---|---|---|
| FR-1 | HIGH | frame.c:3277 `DefaultMaxAxes`, :3345 `DefaultMinAxes`, :2601/:2690 `ConsistentMax/MinAxes` | frame_attrs.rs:692-725 | An unset MaxAxes should default to `max(naxes, MinAxes)`, and an unset MinAxes to `min(naxes, MaxAxes)`. Setting one should also push the other to stay consistent. Rust uses `naxes` and only stores the value. As a result, a 2-axis template with MinAxes=3 never matches anything. **Re-verified.** | Oracle cases `MinAxes=3 → MaxAxes` and `MaxAxes=1 → MinAxes`; `find_frame` of that template against a 3-D Frame. |
| FR-2 | MED | frame.c:11033 (SubFrame overlays the target onto the result), :8536-8546 | basic.rs:851-867; find_frame.rs `default_sub_frame` | The result of `pick_axes`, `find_frame` or `convert` loses the target's Dtai, Dut1, Digits, Epoch, Obs* and System. Only Domain and Title are copied. | `pick_axes` on a Frame with Epoch and Digits set. |
| FR-3 | MED | frame.c:8536-8600 | basic.rs:876-909 | When the template is not a BasicFrame and the result is one, only Domain and ActiveUnit are overlaid. Reachability via the CmpFrame-template path is UNVERIFIED. | `find_frame` of CmpFrame(SpecFrame, Frame) against a 2-D Frame. |
| FR-4 | MED | frame.c:10775-10852 SetUnit, :2527-2597 ClearUnit, :7583 NewUnit | basic.rs:713; frame_attrs.rs:1021 | With ActiveUnit set, C rewrites a set Label and Symbol through `astUnitMapper` and clears Format. It also strips trailing spaces. Rust only stores the value. When fixing this, note that `default_sub_frame` must keep the non-NewUnit path, as C uses `astSetAxisUnit` there. | Label "Dist", Unit m→`log(m)` gives the Label `log( Dist )`. |
| FR-5 | MED | axis.c:637-845 AxisFormat, :2200-2240 | axis_format.rs:164-199 | Format keeps only the conversion code and precision. Dropped or wrong: width, the `-+ 0#` flags, literal text, lowercase `e`/`g` exponents, and `%x`/`%o`/`%X`/`%c`/`%u` (formatted as decimal). An unknown code falls back to `%G` (line 196), where C passes the spec to `sprintf`. The `&` log flag gives `10^3` in C even without Plot; Rust gives `1000`. | Format corpus: `%8.3f`, `%+e`, `%x`, `%&G`, `%.1f deg`, `%#G`. |
| FR-6 | MED | frame.c:7691-7740 | basic.rs:802-805 | `BasicFrame::norm` does nothing, even on a SkyAxis picked out of a SkyFrame. | Pick one SkyFrame axis, then `norm([7.0])` should give 7−2π. |
| FR-7 | MED | frame.c:8790 ValidateAxisSelection, :11006, :11064 | mod.rs:1470-1509 | Duplicate axes are accepted (C raises AST__SELIN). An out-of-range or −1 selection raises AST__AXIIN, where C pads with a default axis and a BAD mapping. A zero-axis pick is rejected, though C allows it. An identity pick returns a PermMap where C returns a UnitMap. | `pick_axes([1,1])`; mapping class of `pick_axes([1,2])`. |
| FR-8 | MED | frame.c:15226-15290, :15200 | basic.rs:1025-1059 | Load ignores active per-axis keys (`Lbl1="X"`), does not raise AST__ATTIN for a bad `System`, and rejects `Naxes=0`. | Read `Begin Frame Naxes=1 Lbl1="X" End Frame`. |
| FR-9 | LOW | frame.c:1442 | mod.rs:1139-1169 | `ax_angle` with axis 0 or an axis > naxes panics instead of raising AST__AXIIN. | `ax_angle(a, b, 0)`. |
| FR-10 | LOW | axis.c:2643-2663; frame.c:12585 | frame_attrs.rs:1050; basic.rs:353 | Setting Top, Bottom or Epoch to BAD counts as unset in C. Rust stores it as a set value. | `set_top(1, BAD_VALUE); test_top(1)`. |
| FR-11 | LOW | axis.c:1335 | basic.rs:588-625 | Unformat does not accept hex floats (`0x10`). Behaviour on `1e+x` is UNVERIFIED. | Unformat `"0x10"`. |
| FR-12 | LOW | `%1.7G` of −0.0 | axis_format.rs:14 | Prints `0` where C prints `-0`. | `format(1, -0.0)`. |

### SkyFrame and SkyAxis

The pinned C has no HADEC system, so its absence is not a gap.
The hot path is clean: `set_epoch_mjd` (skyframe.rs:415) and `set_sky_ref_is` (:522) are single field writes.

| ID | Rating | C | Rust | Finding | Exposed by |
|---|---|---|---|---|---|
| SK-1 | HIGH | skyframe.c:6243, 6316, 6492 `TRANSFORM_2("AMP", epoch, 2000.0)` | sky_convert.rs:409, 418 | The AMP arguments are swapped. SlaMap expects `(date, equinox)` (slamap.rs:1180), so every GAPPT and AZEL source frame computes `mappa(eq=epoch, date=2000)`. GAPPT(2010)→FK5: C gives (0.99715, 0.49942), Rust gives (5.68908, −0.06549). Probed; **re-verified.** | One-point GAPPT→FK5 and AZEL→FK5 oracle test. |
| SK-2 | HIGH | :6230-6233, :6339-6341, :6478-6481, :6596-6599 `J2000H`/`HJ2000` | sky_convert.rs:382-386, 475-477 | J2000 is treated as ICRS, so the frame-bias step (about 15 mas) is missing. Probed. | J2000→ICRS oracle point. |
| SK-3 | HIGH | :6250, :6323, :6463, :6576 `HEEQ`/`EQHE` | sky_convert.rs:402-405, 493-495 | HELIOECLIPTIC uses the geocentric `Ecleq`/`Eqecl`. HELIO(2010)→FK5: C gives (5.73323, 0.31193), Rust gives (0.79560, 0.82292). SlaMap already supports `Heeq`/`Eqhe`. Probed. | HELIO→FK5 oracle point. |
| SK-4 | HIGH | frameset.c:9413 → skyframe.c:10144 | skyframe.rs:1837-1851 | `map_from` aligns in `self.system` rather than the AlignSystem (default ICRS). An Equinox- or Epoch-only change therefore gives a UnitMap. FK5 with Equinox set to 1950 through a FrameSet leaves the point unchanged, where C moves it. FK4→FK5 is about 0.7° wrong. Part of theme A. Probed; **re-verified.** | FrameSet set-Equinox, set-Epoch and set-System oracle tests. |
| SK-5 | HIGH | :10162-10172 | skyframe.rs:2516 (`align_offset` "currently unused") | When both frames have AlignOffset=1 and SkyRefIs≠Ignored, C connects them with a UnitMap. Rust converts through absolute coordinates. Probed: C gives (1.0, 0.5), Rust gives (1.13157, 0.58743). | `convert` between two AlignOffset offset frames. |
| SK-6 | HIGH | SetSystem :9635-9790, ClearSystem :1814-1933 | skyframe.rs:288-305 | Changing System does not re-express a set SkyRef or SkyRefP, so offset frames convert wrongly afterwards. Probed. | SkyRef(1, 0.5) then System=GALACTIC should give (2.85724, −0.34593). |
| SK-7 | MED | astMAKE_CLEAR / GetSystem | skyframe.rs:302-305 | `clear_system` only clears the set flag, so `system()` keeps returning the old value. Probed: C gives ICRS, Rust gives Galactic. | Clear-System test. |
| SK-8 | MED | MakeSkyMapping :6097-6098 (EQREF substitution) | sky_convert.rs:206-331 | For a System with no equinox, Rust uses that frame's default equinox instead of borrowing the other frame's. This changes step skipping. UNVERIFIED by probe. | Galactic(ep1)→Ecliptic B1950(ep2) with AlignSystem=ECLIPTIC. |
| SK-9 | MED | Match/MakeSkyMapping return 0 | sky_convert.rs:227-236; skyframe.rs:2521 | UNKNOWN↔known System: C reports no match, Rust returns `Err(Inter)`, which aborts `find_frame`. Probed. | UNKNOWN→FK5 `find_frame`. |
| SK-10 | MED | Overlay template dispatch, frame.c:8513 | skyframe.rs:2241-2244; find_frame.rs:286 | A plain-Frame template overlays nothing onto a SkyFrame target. The second branch of `skyframe_overlay` (:2390) is dead code. Theme B. Probed. | `find_frame(FK5, Frame(Domain=SKY, Title=Hello, Digits=9))`. |
| SK-11 | MED | GetLAST :3752-3830 | skyframe.rs:763, 2142 | C caches LAST and interpolates linearly within 0.4 day (about 0.1″). Rust converts exactly, so AZEL values will differ. Rust also computes LAST twice per `map_from`/`sub_frame` for every System pair, allocating TimeFrames and a FrameSet on each FrameSet `set_epoch`. The magnitude is UNVERIFIED because SK-1 masks it. | AZEL oracle within 0.4 day, after SK-1 is fixed; bench FrameSet `set_epoch`. |
| SK-12 | MED | skyaxis.c:832-833 | skyaxis.rs:698-700 | A `%` Format on a SkyAxis is ignored, and Rust uses `%1.{digits}G`. C passes the user's format through (its output looks quirky; UNVERIFIED). | `Format(1)=%10.5f`. |
| SK-13 | MED | Match :6855-6870 | skyframe.rs:1975-1981 | Interleaved axes from two SkyFrames (A0, B0, A1, B1): C matches A, Rust finds no match. | `find_frame` on a permuted CmpFrame(Sky, Sky). |
| SK-14 | MED | `astSkyFrame("System=ICRS")` | skyframe.rs:120 | `SkyFrame::new(Icrs)` leaves System unset, so as a template it does not force conversion to ICRS. Whether this is an API decision or a gap needs a ruling. | Galactic target with an ICRS template. |
| SK-15 | LOW | Norm :7180-7196 | skyframe.rs:1424-1426 | With one value BAD, C still normalises the other one; Rust leaves both. The 2π→0 snap is missing. | Norm with a single BAD. |
| SK-16 | LOW | :11436 | skyframe.rs:349 | Equinox is stored for HELIOECLIPTIC, where C does not store it. This affects `test_equinox` and the dump. | Attribute oracle case. |
| SK-17 | LOW | :11658 `fabs` | skyframe.rs:607 | A negative SkyTol is not made absolute. | — |
| SK-18 | LOW | :5370, :5530 | skyframe.rs:1717-1796 | `line_contains`/`line_crossing` do not return the Cartesian side-channel and do not check line ownership. This matters at the Plot step. | Plot step. |
| SK-19 | LOW | GetLonAxis/GetLatAxis are 1-based | skyframe.rs:165-176 | The public `lon_axis()`/`lat_axis()` are 0-based, against the 1-based API rule. `neglon`/`set_neglon` should be `neg_lon` by the naming convention. | — |

### SpecFrame

| ID | Rating | C | Rust | Finding | Exposed by |
|---|---|---|---|---|---|
| SP-1 | HIGH | specframe.c:6020 (default 1e14 Hz), :2629-2630, steps 2/7 at :2782-2888 | specframe.rs:1557-1558, 1667, 1699 | Conversion reads the raw `rest_freq` field and treats unset as 0, so the VLTOFR/FRTOVL steps are silently skipped. The public `rest_freq()` correctly defaults to 1e14. VRAD→FREQ: C gives 1e5 GHz, Rust gives 0. Probed; **re-verified.** | VRAD (no RestFreq) → FREQ. |
| SP-2 | LOW | :2771-2774 | specframe.rs:1650-1658 | Steps 2/7 are dropped for absolute systems too; C drops them only for relative ones. The numbers agree after simplification. C also returns the mapping simplified. | Structural WAVE→WAVE comparison. |
| SP-3 | MED | :2914-2934 | specframe.rs:1728-1760 | When a unit mapper fails, C still matches if the SpecMap is a UnitMap and the units are equal, and otherwise returns no match (AST__BADUN only when reporting). Rust always errors. | Two FREQ frames with the same bad Unit. |
| SP-4 | MED | SetSystem :4094-4120, ClearSystem :573-610 | specframe.rs:407-440 | `set_system` does not clear Label/Symbol/Title. `clear_system` restores no UsedUnit and does not rescale SpecOrigin. Probed: Label stays "MyLabel" where C gives "Wavelength". | Label and Unit after a System change or clear. |
| SP-5 | LOW/MED | :4920-4945 | specframe.rs:2045-2086, 2283-2300 | A non-SpecFrame pick result carries Label "Frequency" and Unit "GHz". C gives "Axis 1" and "", with Domain SPECTRUM set and Symbol SPECTRUM1. Probed. | `pick_axes` with a padding axis. |
| SP-6 | LOW | :1010-1016 | specframe.rs:1284-1285 | `EqualSor` compares raw `Option`s, so "unset" differs from "set to the default". This adds a numerically null step. | Two SOURCE frames, one with SourceVRF set to the default. |

### DSBSpecFrame

| ID | Rating | C | Rust | Finding | Exposed by |
|---|---|---|---|---|---|
| DSB-1 | HIGH | dsbspecframe.c:1154-1170, :1470-1500 | specframe.rs:2103; dsbspecframe.rs:969-1030 | A DSB template overlays no SpecFrame attributes, because the `downcast_ref::<SpecFrame>` fails. DSB FREQ→DSB VRAD at the rest frequency: C gives about 0 km/s, Rust gives 230 with an identity mapping. Theme B. Probed. | `convert` between DSB frames with different System, StdOfRest or Unit. |
| DSB-2 | MED | :976-1040 | dsbspecframe.rs:964-966 | A DSB template matches nothing except DSB or SpecFrame targets, so a CmpFrame(Sky, DSB) cube does not match. Probed. | `find_frame` on an ACSIS-style 3-D CmpFrame. |
| DSB-3 | MED | :780-789 | dsbspecframe.rs:538 | The LO sanity check is `lo < 0` where C checks `LO < |IF|` (AST__ATTIN). Probed. | IF=−4e9, DSBCentre=5e9. |
| DSB-4 | MED | :2051-2085 | dsbspecframe.rs:600-636 | `to_topo_mapping` builds a fresh frame, losing SpecOrigin and using its own AlignStdOfRest. UNVERIFIED. | DSBCentre round-trip with SpecOrigin set. |

### TimeFrame

| ID | Rating | C | Rust | Finding | Exposed by |
|---|---|---|---|---|---|
| TF-1 | HIGH | timeframe.c:3838-4060 | timeframe.rs:914-926, 941-975 | AlignTimeScale is never used. With the same System and TimeScale, Rust takes the shift fast path even when the clock position, Dut1, Dtai or LTOffset differ. LAST with ObsLon 0→1 rad: C shifts by 0.159155 d, Rust by 0. UTC with Dtai set to 40: C shifts by −6 s, Rust by 0. Probed. | LAST↔LAST at two longitudes; UTC↔UTC with different Dtai. |
| TF-2 | MED | :4894-4915 | timeframe.rs:454-458 | With System=BEPOCH, `set_timescale` to anything other than TT is accepted, where C raises AST__ATTIN. Probed. | BEPOCH followed by `set_timescale(UTC)`. |
| TF-3 | MED | :4844-4850, :1025-1030, Overlay :4470-4474 | timeframe.rs:408-428, 333-347, 1700-1705 | System set/clear and Overlay clear only Title; C also clears Label and Symbol. Probed. | Label after `System=JD`. |
| TF-4 | MED | :5340-5370 | timeframe.rs:1666-1676 | A non-TimeFrame pick result drops Unit and sets Domain only if it was set. C gives Unit "d", Domain TIME and Symbol TIME1. Probed. | `pick_axes` on a TimeFrame. |
| TF-5 | LOW/MED | :3537 (`astEQUALS` 1e3), :3720 | timeframe.rs:960, 1158 (`ast_equal`, 1e5) | The TimeOrigin equality tolerance is 100× looser: about 0.2 s at MJD 55000 versus about 2 ms in C. | Two TimeOrigins 0.1 s apart. |
| TF-6 | LOW | :2556-2560 | timeframe.rs:654-670 | Epoch-from-TimeOrigin for GMST, LAST, LMST and non-MJD systems uses the UT1-converted value, where C re-reads the raw origin (a C quirk). UNVERIFIED. | LAST/JD frame with TimeOrigin set and Epoch unset. |

### FluxFrame and SpecFluxFrame

| ID | Rating | C | Rust | Finding | Exposed by |
|---|---|---|---|---|---|
| FX-1 | HIGH | fluxframe.c:1235-1238 | fluxframe.rs:464-465, 798 | `get_spec_frame` and `system_string` read the raw `system` field instead of the unit-derived `system()`. With System implied by Unit, SpecVal is read in Hz rather than Angstrom, the conversion fails, the Title says "Hz", and Region dumps write the wrong System. Probed. | FluxFrame with `Unit=W/m^2/Angstrom` converted to FLXDN. |
| FX-2 | MED | :1514-1530 | fluxframe.rs:351-367 | When Unit is set, System is unset, and no system fits the Unit, C raises AST__BADUN. Rust returns FluxDen. Probed. | `set_unit(1, "m")` then `system()`. |
| FX-3 | MED | Overlay via fluxframe.c:3036 | fluxframe.rs:918-921 (same pattern at specframe.rs:2103-2106) | A plain-Frame template overlays nothing onto a FluxFrame. Theme B. Probed. | `find_frame(FluxFrame, Frame(Title, Label, Epoch, Digits))`. |
| FX-4 | LOW | :2602-2618 | fluxframe.rs:960-974 | A FluxFrame template overlays only Domain onto a non-FluxFrame result. | FluxFrame template with Title set onto a BasicFrame. |
| SFX-1 | MED | specfluxframe.c:1674 → cmpframe.c:10427 | specfluxframe.rs:745-790 | The dump omits the Frame part, the `Axp<n>` perm and `IsA CmpFrame`. Load drops the Title and the axis swap. Probed. | Byte-compare a swapped SpecFluxFrame with Title set. |
| SFX-2 | MED | :1330-1333 | specfluxframe.rs:363-377 | A non-SpecFluxFrame target gets no match; C falls back to CmpFrame Match. Probed. | `find_frame(CmpFrame(Spec, Flux), SpecFluxFrame)`. |
| SFX-3 | MED | :1554-1557 | specfluxframe.rs:455-467, 682-737 | A partial pick gives a BasicFrame where C gives a FluxFrame or SpecFrame. Probed. | `pick_axes(&sff, &[1])`. |
| SFX-4 | LOW | cmpframe.c:4877, 4369 | specfluxframe.rs:243-245, 341-344 | `primary_frame` does not descend into the components, and `system_string` is `None` where C gives "Compound". UNVERIFIED impact. | Region over a SpecFluxFrame, checking the dumped System card. |

### CmpFrame and FrameSet as a Frame

| ID | Rating | C | Rust | Finding | Exposed by |
|---|---|---|---|---|---|
| FS-1 | HIGH | frameset.c:9413 + fluxframe.c:3019-3062 | fluxframe.rs:781-785 | `FluxFrame::map_from` is a constant `UnitMap`, and `with_integrity` (frameset.rs:628-640) skips any UnitMap. Setting `Unit(1)=Jy` gives C 1e26 and Rust 1.0. A System change to SFCBR is accepted silently, where C raises AST__ILOST. Probed; **re-verified.** | FrameSet `set_unit(1, "Jy")` then transform. |
| FS-2 | HIGH | as FS-1 + specfluxframe.c:1554 | specfluxframe.rs:329-331 | `SpecFluxFrame::map_from` is a constant `UnitMap(2)`. Probed; **re-verified.** | As FS-1. |
| FS-3 | HIGH | frame.c Match/SubFrame via frameset.c:9413 | basic.rs:823-825 | `BasicFrame::map_from` is constant, so ActiveUnit is ignored. Changing Unit m→km gives C 1 and Rust 1000. Probed; **re-verified.** | ActiveUnit Frame in a FrameSet, then change the Unit. |
| FS-4 | HIGH | frameset.c:9413 | cmpframe.rs:771-831 | `CmpFrame::map_from` composes its components' `map_from`, so it inherits SK-4 and FS-1 to FS-3. It also returns a UnitMap whenever component axis counts or perms differ (:772-781, :808-810). Probed. | Equinox change on a FrameSet whose current Frame is CmpFrame(Sky, Spec). |
| FS-5 | HIGH | frameset.c vtab Resolve, ResolvePoints, Intersect | mod.rs:943, 1209, 1254 (trait defaults) | FrameSet does not forward these, so a FrameSet with a SkyFrame current uses planar geometry. Probed. | `resolve` on a FrameSet compared with its SkyFrame. |
| FS-6 | MED | frameset.c PermAxes | mod.rs:767-770 vs frameset.rs:922 | Through `dyn Frame`, `perm_axes` only validates. The remapping inherent method is never reached. | `(&mut fs as &mut dyn Frame).perm_axes(&[2, 1])`. |
| FS-7 | LOW | frameset.c IsUnitFrame, PrimaryFrame | mod.rs:1382; frameset.rs:1228 | `is_unit_frame` is constant `true`, and `primary_frame` returns the FrameSet rather than the current Frame's primary. Latent. | — |
| CF-1 | LOW | cmpframe.c PartMatch | cmpframe.rs:1375-1376 | The component MatchEnd is read from the snapshot with the CmpFrame's value pushed in. C reads the component's own value. Read-only. | CmpFrame template with MatchEnd=1 and extra axes. |
| CF-2 | LOW | cmpframe.c:7582 | none (`regions/mod.rs:1394` is Mapping-only) | `RemoveRegions` has no CmpFrame form. This follows from PLAN §9.6 (CmpFrame is not a Mapping). | CmpFrame with a Region component inside a FrameSet. |

C restores integrity only on the string-attribute path (`astVSet` frameset.c:11913-11921, `ClearAttrib` :2663-2671), not in the `MAKE_SET` forwards (:322, :562).
When `ast-capi` lands, its dispatch must route through the integrity routine.

### Native load (cross-class)

| ID | Rating | C | Rust | Finding | Exposed by |
|---|---|---|---|---|---|
| LD-1 | MED | specframe.c load ~182-193; timeframe.c:7248-7263 | specframe.rs:2469+; timeframe.rs:1905+ | The deprecated `GeoLat`/`GeoLon` and `ClLat`/`ClLon` items are not read, so old dumps lose the observer position. | Load a pre-v3 dump containing them. |
| LD-2 | MED | frame.c astLoadFrame | specframe.rs:2469-2620; timeframe.rs:1905-2003 | SpecFrame and TimeFrame loaders skip `frame_attrs::read_frame_attrs`, dropping Digits, MchEnd, MinAx, MaxAx, Permut and Presrv, which the shared writer emits. TimeFrame reads ActUnt asymmetrically. | Round-trip a SpecFrame with MaxAxes=3 and Digits=10. |

### Cross-cutting

| ID | Rating | C | Rust | Finding | Exposed by |
|---|---|---|---|---|---|
| XC-1 | MED | skyframe.c:4606-4689, 10129, 11145 (VerifyMSMAttrs); specframe.c:2116, 5668; dsbspecframe.c:2333; timeframe.c:6398 | none (`rg -i usedefs crates/ast-core/src` finds only a comment) | UseDefs is absent. With UseDefs=0, C refuses alignments that rely on a defaulted Epoch, Equinox, RestFreq and so on, and uses it in the default SkyFrame Title. Rust always behaves as UseDefs=1. This is an Object-level attribute, so the design needs a ruling on where it lives, as there is no Object trait. | `convert` with UseDefs=0 and Epoch unset should fail. |

## C methods with no Rust counterpart

| C method | Classes | Impact |
|---|---|---|
| Abbrev, Centre, Fields, Gap, FrameGrid, AxisGap | Frame, SkyFrame, TimeFrame, CmpFrame | Plot support. Deferred to the Plot step. |
| NormalPoints / NormPoints | Frame, SkyFrame, CmpFrame, FrameSet | Only the public `astNormPoints` reaches it, plus Region mesh code (`region.c:6671, 9385, 10996`). Visible in Region meshes that cross RA=0. Missing public API. |
| AxIn | Frame, Axis, SkyAxis | Range test with sky wrap, used by `box.c`. Visible in Box containment on sky axes. |
| ReportPoints | Frame | Diagnostic output for Mapping `Report=1` only. |
| ValidateAxisSelection | Frame | Its absence shows up as FR-7. |
| CurrentTime | TimeFrame | Public and Fortran API only; no internal caller. |
| GetRefPos | SpecFrame | Public API only. Already in PLAN.md. |
| GetDensitySystem / GetDensityUnit | FluxFrame | Only private equivalents exist, and those carry the FX-1 bug. |
| UseDefs | Object | XC-1. |
| SetAxis | Frame | Covered by the crate-private `set_axis_object` (basic.rs:213). |
| CheckPerm, CleanAttribs, frame flags | Frame | Internal. `validate_permutation` covers CheckPerm. AST__INTFLAG is modelled ad hoc in SpecFrame and is unused in SkyFrame (see SK-5). |

## Stale entries in `docs/issues/frame-mapping-c-port-gaps.md`

These entries should be corrected when the findings above are filed:

- §0 item 3 says every constant-default trait method on FrameSet forwards. That is false for `resolve`, `resolve_points`, `intersect`, `perm_axes` and `is_unit_frame` (FS-5 to FS-7). Its "still open from 6b" list (variants, convert, AST__ALLFRAMES) is marked FIXED in §6b.
- §0 item 4 cites the SkyTol gate at "three" sites; there are two (skyframe.rs:1851, :2537).
- §0 item 7 lists the SkyFrame `NormBox` `reg` argument as deferred. It is superseded by the `inside` predicate (skyframe.rs:1541). It also calls the `&` log rendering Plot-only, which it is not (FR-5).
- §6 lists LineDef, LineContains, LineCrossing and LineOffset as unimplemented; they exist (mod.rs:988-1098) and match C. It says NewUnit fires from Overlay; C calls it only from SetUnit and ClearUnit (frame.c:2591, 10841), and it is reachable today (FR-4). It calls `find_frame.rs` a faithful port, which is not true of the SubFrame target overlay (FR-2, FR-3). Its Format entry omits the gaps in FR-5.
- §6d says Match, SubFrame and Overlay are ported "including offset-coordinate frames". That is contradicted by SK-1 to SK-5, SK-10 and SK-13. Its DEFERRED bullet lists GetDirection, Offset2, NormBox and the Line* methods, all of which now exist. The Epoch-FK4 and Bottom/Top entries are not marked FIXED there, although §0 item 10 records them as fixed.
- §6e calls `make_spec_mapping_full` faithful, which SP-1 to SP-3 contradict. Its DSB test coverage misses DSB-1 and DSB-2.
- §6f says `make_time_mapping` covers all scales but omits AlignTimeScale (TF-1). Its SpecFluxFrame coverage claim is contradicted by SFX-1 to SFX-3.
- The comment at cmpframe.rs:764-769 says the CmpFrame MinAxes/MaxAxes default is `naxes()`. The code (`frame_attrs_match!(min = 0, max = 1_000_000)`, :258) and §6c say otherwise.

## Probe sources

The paired C and Rust probes are in the session scratchpad and are not committed.
To make any finding a regression test, port the probe's inputs and C output into a `crates/ast-core/tests/` oracle case, following `docs/issues/c-oracle-probe-recipe.md`.
