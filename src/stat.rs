use std::io;

use api;
use encoding::Json;
use object::CommittedObject;

#[derive(Deserialize)]
pub struct Stat {
    #[serde(rename="Hash")]
    pub hash: String,
    #[serde(rename="NumLinks")]
    pub num_links: u32,
    #[serde(rename="DataSize")]
    pub data_size: u32,
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

/// Lookup object stats.
pub fn stat(path: &str) -> io::Result<Stat> {
    api::get::<Json, Stat>("object/stat", &[("arg", path)])
}
