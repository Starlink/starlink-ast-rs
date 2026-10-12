//! A FitsChan's dump and loader (fitschan.c `Dump` and `astLoadFitsChan`):
//! its attributes, then every card, used ones included, with its name, type,
//! flags, value and comment.
//!
//! C also dumps the FitsTables a FitsChan holds for the `-TAB` algorithm,
//! which is not yet ported, so a dump here never holds them and a `Tables`
//! item read from one is ignored.

use super::card::{CardFlags, CardType, FitsCard, FitsValue};
use super::{AltAxes, Encoding, FitsChan};
use crate::channel::base;
use crate::constants::BAD_VALUE;
use crate::error::{AstError, AstResult, ErrorCode};
use crate::serial::{Node, NodeValue, Serializable};

/// Write `value` under `key`: as an item when `set`, otherwise as a comment
/// holding the default, as C's `astWrite<X>` writes a helpful unset value.
fn put(node: &mut Node, key: &str, set: bool, value: NodeValue, comment: &str) {
    let comment = Some(comment.to_string());
    if !set {
        node.push_unset(key, value, comment);
        return;
    }
    match value {
        NodeValue::Int(v) => node.push_int(key, v, comment),
        NodeValue::Double(v) => node.push_double(key, v, comment),
        NodeValue::String(v) => node.push_string(key, &v, comment),
        NodeValue::Object(_) | NodeValue::Unset(_) => unreachable!("a FitsChan attribute"),
    }
}

/// Write an integer `value` under `key` when `set`, as C's `astWriteInt`
/// writes a value that is not helpful.
fn put_int_if_set(node: &mut Node, key: &str, set: bool, value: i64, comment: &str) {
    if set {
        node.push_int(key, value, Some(comment.to_string()));
    }
}

/// An unknown name read for `what`, which C's `FindString` reports.
fn illegal(name: &str, what: &str) -> AstError {
    AstError::new(
        ErrorCode::Rderr,
        format!("astRead(Channel): Illegal value '{name}' supplied for {what}."),
    )
}

/// The value of the floating point item `key`, or `None` when absent or
/// unset.
fn get_double_if_set(node: &Node, key: &str) -> Option<f64> {
    match node.get(key) {
        Some(NodeValue::Double(v)) => Some(*v),
        Some(NodeValue::Int(v)) => Some(*v as f64),
        _ => None,
    }
}

impl Serializable for FitsChan {
    fn class_name(&self) -> &'static str {
        "FitsChan"
    }

    /// fitschan.c `Dump`, after the Object and Channel parts. C reads the
    /// source first, as finding the current card does, so the dump is made
    /// from a copy, which reads it in the same way.
    fn to_node(&self) -> AstResult<Node> {
        let mut fc = self.clone();
        let mut node = Node::new("FitsChan");
        node.class_comment = Some("I/O channels to FITS files".to_string());
        base::write_parts(&mut node, fc.ident.as_deref(), &fc.config, false);

        let icard = fc.cards.get_card();
        node.push_int(
            "Card",
            icard as i64,
            Some("Index of current card".to_string()),
        );
        put(
            &mut node,
            "Encod",
            fc.test_encoding(),
            NodeValue::String(fc.encoding().name().to_string()),
            "Encoding system",
        );
        put(
            &mut node,
            "FAxOrd",
            fc.test_fits_axis_order(),
            NodeValue::String(fc.fits_axis_order().to_string()),
            "Order of WCS axes in new FITS headers",
        );
        put(
            &mut node,
            "FitsDg",
            fc.test_fits_digits(),
            NodeValue::Int(i64::from(fc.fits_digits())),
            "No. of digits for floating point values",
        );
        put(
            &mut node,
            "FitsRn",
            fc.test_fits_rounding(),
            NodeValue::Int(i64::from(fc.fits_rounding())),
            "No. of digits guarded from rounding",
        );
        put(
            &mut node,
            "AltAx",
            fc.test_alt_axes(),
            NodeValue::String(fc.alt_axes().name().to_string()),
            "Creation of alternate axes",
        );
        let b1950 = fc.def_b1950();
        put(
            &mut node,
            "DfB1950",
            fc.test_def_b1950(),
            NodeValue::Int(i64::from(b1950)),
            if b1950 {
                "Default to FK4 B1950"
            } else {
                "Default to ICRS"
            },
        );
        let tab_ok = fc.tab_ok();
        put(
            &mut node,
            "TabOK",
            fc.test_tab_ok(),
            NodeValue::Int(i64::from(tab_ok)),
            if tab_ok > 0 {
                "EXTVER value for -TAB headers"
            } else {
                "Do not support -TAB CTYPE codes"
            },
        );
        let force_tab = fc.force_tab();
        put(
            &mut node,
            "FrcTab",
            fc.test_force_tab(),
            NodeValue::Int(i64::from(force_tab)),
            if force_tab != 0 {
                "Force use of -TAB"
            } else {
                "Only use -TAB if necessary"
            },
        );
        let ignore = fc.ignore_bad_alt();
        put(
            &mut node,
            "IgBdAl",
            fc.test_ignore_bad_alt(),
            NodeValue::Int(i64::from(ignore)),
            if ignore {
                "Skip unreadable alternate axes"
            } else {
                "Abort on unreadable alternate axes"
            },
        );
        let cd = fc.cd_matrix();
        put(
            &mut node,
            "CdMat",
            fc.test_cd_matrix(),
            NodeValue::Int(i64::from(cd)),
            if cd { "Use CD Matrix" } else { "Use PC matrix" },
        );
        let car_lin = fc.car_lin();
        put(
            &mut node,
            "CarLin",
            fc.test_car_lin(),
            NodeValue::Int(i64::from(car_lin)),
            if car_lin {
                "Use simple linear CAR projections"
            } else {
                "Use full FITS-WCS CAR projections"
            },
        );
        put(
            &mut node,
            "SipRpl",
            fc.test_sip_replace(),
            NodeValue::Int(i64::from(fc.sip_replace())),
            "Replace SIP inverse coefficients?",
        );
        put(
            &mut node,
            "FitsTol",
            fc.test_fits_tol(),
            NodeValue::Double(fc.fits_tol()),
            "[pixel] Max allowed departure from linearity",
        );
        let poly_tan = fc.poly_tan();
        put_int_if_set(
            &mut node,
            "PolyTan",
            fc.test_poly_tan(),
            i64::from(poly_tan),
            if poly_tan != 0 {
                "Use distorted TAN convention"
            } else {
                "Use standard TAN convention"
            },
        );
        let sip_ok = fc.sip_ok();
        put_int_if_set(
            &mut node,
            "SipOK",
            fc.test_sip_ok(),
            i64::from(sip_ok),
            if sip_ok {
                "Use SIP distortion convention"
            } else {
                "Ignore SIP keywords"
            },
        );
        let iwc = fc.iwc();
        put(
            &mut node,
            "Iwc",
            fc.test_iwc(),
            NodeValue::Int(i64::from(iwc)),
            if iwc {
                "Include an IWC Frame"
            } else {
                "Do not include an IWC Frame"
            },
        );
        put_int_if_set(
            &mut node,
            "Clean",
            fc.test_clean(),
            i64::from(fc.clean()),
            "Always remove used cards?",
        );
        put(
            &mut node,
            "Warn",
            fc.test_warning_conditions(),
            NodeValue::String(fc.warning_conditions().to_string()),
            "Warnings to be reported",
        );

        for (i, card) in fc.cards.iter().enumerate() {
            write_card(&mut node, i + 1, card);
        }
        Ok(node)
    }

    /// fitschan.c `astLoadFitsChan`: no source or sink; the attributes
    /// through their setters, apart from FitsAxisOrder and Warnings, which C
    /// stores as read; then the cards, up to the first with no type.
    fn from_node(node: &Node) -> AstResult<Self> {
        let mut fc = FitsChan::new();
        fc.ident = base::read_parts(node, &mut fc.config);
        let part = node.section("FitsChan");

        if let Some(name) = part.get_string_if_set("Encod")
            && name != "UNKNOWN"
        {
            let encoding = Encoding::from_name(&name)
                .ok_or_else(|| illegal(&name, "the FitsChan component 'Encod'"))?;
            fc.set_encoding(encoding);
        }
        fc.fits_axis_order = part.get_string_if_set("FAxOrd");
        if let Some(name) = part.get_string_if_set("AltAx") {
            let alt_axes = AltAxes::from_name(&name)
                .ok_or_else(|| illegal(&name, "the FitsChan component 'AltAx'"))?;
            fc.set_alt_axes(alt_axes);
        }
        if let Some(v) = part.get_int_any_if_set("FitsDg") {
            fc.set_fits_digits(v as i32);
        }
        if let Some(v) = part.get_int_any_if_set("FitsRn") {
            fc.set_fits_rounding(v as i32);
        }
        if let Some(v) = part.get_int_any_if_set("DfB1950") {
            fc.set_def_b1950(v != 0);
        }
        if let Some(v) = part.get_int_any_if_set("TabOK") {
            fc.set_tab_ok(v as i32);
        }
        if let Some(v) = part.get_int_any_if_set("FrcTab") {
            fc.set_force_tab(v as i32);
        }
        if let Some(v) = part.get_int_any_if_set("IgBdAl") {
            fc.set_ignore_bad_alt(v != 0);
        }
        if let Some(v) = part.get_int_any_if_set("CdMat") {
            fc.set_cd_matrix(v != 0);
        }
        if let Some(v) = part.get_int_any_if_set("CarLin") {
            fc.set_car_lin(v != 0);
        }
        if let Some(v) = part.get_int_any_if_set("SipRpl") {
            fc.set_sip_replace(v != 0);
        }
        if let Some(v) = get_double_if_set(&part, "FitsTol") {
            fc.set_fits_tol(v);
        }
        if let Some(v) = part.get_int_any_if_set("PolyTan") {
            fc.set_poly_tan(v as i32);
        }
        if let Some(v) = part.get_int_any_if_set("SipOK") {
            fc.set_sip_ok(v != 0);
        }
        if let Some(v) = part.get_int_any_if_set("Iwc") {
            fc.set_iwc(v != 0);
        }
        if let Some(v) = part.get_int_any_if_set("Clean") {
            fc.set_clean(v != 0);
        }
        fc.warning_conditions = part.get_string_if_set("Warn");

        for n in 1.. {
            let name = part.get_string(&format!("Ty{n}"), " ");
            if name == " " {
                break;
            }
            let ty = CardType::from_dump_name(&name)
                .ok_or_else(|| illegal(&name, "a FitsChan keyword data type"))?;
            fc.cards.push(read_card(&part, n, ty));
        }
        fc.cards.set_card(part.get_int("Card", 0));
        Ok(fc)
    }
}

/// Write card `n` of a dump.
fn write_card(node: &mut Node, n: usize, card: &FitsCard) {
    let item = |name: &str| format!("{name}{n}");
    let value_comment = || Some("FITS keyword value".to_string());
    node.push_string(
        &item("Nm"),
        card.c_name(),
        Some("FITS keyword name".to_string()),
    );
    let ty = card.card_type();
    if let Some(name) = ty.dump_name() {
        node.push_string(
            &item("Ty"),
            name,
            Some("FITS keyword data type".to_string()),
        );
    }
    let flags = card.flags.c_bits();
    if flags != 0 {
        node.push_int(&item("Fl"), flags, Some("FITS keyword flags".to_string()));
    }
    match &card.value {
        FitsValue::Float(v) => node.push_double(&item("Dt"), *v, value_comment()),
        FitsValue::String(s) | FitsValue::Continue(s) => {
            node.push_string(&item("Dt"), s, value_comment());
        }
        FitsValue::Int(v) | FitsValue::KInt(v) => node.push_int(&item("Dt"), *v, value_comment()),
        FitsValue::Logical(v) => node.push_int(&item("Dt"), i64::from(*v), value_comment()),
        FitsValue::ComplexFloat(re, im) => {
            node.push_double(
                &item("Dr"),
                *re,
                Some("FITS keyword real value".to_string()),
            );
            node.push_double(
                &item("Di"),
                *im,
                Some("FITS keyword imaginary value".to_string()),
            );
        }
        FitsValue::ComplexInt(re, im) => {
            node.push_int(
                &item("Dr"),
                *re,
                Some("FITS keyword real value".to_string()),
            );
            node.push_int(
                &item("Di"),
                *im,
                Some("FITS keyword imaginary value".to_string()),
            );
        }
        FitsValue::Comment(_) | FitsValue::Undef => {}
    }
    if let Some(comment) = card.c_comment() {
        node.push_string(
            &item("Cm"),
            comment,
            Some("FITS keyword comment".to_string()),
        );
    }
}

/// Read card `n` of type `ty` from a dump, giving each missing item C's
/// default.
fn read_card(part: &Node, n: usize, ty: CardType) -> FitsCard {
    let item = |name: &str| format!("{name}{n}");
    let keyword = part.get_string(&item("Nm"), "        ");
    let comment = part.get_string_if_set(&item("Cm"));
    let value = match ty {
        CardType::Float => FitsValue::Float(part.get_double(&item("Dt"), BAD_VALUE)),
        CardType::String => FitsValue::String(part.get_string(&item("Dt"), "")),
        CardType::Continue => FitsValue::Continue(part.get_string(&item("Dt"), "")),
        CardType::Int => FitsValue::Int(part.get_int(&item("Dt"), 0)),
        CardType::KInt => FitsValue::KInt(part.get_int(&item("Dt"), 0)),
        CardType::Logical => FitsValue::Logical(part.get_int(&item("Dt"), 0) != 0),
        CardType::ComplexF => FitsValue::ComplexFloat(
            part.get_double(&item("Dr"), BAD_VALUE),
            part.get_double(&item("Di"), BAD_VALUE),
        ),
        CardType::ComplexI => {
            FitsValue::ComplexInt(part.get_int(&item("Dr"), 0), part.get_int(&item("Di"), 0))
        }
        CardType::Comment => FitsValue::Comment(comment.clone().unwrap_or_default()),
        CardType::Undef | CardType::NoType => FitsValue::Undef,
    };
    // Dumps made before AST V1.5 hold only the deletion flag, "Dl".
    let mut flags = part.get_int(&item("Fl"), 0);
    if flags == 0 {
        flags = part.get_int(&item("Dl"), 0);
    }
    FitsCard {
        keyword: keyword.trim_end().to_string(),
        comment: if ty == CardType::Comment {
            None
        } else {
            comment
        },
        value,
        used: false,
        flags: CardFlags::from_c_bits(flags),
    }
}
