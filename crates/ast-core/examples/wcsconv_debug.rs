//! Debug helper: load a wcsconv input, write it under the requested
//! encoding, print the resulting header.

use ast_core::AnyObject;
use ast_core::channel::Channel;
use ast_core::fitschan::{Encoding, FitsChan};

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let fixture = args.get(1).expect("input fixture path");
    let target_enc = args.get(2).map(|s| s.as_str()).unwrap_or("fits-wcs");
    // Optional comma-separated attrs (e.g. "cdmatrix=1,FitsDigits=8").
    let attrs = args.get(3).cloned().unwrap_or_default();

    let text = std::fs::read_to_string(fixture).unwrap();
    let mut fc_in_opt: Option<FitsChan> = None;
    let obj = if fixture.ends_with(".ast") {
        ast_core::from_native_string(&text).expect("native parse")
    } else {
        let mut fc = FitsChan::from_header(&text).unwrap();
        // Apply the read-side attributes before read, mirroring wcsconverter.c.
        for kv in attrs.split(',') {
            if let Some((k, v)) = kv.trim().split_once('=') {
                let v = v.trim();
                let on = v == "1" || v.eq_ignore_ascii_case("true");
                let k = k.trim();
                if k.eq_ignore_ascii_case("sipreplace") {
                    fc.set_sip_replace(on);
                } else if k.eq_ignore_ascii_case("carlin") {
                    fc.set_car_lin(on);
                }
            }
        }
        // Mirror C wcsconverter.c: leave the FitsChan with no explicit
        // encoding; the `read()` path will lazily auto-detect.
        let o = fc.read().unwrap().expect("input parses");
        fc_in_opt = Some(fc);
        o
    };

    if let AnyObject::FrameSet(ref fs) = obj {
        eprintln!("FrameSet: nframe={}", fs.nframe());
        for i in 1..=fs.nframe() {
            if let Ok(f) = fs.frame(ast_core::frames::FrameRef::Index(i)) {
                eprintln!(
                    "  Frame {}: domain={:?} ident={:?} sys={:?}",
                    i,
                    f.domain(),
                    f.ident(),
                    f.system_string()
                );
                if let Some(sky) = f.as_any().downcast_ref::<ast_core::frames::SkyFrame>() {
                    eprintln!(
                        "    SkyFrame: test_epoch={} epoch_mjd={} test_equinox={} equinox_mjd={} sky_ref_is={:?} perm={:?}",
                        sky.test_epoch(),
                        sky.epoch_mjd(),
                        sky.test_equinox(),
                        sky.equinox_mjd(),
                        sky.sky_ref_is(),
                        sky.axis_permutation(),
                    );
                }
                if let Some(cmp) = f.as_any().downcast_ref::<ast_core::frames::CmpFrame>() {
                    let dump_sub = |idx: usize, sub: &dyn ast_core::frames::Frame| {
                        eprintln!(
                            "    Sub {}: domain={:?} sys={:?} naxes={}",
                            idx,
                            sub.domain(),
                            sub.system_string(),
                            sub.naxes()
                        );
                        if let Some(sky) = sub.as_any().downcast_ref::<ast_core::frames::SkyFrame>()
                        {
                            eprintln!(
                                "      SkyFrame: test_epoch={} epoch_mjd={} test_equinox={} equinox_mjd={} perm={:?}",
                                sky.test_epoch(),
                                sky.epoch_mjd(),
                                sky.test_equinox(),
                                sky.equinox_mjd(),
                                sky.axis_permutation(),
                            );
                        }
                        if let Some(spec) =
                            sub.as_any().downcast_ref::<ast_core::frames::SpecFrame>()
                        {
                            eprintln!(
                                "      SpecFrame: test_epoch={} rest_freq={:?} sor={:?} svrf={:?} svel={:?}",
                                spec.test_epoch(),
                                spec.rest_freq(),
                                spec.std_of_rest(),
                                spec.source_vrf(),
                                spec.source_vel()
                            );
                        }
                    };
                    dump_sub(1, cmp.frame1());
                    dump_sub(2, cmp.frame2());
                }
            }
        }
    }

    let enc = match target_enc {
        "fits-pc" => Encoding::FitsPc,
        "fits-wcs" => Encoding::FitsWcs,
        "fits-iraf" => Encoding::FitsIraf,
        "fits-aips" => Encoding::FitsAips,
        "fits-aips++" => Encoding::FitsAipsPP,
        "fits-class" => Encoding::FitsClass,
        "fits-dss" | "dss" => Encoding::Dss,
        "native" | "ast" => Encoding::Native,
        other => panic!("unknown encoding {other}"),
    };

    let mut fc_out = fc_in_opt.unwrap_or_default();
    fc_out.set_encoding(enc);
    fc_out.set_fits_digits(8);
    for kv in attrs.split(',') {
        let kv = kv.trim();
        if kv.is_empty() {
            continue;
        }
        if let Some((k, v)) = kv.split_once('=') {
            let key = k.trim().to_lowercase();
            let val = v.trim();
            match key.as_str() {
                "cdmatrix" => fc_out.set_cd_matrix(val == "1" || val.eq_ignore_ascii_case("true")),
                "fitsdigits" => {
                    if let Ok(d) = val.parse::<i32>() {
                        fc_out.set_fits_digits(d);
                    }
                }
                "fitsrounding" => {
                    if let Ok(d) = val.parse::<i32>() {
                        fc_out.set_fits_rounding(d);
                    }
                }
                _ => {}
            }
        }
    }
    // Two NATIVE-family targets, mirroring wcsconverter.c's branch on
    // `strcasecmp(encoding, "AST")`: `ast` is an astChannel text dump, while
    // `native` is a FitsChan write under NATIVE encoding (the
    // COMMENT-banner + BEGAST/ENDAST keyword-card form).
    if target_enc == "ast" {
        println!("=== {} → {} ===", fixture, target_enc);
        obj.show().unwrap();
        return;
    }
    fc_out.write(&obj).unwrap();
    println!("=== {} → {} ===", fixture, target_enc);
    println!("{}", fc_out.to_header().unwrap());
}
