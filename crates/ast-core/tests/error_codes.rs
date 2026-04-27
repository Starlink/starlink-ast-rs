use ast_core::error::ErrorCode;

#[test]
fn atger_has_correct_raw_value() {
    // MESSGEN formula: ((facility | 0x800) << 16) + ((msg_num | 0x1000) << 3) + severity
    // AST__ATGER: facility=1521, msg_num=300, severity=2 (ERROR)
    // Verified against cmake-generated ast_err.h: AST__ATGER = 233933154
    assert_eq!(ErrorCode::Atger.as_raw(), 233_933_154);
}

#[test]
fn from_raw_round_trips() {
    let code = ErrorCode::Atger;
    let raw = code.as_raw();
    assert_eq!(ErrorCode::from_raw(raw), Some(code));
}

#[test]
fn from_raw_returns_none_for_invalid() {
    assert_eq!(ErrorCode::from_raw(0), None);
    assert_eq!(ErrorCode::from_raw(u32::MAX), None);
}

#[test]
fn display_shows_description() {
    let desc = format!("{}", ErrorCode::Atger);
    assert_eq!(desc, "attribute getting error");
}

#[test]
fn error_code_3dfset_accessible() {
    // Verify that the "3DFSET" error code (starts with digit) is accessible
    // as "Threedfset" in Rust (to_pascal_case: "3" -> "Three" + "DFSET" lowercased)
    let _ = ErrorCode::Threedfset;
}
