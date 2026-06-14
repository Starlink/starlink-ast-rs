//! Integration tests for SpecFrame.
//!
//! Covers: construction, attributes, system conversion (frequency to wavelength,
//! velocity with rest frequency), standard-of-rest correction, serialisation
//! round-trip, and distance/format operations.
//!
//! Rust equivalent of reference/starlink-ast/ast_tester/testspecframe.c
//! (subset — tests the features implemented in Step 7c).

use ast_core::constants::BAD_VALUE;
use ast_core::coords::PointSet;
use ast_core::frames::{Frame, FrameSet, SpecFrame, SpecSystem, StdOfRest};
use ast_core::mappings::Mapping;
use ast_core::{AnyObject, from_native_string, to_native_string};

// ===========================================================================
// Helpers
// ===========================================================================

/// Speed of light in m/s.
const C_LIGHT: f64 = 2.99792458e8;

/// Assert two floats are within tolerance.
fn assert_close(a: f64, b: f64, tol: f64, label: &str) {
    assert!(
        (a - b).abs() < tol,
        "{label}: expected {b}, got {a} (diff = {:.2e})",
        (a - b).abs()
    );
}

/// Transform a single spectral value through a FrameSet (forward).
fn transform_fwd(fs: &FrameSet, val: f64) -> f64 {
    let pts = PointSet::from_rows([[val]]);
    let out = fs.transform(&pts, true).unwrap();
    out.point(0)[0]
}

/// Transform a single spectral value through a FrameSet (inverse).
fn transform_inv(fs: &FrameSet, val: f64) -> f64 {
    let pts = PointSet::from_rows([[val]]);
    let out = fs.transform(&pts, false).unwrap();
    out.point(0)[0]
}

// ===========================================================================
// Construction and attributes
// ===========================================================================

#[test]
fn test_specframe_construction() {
    let sf = SpecFrame::new(SpecSystem::Freq);
    assert_eq!(sf.naxes(), 1);
    assert_eq!(sf.system(), SpecSystem::Freq);
    assert_eq!(sf.std_of_rest(), StdOfRest::Heliocentric);
    assert_eq!(sf.system_string(), Some("FREQ"));
}

#[test]
fn test_specframe_attributes() {
    let mut sf = SpecFrame::new(SpecSystem::Wavelen);
    sf.set_rest_freq(1.42040575e9); // HI 21cm in Hz
    sf.set_ref_ra(1.234);
    sf.set_ref_dec(0.567);
    sf.set_std_of_rest(StdOfRest::Lsrk);
    sf.set_epoch_mjd(51544.5);

    assert_eq!(sf.system(), SpecSystem::Wavelen);
    assert_eq!(sf.std_of_rest(), StdOfRest::Lsrk);
    assert_close(sf.rest_freq().unwrap(), 1.42040575e9, 1.0, "rest_freq");
    assert_close(sf.ref_ra().unwrap(), 1.234, 1e-15, "ref_ra");
    assert_close(sf.ref_dec().unwrap(), 0.567, 1e-15, "ref_dec");
    assert_close(sf.epoch_mjd(), 51544.5, 1e-15, "epoch");
}

#[test]
fn test_spec_system_parsing() {
    assert_eq!(SpecSystem::parse("FREQ"), Some(SpecSystem::Freq));
    assert_eq!(SpecSystem::parse("freq"), Some(SpecSystem::Freq));
    assert_eq!(SpecSystem::parse("WAVE"), Some(SpecSystem::Wavelen));
    assert_eq!(SpecSystem::parse("VELO"), Some(SpecSystem::Vrel));
    assert_eq!(SpecSystem::parse("VRAD"), Some(SpecSystem::Vradio));
    assert_eq!(SpecSystem::parse("ZOPT"), Some(SpecSystem::Redshift));
    assert_eq!(SpecSystem::parse("BETA"), Some(SpecSystem::Beta));
    assert_eq!(SpecSystem::parse("junk"), None);
}

#[test]
fn test_std_of_rest_parsing() {
    assert_eq!(
        StdOfRest::parse("Heliocentric"),
        Some(StdOfRest::Heliocentric)
    );
    assert_eq!(StdOfRest::parse("HELIO"), Some(StdOfRest::Heliocentric));
    assert_eq!(StdOfRest::parse("LSRK"), Some(StdOfRest::Lsrk));
    assert_eq!(StdOfRest::parse("LSR"), Some(StdOfRest::Lsrk));
    assert_eq!(StdOfRest::parse("BARY"), Some(StdOfRest::Barycentric));
    assert_eq!(StdOfRest::parse("TOPO"), Some(StdOfRest::Topocentric));
    assert_eq!(StdOfRest::parse("Source"), Some(StdOfRest::Source));
    assert_eq!(StdOfRest::parse("garbage"), None);
}

#[test]
fn test_is_velocity() {
    assert!(!SpecSystem::Freq.is_velocity());
    assert!(!SpecSystem::Energy.is_velocity());
    assert!(!SpecSystem::Wavenum.is_velocity());
    assert!(!SpecSystem::Wavelen.is_velocity());
    assert!(!SpecSystem::Airwave.is_velocity());
    assert!(SpecSystem::Vradio.is_velocity());
    assert!(SpecSystem::Voptical.is_velocity());
    assert!(SpecSystem::Redshift.is_velocity());
    assert!(SpecSystem::Beta.is_velocity());
    assert!(SpecSystem::Vrel.is_velocity());
}

// ===========================================================================
// System conversion: frequency <-> wavelength
// ===========================================================================

#[test]
fn test_freq_to_wavelength_convert() {
    // Create a SpecFrame in frequency, convert to wavelength.
    // Set explicit canonical SI units so the converter does not
    // wrap a display-unit scaler (default Freq display is "GHz",
    // default Wavelen display is "Angstrom").
    let mut sf_freq = SpecFrame::new(SpecSystem::Freq);
    sf_freq.set_unit("Hz");
    let mut sf_wave = SpecFrame::new(SpecSystem::Wavelen);
    sf_wave.set_unit("m");

    let fs = sf_freq.convert_to(&sf_wave, &[]).unwrap().unwrap();

    // Convert 5e14 Hz to wavelength: lambda = c / f
    let freq = 5.0e14; // Hz
    let expected_wl = C_LIGHT / freq; // ~6e-7 m = 600 nm
    let result = transform_fwd(&fs, freq);
    assert_close(result, expected_wl, 1e-15, "freq_to_wave");

    // Inverse: wavelength -> frequency
    let freq_back = transform_inv(&fs, expected_wl);
    assert_close(freq_back, freq, 1.0, "wave_to_freq");
}

#[test]
fn test_freq_to_energy() {
    // E = h * f
    let mut sf_freq = SpecFrame::new(SpecSystem::Freq);
    sf_freq.set_unit("Hz");
    let sf_energy = SpecFrame::new(SpecSystem::Energy);

    let fs = sf_freq.convert_to(&sf_energy, &[]).unwrap().unwrap();

    let freq = 1.0e15; // 1 PHz
    let h = 6.6260755e-34; // Planck constant (matches AST)
    let expected_e = h * freq;
    let result = transform_fwd(&fs, freq);
    assert_close(result, expected_e, 1e-50, "freq_to_energy");
}

#[test]
fn test_freq_to_wavenum() {
    // k = f / c
    let mut sf_freq = SpecFrame::new(SpecSystem::Freq);
    sf_freq.set_unit("Hz");
    let sf_wn = SpecFrame::new(SpecSystem::Wavenum);

    let fs = sf_freq.convert_to(&sf_wn, &[]).unwrap().unwrap();

    let freq = 3.0e14;
    let expected_wn = freq / C_LIGHT; // 1/m
    let result = transform_fwd(&fs, freq);
    assert_close(result, expected_wn, 1.0, "freq_to_wavenum");
}

// ===========================================================================
// Velocity conversion (requires rest frequency)
// ===========================================================================

#[test]
fn test_freq_to_vrel() {
    // Relativistic velocity from frequency:
    // v = c * (rf^2 - f^2) / (rf^2 + f^2)
    let rf = 1.42040575e9; // HI line in Hz

    let mut sf_freq = SpecFrame::new(SpecSystem::Freq);
    sf_freq.set_unit("Hz");
    sf_freq.set_rest_freq(rf);

    let mut sf_vrel = SpecFrame::new(SpecSystem::Vrel);
    sf_vrel.set_unit("m/s");
    sf_vrel.set_rest_freq(rf);

    let fs = sf_freq.convert_to(&sf_vrel, &[]).unwrap().unwrap();

    // At rest frequency, velocity should be 0.
    let v_at_rest = transform_fwd(&fs, rf);
    assert_close(v_at_rest, 0.0, 1e-3, "vrel_at_rest");

    // Slightly shifted frequency: approaching (higher freq = negative velocity for receding)
    let f_shifted = rf * 1.001; // slightly higher freq => negative velocity (approaching)
    let v_shifted = transform_fwd(&fs, f_shifted);
    // v = c * (rf^2 - f^2)/(rf^2 + f^2)
    let expected_v =
        C_LIGHT * (rf * rf - f_shifted * f_shifted) / (rf * rf + f_shifted * f_shifted);
    assert_close(v_shifted, expected_v, 1.0, "vrel_shifted");
}

#[test]
fn test_vradio_to_freq() {
    // Radio velocity: v_r = c * (1 - f/rf) => f = rf * (1 - v_r/c)
    let rf = 1.42040575e9;

    let mut sf_vr = SpecFrame::new(SpecSystem::Vradio);
    sf_vr.set_unit("m/s");
    sf_vr.set_rest_freq(rf);

    let mut sf_freq = SpecFrame::new(SpecSystem::Freq);
    sf_freq.set_unit("Hz");
    sf_freq.set_rest_freq(rf);

    let fs = sf_vr.convert_to(&sf_freq, &[]).unwrap().unwrap();

    let vr = 100000.0; // 100 km/s radio velocity
    let expected_freq = rf * (1.0 - vr / C_LIGHT);
    let result = transform_fwd(&fs, vr);
    assert_close(result, expected_freq, 1.0, "vradio_to_freq");
}

// ===========================================================================
// Standard of Rest correction
// ===========================================================================

#[test]
fn test_sor_helio_to_lsrk() {
    // Converting between heliocentric and LSRK requires RefRA and RefDec.
    // The LSRK correction is a purely directional effect.
    let mut sf_helio = SpecFrame::new(SpecSystem::Freq);
    sf_helio.set_std_of_rest(StdOfRest::Heliocentric);
    sf_helio.set_ref_ra(0.0);
    sf_helio.set_ref_dec(0.0);

    let mut sf_lsrk = SpecFrame::new(SpecSystem::Freq);
    sf_lsrk.set_std_of_rest(StdOfRest::Lsrk);
    sf_lsrk.set_ref_ra(0.0);
    sf_lsrk.set_ref_dec(0.0);

    let fs = sf_helio.convert_to(&sf_lsrk, &[]).unwrap().unwrap();

    // For RA=0, Dec=0, the LSRK correction shifts the frequency.
    // The shift depends on the projection of the solar motion onto the
    // line of sight. It should be a small relative shift.
    let freq = 1.42040575e9;
    let result = transform_fwd(&fs, freq);

    // Result should differ from input by a small Doppler factor.
    // |delta_f / f| < 0.001 (typical solar motion ~20 km/s, so ~7e-5)
    let relative_shift = (result - freq).abs() / freq;
    assert!(
        relative_shift < 1e-3,
        "LSRK shift too large: relative shift = {relative_shift:.2e}"
    );
    assert!(
        relative_shift > 1e-8,
        "LSRK shift too small: relative shift = {relative_shift:.2e}"
    );

    // Round-trip: inverse should recover the original.
    let back = transform_inv(&fs, result);
    assert_close(back, freq, 1.0, "lsrk_round_trip");
}

#[test]
fn test_same_system_same_sor_is_unitmap() {
    // When system and SOR are the same, conversion should be identity.
    let sf1 = SpecFrame::new(SpecSystem::Freq);
    let sf2 = SpecFrame::new(SpecSystem::Freq);

    let fs = sf1.convert_to(&sf2, &[]).unwrap().unwrap();

    let freq = 1.234e10;
    let result = transform_fwd(&fs, freq);
    assert_close(result, freq, 1e-10, "identity_conversion");
}

// ===========================================================================
// Frame trait methods: distance, format, unformat
// ===========================================================================

#[test]
fn test_distance() {
    let sf = SpecFrame::new(SpecSystem::Freq);
    let d = sf.distance(&[1e9], &[2e9]).unwrap();
    assert_close(d, 1e9, 1.0, "distance");
}

#[test]
fn test_distance_bad() {
    let sf = SpecFrame::new(SpecSystem::Freq);
    let d = sf.distance(&[BAD_VALUE], &[2e9]).unwrap();
    assert_eq!(d, BAD_VALUE);
}

#[test]
fn test_format_freq() {
    let sf = SpecFrame::new(SpecSystem::Freq);
    let s = sf.format(1, 1.42e9).unwrap();
    assert!(s.contains("Hz"), "Expected Hz units in: {s}");
}

#[test]
fn test_format_redshift() {
    let sf = SpecFrame::new(SpecSystem::Redshift);
    let s = sf.format(1, 0.5).unwrap();
    // Redshift is dimensionless.
    assert!(!s.contains("Hz"), "Redshift should not have Hz: {s}");
}

#[test]
fn test_unformat() {
    let sf = SpecFrame::new(SpecSystem::Freq);
    let (val, nc) = sf.unformat(1, "1.5e9 Hz").unwrap();
    assert_close(val, 1.5e9, 1.0, "unformat_value");
    assert!(nc > 0, "unformat should consume characters");
}

/// SpecFrame does not override Unformat in C (specframe.c has no
/// `static int Unformat`); it inherits the base Frame/Axis contract
/// (`astAxisUnformat`, axis.c:1284). Oracle-verified against the built
/// reference libast: each case below matches `astUnformat` on a C SpecFrame.
#[test]
fn test_unformat_base_contract() {
    let sf = SpecFrame::new(SpecSystem::Freq);
    // Plain number.
    assert_eq!(sf.unformat(1, "12.5").unwrap(), (12.5, 4));
    // Leading and trailing whitespace are both counted in nread.
    assert_eq!(sf.unformat(1, "  12.5  ").unwrap(), (12.5, 8));
    // Trailing non-numeric text stops the scan but is not counted.
    assert_eq!(sf.unformat(1, "12.5xyz").unwrap(), (12.5, 4));
    // The "<bad>" token yields BAD_VALUE, including a spaced/mixed-case form.
    assert_eq!(sf.unformat(1, "<bad>").unwrap(), (BAD_VALUE, 5));
    assert_eq!(sf.unformat(1, " < b a d > ").unwrap(), (BAD_VALUE, 11));
    // Unparseable input: nread = 0, no error.
    assert_eq!(sf.unformat(1, "xyz").unwrap(), (BAD_VALUE, 0));
    assert_eq!(sf.unformat(1, "").unwrap(), (BAD_VALUE, 0));
    assert_eq!(sf.unformat(1, "  ").unwrap(), (BAD_VALUE, 0));
}

// ===========================================================================
// Equality
// ===========================================================================

#[test]
fn test_equal_same() {
    let sf1 = SpecFrame::new(SpecSystem::Freq);
    let sf2 = SpecFrame::new(SpecSystem::Freq);
    assert!(sf1.equal(&sf2));
}

#[test]
fn test_not_equal_different_system() {
    let sf1 = SpecFrame::new(SpecSystem::Freq);
    let sf2 = SpecFrame::new(SpecSystem::Wavelen);
    assert!(!sf1.equal(&sf2));
}

#[test]
fn test_not_equal_different_sor() {
    let mut sf1 = SpecFrame::new(SpecSystem::Freq);
    sf1.set_std_of_rest(StdOfRest::Heliocentric);
    let mut sf2 = SpecFrame::new(SpecSystem::Freq);
    sf2.set_std_of_rest(StdOfRest::Lsrk);
    assert!(!sf1.equal(&sf2));
}

// ===========================================================================
// Serialisation round-trip
// ===========================================================================

#[test]
fn test_serialise_roundtrip() {
    let mut sf = SpecFrame::new(SpecSystem::Wavelen);
    sf.set_rest_freq(1.42040575e9);
    sf.set_ref_ra(1.5);
    sf.set_ref_dec(-0.3);
    sf.set_std_of_rest(StdOfRest::Lsrk);
    sf.set_source_vel(25000.0);
    sf.set_source_vrf(StdOfRest::Heliocentric);
    sf.set_source_sys(SpecSystem::Vrel);
    sf.set_epoch_mjd(51544.5);
    sf.set_obs_lat(0.5);
    sf.set_obs_lon(-1.0);
    sf.set_obs_alt(100.0);
    sf.set_spec_origin(5.0e-7);
    sf.set_align_std_of_rest(StdOfRest::Barycentric);

    let obj = AnyObject::SpecFrame(sf);
    let text = to_native_string(&obj).unwrap();

    // Verify it contains expected keywords.
    assert!(text.contains("SpecFrame"), "Missing class name");
    assert!(text.contains("SoR"), "Missing SoR");
    assert!(text.contains("RstFrq"), "Missing RstFrq");
    assert!(text.contains("RefRA"), "Missing RefRA");
    assert!(text.contains("SrcVel"), "Missing SrcVel");
    assert!(text.contains("SpOrg"), "Missing SpOrg");
    assert!(text.contains("AlSoR"), "Missing AlSoR");

    // Parse back.
    let obj2 = from_native_string(&text).unwrap();
    let sf2 = match obj2 {
        AnyObject::SpecFrame(f) => f,
        other => panic!("Expected SpecFrame, got {:?}", other),
    };

    assert_eq!(sf2.system(), SpecSystem::Wavelen);
    assert_eq!(sf2.std_of_rest(), StdOfRest::Lsrk);
    assert_close(sf2.rest_freq().unwrap(), 1.42040575e9, 1.0, "rf_roundtrip");
    assert_close(sf2.ref_ra().unwrap(), 1.5, 1e-14, "refra_roundtrip");
    assert_close(sf2.ref_dec().unwrap(), -0.3, 1e-14, "refdec_roundtrip");
    assert_close(
        sf2.source_vel().unwrap(),
        25000.0,
        1e-10,
        "srcvel_roundtrip",
    );
    assert_eq!(sf2.source_vrf(), StdOfRest::Heliocentric);
    assert_eq!(sf2.source_sys(), SpecSystem::Vrel);
    assert_close(sf2.epoch_mjd(), 51544.5, 1e-14, "epoch_roundtrip");
    assert_close(sf2.spec_origin().unwrap(), 5.0e-7, 1e-20, "sporg_roundtrip");
    assert_eq!(sf2.align_std_of_rest(), StdOfRest::Barycentric);
}

#[test]
fn test_serialise_minimal() {
    // A minimal SpecFrame should round-trip cleanly.
    let sf = SpecFrame::new(SpecSystem::Freq);
    let obj = AnyObject::SpecFrame(sf);
    let text = to_native_string(&obj).unwrap();
    let obj2 = from_native_string(&text).unwrap();
    let sf2 = match obj2 {
        AnyObject::SpecFrame(f) => f,
        other => panic!("Expected SpecFrame, got {:?}", other),
    };
    assert_eq!(sf2.system(), SpecSystem::Freq);
    assert_eq!(sf2.std_of_rest(), StdOfRest::Heliocentric);
}

// ===========================================================================
// map_from (FrameSet integrity)
// ===========================================================================

#[test]
fn test_map_from_system_change() {
    // Create a SpecFrame, snapshot, change system, compute mapping.
    // Use canonical SI units explicitly so the canonical-units chain
    // is identity (no display-unit scaling).
    let mut sf = SpecFrame::new(SpecSystem::Freq);
    sf.set_unit("Hz");
    let snapshot = sf.snapshot();

    let mut sf2 = SpecFrame::new(SpecSystem::Freq);
    sf2.set_unit("Hz");
    sf2.set_system(SpecSystem::Wavelen);
    sf2.set_unit("m");

    let mapping = sf2.map_from(snapshot.as_ref()).unwrap();

    // Map frequency to wavelength.
    let freq = 5e14;
    let pts = PointSet::from_rows([[freq]]);
    let out = mapping.transform(&pts, true).unwrap();
    let wl = out.point(0)[0];
    let expected = C_LIGHT / freq;
    assert_close(wl, expected, 1e-15, "map_from_sys_change");
}

// ===========================================================================
// Conversion with velocity and rest frequency
// ===========================================================================

#[test]
fn test_wavelen_to_voptical() {
    // Optical velocity: v_opt = c * (lambda/lambda_0 - 1)
    let rf = 1.42040575e9; // Hz
    let lambda_0 = C_LIGHT / rf; // rest wavelength

    let mut sf_wave = SpecFrame::new(SpecSystem::Wavelen);
    sf_wave.set_unit("m");
    sf_wave.set_rest_freq(rf);

    let mut sf_vopt = SpecFrame::new(SpecSystem::Voptical);
    sf_vopt.set_unit("m/s");
    sf_vopt.set_rest_freq(rf);

    let fs = sf_wave.convert_to(&sf_vopt, &[]).unwrap().unwrap();

    // At rest wavelength, v_opt = 0.
    let v = transform_fwd(&fs, lambda_0);
    assert_close(v, 0.0, 1e-3, "vopt_at_rest");

    // 10% longer wavelength: v_opt = c * 0.1
    let wl_shifted = lambda_0 * 1.1;
    let v_shifted = transform_fwd(&fs, wl_shifted);
    let expected_vopt = C_LIGHT * 0.1; // c * (1.1 - 1.0) = 0.1c
    assert_close(v_shifted, expected_vopt, 1e2, "vopt_shifted");
}

// ===========================================================================
// BAD_VALUE handling
// ===========================================================================

// ===========================================================================
// Unit conversion (commit 3 of the unit-engine port)
// ===========================================================================

#[test]
fn test_unit_mhz_to_hz_within_freq() {
    // Source SpecFrame in MHz, target in explicit Hz. Conversion
    // factor: 1 MHz = 1e6 Hz.
    let mut sf_mhz = SpecFrame::new(SpecSystem::Freq);
    sf_mhz.set_unit("MHz");
    let mut sf_hz = SpecFrame::new(SpecSystem::Freq);
    sf_hz.set_unit("Hz");

    let fs = sf_mhz.convert_to(&sf_hz, &[]).unwrap().unwrap();
    let v = transform_fwd(&fs, 1.0);
    // 1 MHz = 1e6 Hz.
    assert_close(v, 1.0e6, 1.0, "mhz_to_hz");
}

#[test]
fn test_unit_kms_to_ms_within_vrad() {
    // 1 km/s = 1e3 m/s.
    let mut sf_kms = SpecFrame::new(SpecSystem::Vradio);
    sf_kms.set_unit("km/s");
    let mut sf_ms = SpecFrame::new(SpecSystem::Vradio);
    sf_ms.set_unit("m/s");

    let fs = sf_kms.convert_to(&sf_ms, &[]).unwrap().unwrap();
    let v = transform_fwd(&fs, 1.0);
    assert_close(v, 1.0e3, 1.0e-6, "kms_to_ms");
}

#[test]
fn test_unit_unchanged_returns_unitmap() {
    // Both default (Hz). Same SOR / system → empty SpecMap → UnitMap.
    let sf_a = SpecFrame::new(SpecSystem::Freq);
    let sf_b = SpecFrame::new(SpecSystem::Freq);
    let fs = sf_a.convert_to(&sf_b, &[]).unwrap().unwrap();
    let v = transform_fwd(&fs, 42.0);
    assert_close(v, 42.0, 1.0e-12, "identity");
}

#[test]
fn test_unit_round_trip_mhz_to_ghz() {
    // 1 MHz = 1e-3 GHz.
    let mut sf_mhz = SpecFrame::new(SpecSystem::Freq);
    sf_mhz.set_unit("MHz");
    let mut sf_ghz = SpecFrame::new(SpecSystem::Freq);
    sf_ghz.set_unit("GHz");

    let fs = sf_mhz.convert_to(&sf_ghz, &[]).unwrap().unwrap();
    let v = transform_fwd(&fs, 1000.0);
    assert_close(v, 1.0, 1.0e-12, "1000 MHz = 1 GHz");
}

#[test]
fn test_bad_value_transform() {
    let sf_freq = SpecFrame::new(SpecSystem::Freq);
    let sf_wave = SpecFrame::new(SpecSystem::Wavelen);

    let fs = sf_freq.convert_to(&sf_wave, &[]).unwrap().unwrap();

    let pts = PointSet::from_rows([[BAD_VALUE]]);
    let out = fs.transform(&pts, true).unwrap();
    let v = out.point(0)[0];
    assert_eq!(v, BAD_VALUE, "BAD_VALUE should propagate");
}

// ===========================================================================
// SpecOrigin: offset spectral frames (oracle values from the reference C
// library; RestFreq 1.420405752 GHz, default VRAD units km/s)
// ===========================================================================

fn vrad_lsrk() -> SpecFrame {
    let mut sf = SpecFrame::new(SpecSystem::Vradio);
    sf.set_std_of_rest(StdOfRest::Lsrk);
    sf.set_rest_freq(1.420405752e9);
    sf
}

#[test]
fn convert_applies_spec_origins() {
    // Offset frame -> offset frame: value + origin1 - origin2.
    let mut f1 = vrad_lsrk();
    f1.set_spec_origin(10.0);
    let mut f2 = vrad_lsrk();
    f2.set_spec_origin(25.0);
    let fs = Frame::convert_to(&f1, &f2, &[])
        .unwrap()
        .expect("conversion");
    let pts = PointSet::from_columns(&[&[5.0_f64]]).unwrap();
    let out = Mapping::transform(&fs, &pts, true).unwrap();
    assert!(
        (out.point(0)[0] + 10.0).abs() < 1e-9,
        "got {}",
        out.point(0)[0]
    );
}

#[test]
fn convert_offset_to_absolute_applies_origin() {
    let mut f1 = vrad_lsrk();
    f1.set_spec_origin(10.0);
    let f2 = vrad_lsrk();
    let fs = Frame::convert_to(&f1, &f2, &[])
        .unwrap()
        .expect("conversion");
    let pts = PointSet::from_columns(&[&[5.0_f64]]).unwrap();
    let out = Mapping::transform(&fs, &pts, true).unwrap();
    assert!(
        (out.point(0)[0] - 15.0).abs() < 1e-9,
        "got {}",
        out.point(0)[0]
    );
}

#[test]
fn align_spec_offset_aligns_offset_values() {
    // With AlignSpecOffset set on both frames the offset values align
    // directly (fitschan.c... specframe.c:2947-2965 step 9).
    let mut f1 = vrad_lsrk();
    f1.set_spec_origin(10.0);
    f1.set_align_spec_offset(true);
    let mut f2 = vrad_lsrk();
    f2.set_spec_origin(25.0);
    f2.set_align_spec_offset(true);
    let fs = Frame::convert_to(&f1, &f2, &[])
        .unwrap()
        .expect("conversion");
    let pts = PointSet::from_columns(&[&[5.0_f64]]).unwrap();
    let out = Mapping::transform(&fs, &pts, true).unwrap();
    assert!(
        (out.point(0)[0] - 5.0).abs() < 1e-9,
        "got {}",
        out.point(0)[0]
    );
}

#[test]
fn set_system_rescales_spec_origin() {
    // Oracle: VRAD origin 10 km/s becomes 1.42035837236405 GHz when
    // the System changes to FREQ, and 10.0000000000761 km/s coming
    // back (round-trip drift through the relativistic formulae).
    let mut sf = vrad_lsrk();
    sf.set_spec_origin(10.0);
    sf.set_system(SpecSystem::Freq);
    let o = sf.spec_origin().unwrap();
    assert!(
        (o - 1.42035837236405).abs() < 1e-9,
        "VRAD->FREQ origin: got {o}"
    );
    sf.set_system(SpecSystem::Vradio);
    let o = sf.spec_origin().unwrap();
    assert!((o - 10.0).abs() < 1e-8, "FREQ->VRAD origin: got {o}");
}
