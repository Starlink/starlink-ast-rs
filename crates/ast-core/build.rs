use std::env;
use std::fs;
use std::io::Write;
use std::path::PathBuf;

fn main() {
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    let vendor_dir = manifest_dir.join("../../vendor");

    compile_pal_erfa(&vendor_dir);
    compile_wcslib(&vendor_dir);
    generate_error_codes(&vendor_dir);
}

fn compile_pal_erfa(vendor_dir: &PathBuf) {
    println!("cargo::rerun-if-changed={}", vendor_dir.join("palwrap.c").display());

    cc::Build::new()
        .file(vendor_dir.join("palwrap.c"))
        .include(vendor_dir)
        .include(vendor_dir.join("pal"))
        .include(vendor_dir.join("erfa"))
        .warnings(false)
        .compile("ast_pal_erfa");
}

fn compile_wcslib(vendor_dir: &PathBuf) {
    let wcslib_dir = vendor_dir.join("wcslib");

    println!("cargo::rerun-if-changed={}", wcslib_dir.join("proj.c").display());
    println!("cargo::rerun-if-changed={}", wcslib_dir.join("tpn.c").display());
    println!("cargo::rerun-if-changed={}", wcslib_dir.join("wcstrig.c").display());
    println!("cargo::rerun-if-changed={}", wcslib_dir.join("ast_memory_stub.c").display());

    cc::Build::new()
        .file(wcslib_dir.join("proj.c"))
        .file(wcslib_dir.join("tpn.c"))
        .file(wcslib_dir.join("wcstrig.c"))
        .file(wcslib_dir.join("ast_memory_stub.c"))
        .include(&wcslib_dir)
        .warnings(false)
        .compile("ast_wcslib");
}

fn generate_error_codes(vendor_dir: &PathBuf) {
    let msg_path = vendor_dir.join("ast_err.msg");
    println!("cargo::rerun-if-changed={}", msg_path.display());

    let contents = fs::read_to_string(&msg_path)
        .unwrap_or_else(|e| panic!("Failed to read {}: {}", msg_path.display(), e));

    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());
    let out_path = out_dir.join("error_codes.rs");
    let mut out = fs::File::create(&out_path).unwrap();

    let mut facility_code: u64 = 0;
    let mut severity: u64 = 0;
    let mut message_number: u64 = 0;

    // Collect all error codes first
    let mut variants: Vec<(String, u64, String)> = Vec::new(); // (name, value, description)

    for line in contents.lines() {
        let line = line.trim();

        if line.is_empty() || line.starts_with('!') {
            continue;
        }

        if let Some(rest) = line.strip_prefix(".FACILITY") {
            // Parse: AST,1521/PREFIX=AST__
            let rest = rest.trim();
            if let Some(comma_pos) = rest.find(',') {
                let after_comma = &rest[comma_pos + 1..];
                if let Some(slash_pos) = after_comma.find('/') {
                    facility_code = after_comma[..slash_pos].parse().unwrap();
                }
            }
            continue;
        }

        if let Some(rest) = line.strip_prefix(".SEVERITY") {
            let sev_name = rest.trim();
            severity = match sev_name {
                "WARNING" => 0,
                "SUCCESS" => 1,
                "ERROR" => 2,
                "INFORMATIONAL" => 3,
                "SEVERE" => 4,
                _ => 0,
            };
            continue;
        }

        if let Some(rest) = line.strip_prefix(".BASE") {
            message_number = rest.trim().parse().unwrap();
            continue;
        }

        if line.starts_with(".TITLE") || line.starts_with(".END") {
            continue;
        }

        // Parse message definition: NAME <description>
        if let Some(angle_start) = line.find('<') {
            if let Some(angle_end) = line.find('>') {
                let name = line[..angle_start].trim();
                let description = &line[angle_start + 1..angle_end];

                // Starlink MESSGEN formula
                let errcode = ((facility_code | 0x800) << 16)
                    + ((message_number | 0x1000) << 3)
                    + severity;

                // Convert name to PascalCase: "ATGER" -> "Atger", "3DFSET" -> "ThreeDfset"
                let variant_name = to_pascal_case(name);

                variants.push((variant_name, errcode, description.to_string()));
                message_number += 1;
            }
        }
    }

    // Write the enum
    writeln!(out, "/// Error codes defined by the AST library.").unwrap();
    writeln!(out, "///").unwrap();
    writeln!(out, "/// Values match the Starlink MESSGEN formula for binary compatibility").unwrap();
    writeln!(out, "/// with the C library's `AST__*` error constants.").unwrap();
    writeln!(out, "#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]").unwrap();
    writeln!(out, "#[repr(u32)]").unwrap();
    writeln!(out, "#[non_exhaustive]").unwrap();
    writeln!(out, "pub enum ErrorCode {{").unwrap();
    for (name, value, desc) in &variants {
        writeln!(out, "    /// {desc}").unwrap();
        writeln!(out, "    {name} = {value},").unwrap();
    }
    writeln!(out, "}}").unwrap();
    writeln!(out).unwrap();

    // Write Display impl
    writeln!(out, "impl fmt::Display for ErrorCode {{").unwrap();
    writeln!(out, "    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {{").unwrap();
    writeln!(out, "        let desc = match self {{").unwrap();
    for (name, _, desc) in &variants {
        writeln!(out, "            ErrorCode::{name} => \"{desc}\",").unwrap();
    }
    writeln!(out, "        }};").unwrap();
    writeln!(out, "        write!(f, \"{{desc}}\")").unwrap();
    writeln!(out, "    }}").unwrap();
    writeln!(out, "}}").unwrap();
    writeln!(out).unwrap();

    // Write as_raw method
    writeln!(out, "impl ErrorCode {{").unwrap();
    writeln!(out, "    /// Return the raw numeric error code value.").unwrap();
    writeln!(out, "    pub fn as_raw(self) -> u32 {{").unwrap();
    writeln!(out, "        self as u32").unwrap();
    writeln!(out, "    }}").unwrap();
    writeln!(out).unwrap();

    // Write from_raw method
    writeln!(out, "    /// Convert a raw numeric value to an ErrorCode, if valid.").unwrap();
    writeln!(out, "    pub fn from_raw(value: u32) -> Option<Self> {{").unwrap();
    writeln!(out, "        match value {{").unwrap();
    for (name, value, _) in &variants {
        writeln!(out, "            {value} => Some(ErrorCode::{name}),").unwrap();
    }
    writeln!(out, "            _ => None,").unwrap();
    writeln!(out, "        }}").unwrap();
    writeln!(out, "    }}").unwrap();
    writeln!(out, "}}").unwrap();
}

/// Convert an AST error name to PascalCase.
///
/// Rules:
/// - All-uppercase names: capitalize first letter, lowercase rest ("ATGER" -> "Atger")
/// - Names starting with a digit: prefix with word ("3DFSET" -> "ThreeDfset")
fn to_pascal_case(name: &str) -> String {
    let name = if name.starts_with(|c: char| c.is_ascii_digit()) {
        let digit_word = match name.chars().next().unwrap() {
            '0' => "Zero",
            '1' => "One",
            '2' => "Two",
            '3' => "Three",
            '4' => "Four",
            '5' => "Five",
            '6' => "Six",
            '7' => "Seven",
            '8' => "Eight",
            '9' => "Nine",
            _ => unreachable!(),
        };
        format!("{}{}", digit_word, &name[1..])
    } else {
        name.to_string()
    };

    let mut result = String::new();
    let mut chars = name.chars();
    if let Some(first) = chars.next() {
        result.push(first.to_ascii_uppercase());
    }
    for c in chars {
        result.push(c.to_ascii_lowercase());
    }
    result
}
