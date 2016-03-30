use std::fmt;
use std::io;
use std::ops::Deref;

use object::{get, CommittedObject};
use api;
use stat::stat;
use encoding::Json;

/// A thin reference to an object.
///
/// Dereferences to the object's hash.
#[derive(Eq, PartialEq, Debug, Clone)]
pub struct Reference {
    size: u64,
    hash: String,
}

/// Internal constructor. DO NOT EXPORT!
#[inline(always)]
pub fn new_reference(size: u64, hash: String) -> Reference {
    Reference {
        size: size,
        hash: hash,
    }
}

impl fmt::Display for Reference {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "/ipfs/{}", self.hash)
    }
}

impl Deref for Reference {
    type Target = str;

    #[inline]
    fn deref(&self) -> &str {
        &self.hash
    }
}

impl Reference {
    /// Get the referenced object.
    pub fn get(&self) -> io::Result<CommittedObject> {
        get(&self.hash).and_then(|v| if v.size() != self.size {
            Err(io::Error::new(io::ErrorKind::InvalidData,
                               "reference and referenced object sizes do not match"))
        } else {
            Ok(v)
        })
    }

    /// Get the size of the referenced object.
    pub fn size(&self) -> u64 {
        self.size
    }

    /// Get the hash of the referenced object.
    pub fn hash(&self) -> &str {
        &self.hash
    }
}

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

/// Get a reference to an object.
///
/// This is useful when you want to link to an object but don't want to materialize it.
///
/// Note: This will still download the object.
pub fn lookup(path: &str) -> io::Result<Reference> {
    let stats = stat(&path)?;
    Ok(Reference {
        hash: stats.hash,
        size: stats.cumulative_size,
    })
}
