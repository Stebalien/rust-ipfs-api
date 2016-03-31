//! API for resolving IPFS/IPNS names.
use std::io;
use std::time::Duration;

use api;
use object::Reference;
use encoding::{Ignore, Json};

/// Resolve an IPFS path.
///
/// You probably don't ever need to use this function. Just use `lookup`.
pub fn resolve(path: &str, recursive: bool) -> io::Result<String> {
    #[derive(Deserialize)]
    struct ResolveResult {
        #[serde(rename="Path")]
        path: String,
    }

    let resp = api::get::<Json, ResolveResult>("resolve", &[("recursive", api::bool_to_str(recursive)), ("arg", path)])?;
    Ok(resp.path)
}

/// Publish the specified object at this peer's primary address for the default
/// duration (24h).
///
/// TODO: Better explain timeouts?
pub fn publish<R: AsRef<Reference>>(obj: &R) -> io::Result<()> {
    publish_for(obj, Duration::from_secs(60*60)*24)
}

/// Publish the specified object at this peer's primary address for the
/// specified duration.
pub fn publish_for<R: AsRef<Reference>>(obj: &R, expires_in: Duration) -> io::Result<()> {
    let time = format!("{}s{}ns", expires_in.as_secs(), expires_in.subsec_nanos());
    api::post::<Ignore, ()>("name/publish", &[
        ("resolve", "false"),
        ("lifetime", &time),
        ("arg", obj.as_ref().hash()),
    ])
}

// IPNS address.
// pub struct Identity(String);
//
// impl Identity {
//     pub fn publish<R: AsRef<Reference>>(&self, obj: &R) -> io::Result<()> {
//         // FIXME: Waiting for multiple keys.
//         publish(obj)
//     }
//
//     pub fn publish_for<R: AsRef<Reference>>(&self, obj: &R, expires_in: Duration) -> io::Result<()> {
//         // FIXME: Waiting for multiple keys.
//         publish_for(obj, expires_in)
//     }
// }
