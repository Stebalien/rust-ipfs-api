use std::io;

use api;
use encoding::Json;
use object::CommittedObject;

/// Status of an IPFS object.
///
/// Returned from [stat](fn.stat.html).
#[derive(Deserialize)]
pub struct Stat {
    /// The object's hash.
    #[serde(rename="Hash")]
    pub hash: String,

    /// The number of links in the object.
    #[serde(rename="NumLinks")]
    pub num_links: u32,

    /// The size of the object's data field.
    #[serde(rename="DataSize")]
    pub data_size: u32,

    /// The total size of the object and it's children.
    #[serde(rename="CumulativeSize")]
    pub cumulative_size: u64,

    #[doc(hidden)]
    #[serde(default)]
    _non_exhaustive: (),
}

// Internal API. DO NOT EXPORT!
pub fn stat_object(obj: &CommittedObject) -> Stat {
    Stat {
        hash: obj.hash().to_owned(),
        num_links: obj.links.len() as u32,
        data_size: obj.data.len() as u32,
        cumulative_size: obj.size(),
        _non_exhaustive: (),
    }
}

/// Lookup information about an object.
///
/// This *will* cause the IPFS node to fetch the object but won't try to
/// materialize it (so it's faster than get, especially if the object hash been
/// cached).
pub fn stat(path: &str) -> io::Result<Stat> {
    api::get::<Json, Stat>("object/stat", &[("arg", path)])
}
