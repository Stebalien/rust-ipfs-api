//! API for resolving IPFS/IPNS names.
use std::io;

use api;
use encoding::Json;

/// Resolve an IPFS path.
pub fn resolve(path: &str, recursive: bool) -> io::Result<String> {
    #[derive(Deserialize)]
    struct ResolveResult {
        #[serde(rename="Path")]
        path: String,
    }

    let resp = api::get::<Json, ResolveResult>("resolve", &[("recursive", api::bool_to_str(recursive)), ("arg", path)])?;
    Ok(resp.path)
}

