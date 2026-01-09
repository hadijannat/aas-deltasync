//! # AAS-ΔSync CLI
//!
//! Command-line utilities for encoding, testing, and debugging.

use aas_deltasync_adapter_aas::{decode_id_base64url, encode_id_base64url};
use anyhow::{Context, Result};
use std::env;

fn main() -> Result<()> {
    let args: Vec<String> = env::args().collect();

    if args.len() < 2 {
        print_help();
        return Ok(());
    }

    match args[1].as_str() {
        "encode" => {
            if args.len() < 3 {
                eprintln!("Usage: aas-deltasync encode <identifier>");
                std::process::exit(1);
            }
            let id = &args[2];
            let encoded = encode_id_base64url(id);
            println!("{encoded}");
        }
        "decode" => {
            if args.len() < 3 {
                eprintln!("Usage: aas-deltasync decode <encoded>");
                std::process::exit(1);
            }
            let encoded = &args[2];
            let decoded = decode_id_base64url(encoded).context("Failed to decode")?;
            println!("{decoded}");
        }
        "help" | "--help" | "-h" => {
            print_help();
        }
        cmd => {
            eprintln!("Unknown command: {cmd}");
            print_help();
            std::process::exit(1);
        }
    }

    Ok(())
}

fn print_help() {
    println!(
        r#"AAS-ΔSync CLI

USAGE:
    aas-deltasync <COMMAND> [OPTIONS]

COMMANDS:
    encode <id>       Encode an AAS identifier to base64url (no padding)
    decode <encoded>  Decode a base64url-encoded identifier
    help              Show this help message

EXAMPLES:
    aas-deltasync encode "urn:example:aas:asset1"
    aas-deltasync decode "dXJuOmV4YW1wbGU6YWFzOmFzc2V0MQ"
"#
    );
}
