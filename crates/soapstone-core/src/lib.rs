//! Soapstone core: load a WSDL with everything it imports and build SOAP sample requests.

mod loader;

pub use loader::{Document, LoadError, fetch_all, to_url};
