//! Soapstone core: load a WSDL with everything it imports and build SOAP sample requests.

mod envelope;
mod loader;
mod qname;
mod sample;
mod wsdl;
mod xsd;

pub use loader::{Document, LoadError, fetch_all, to_url};
pub use wsdl::{Definition, Operation, SoapVersion, Style};
