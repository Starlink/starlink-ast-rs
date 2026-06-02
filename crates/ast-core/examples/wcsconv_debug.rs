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
    let obj = if fixture.ends_with(".ast") {
        ast_core::from_native_string(&text).expect("native parse")
    } else {
        let mut fc = FitsChan::from_header(&text).unwrap();
        fc.guess_encoding();
        fc.read().unwrap().expect("input parses")
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

    let mut fc_out = if fixture.ends_with(".ast") {
        FitsChan::new()
    } else {
        FitsChan::from_header(&text).unwrap()
    };
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
                    if let Ok(d) = val.parse::<usize>() {
                        fc_out.set_fits_digits(d);
                    }
                }
                "fitsrounding" => {
                    if let Ok(d) = val.parse::<usize>() {
                        fc_out.set_fits_rounding(d);
                    }
                }
                _ => {}
            }
        }
    }
    fc_out.write(&obj).unwrap();
    println!("=== {} → {} ===", fixture, target_enc);
    println!("{}", fc_out.to_header());
}
