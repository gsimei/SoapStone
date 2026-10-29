//! Lists a WSDL's SOAP operations, or prints the sample request for one of them.
//!
//! Usage: soapstone-sample <wsdl-url-or-path> [--op <Binding>/<Operation>] [--time]

use std::process::ExitCode;
use std::time::Instant;

use soapstone_core::{Definition, LoadError, SoapVersion, Style, fetch_all, to_url};

const USAGE: &str = "usage: soapstone-sample <wsdl-url-or-path> [--op <Binding>/<Operation>] [--time]";

fn main() -> ExitCode {
    let mut input = None;
    let mut op = None;
    let mut time = false;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--op" => op = args.next(),
            "--time" => time = true,
            _ => input = Some(arg),
        }
    }
    let Some(input) = input else {
        eprintln!("{USAGE}");
        return ExitCode::from(2);
    };
    run(&input, op.as_deref(), time).unwrap_or_else(|e| {
        eprintln!("error: {e}");
        ExitCode::FAILURE
    })
}

fn ms(since: Instant) -> String {
    format!("{:.1} ms", since.elapsed().as_secs_f64() * 1000.0)
}

fn run(input: &str, op: Option<&str>, time: bool) -> Result<ExitCode, LoadError> {
    let started = Instant::now();
    let documents = fetch_all(&to_url(input)?)?;
    if time {
        eprintln!("fetch: {} ({} documents)", ms(started), documents.len());
    }
    let started = Instant::now();
    let def = Definition::parse(documents)?;
    if time {
        eprintln!("parse: {}", ms(started));
        let slowest = def
            .operations()
            .iter()
            .map(|o| {
                let started = Instant::now();
                def.sample_request(o);
                (started.elapsed(), o)
            })
            .max_by_key(|(elapsed, _)| *elapsed);
        if let Some((elapsed, o)) = slowest {
            eprintln!("slowest sample: {:.2} ms ({}/{})", elapsed.as_secs_f64() * 1000.0, o.binding, o.name);
        }
    }

    match op {
        Some(id) => match id.split_once('/').and_then(|(binding, name)| def.operation(binding, name)) {
            Some(o) => println!("{}", def.sample_request(o)),
            None => {
                eprintln!("operation {id} not found (expected <Binding>/<Operation>)");
                return Ok(ExitCode::FAILURE);
            }
        },
        None => {
            for o in def.operations() {
                let version = if o.soap_version == SoapVersion::V12 { "1.2" } else { "1.1" };
                let style = if o.style == Style::Rpc { "rpc" } else { "document" };
                let usage = if o.encoded { "encoded" } else { "literal" };
                let action = o.soap_action.as_deref().unwrap_or("-");
                let endpoint = o.endpoint.as_deref().unwrap_or("-");
                println!("{}/{}  SOAP {version}  {style}/{usage}  action={action}  endpoint={endpoint}", o.binding, o.name);
            }
        }
    }
    Ok(ExitCode::SUCCESS)
}
