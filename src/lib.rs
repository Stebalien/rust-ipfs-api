#![feature(custom_derive, plugin, question_mark, associated_consts)]
#![plugin(serde_macros)]
extern crate serde;
extern crate serde_json;
extern crate hyper;
extern crate protobuf;
extern crate url;
extern crate rust_base58 as base58;
extern crate multipart;

#[macro_use]
extern crate lazy_static;

#[allow(non_snake_case)]
mod merkledag;

use std::error::Error as StdError;
use protobuf::{MessageStatic, Message};
use std::fmt;
use std::io;
use base58::{ToBase58, FromBase58};
use std::ops::Deref;

mod api;

use api::{Json, Protobuf, Ignore};

pub use api::{set_api_endpoint, get_api_endpoint};

/// An IPFS object.
#[derive(Eq, PartialEq, Default, Debug, Clone)]
pub struct Object {
    pub data: Vec<u8>,
    pub links: Vec<Link>,
}

/// An IPFS object that has been committed.
#[derive(Debug, Clone)]
pub struct CommittedObject {
    reference: Reference,
    object: Object,
}

impl From<CommittedObject> for Reference {
    fn from(c: CommittedObject) -> Reference {
        c.reference
    }
}

impl Deref for CommittedObject {
    type Target = Object;
    fn deref(&self) -> &Object {
        &self.object
    }
}

impl Object {
    /// Calculate the (current) size of the object.
    pub fn size(&self) -> u64 {
        self.data.len() as u64 + self.links.iter().fold(0, |c, l| c + l.object.size)
    }

    /// Get a child object.
    ///
    /// Behavior:
    ///
    /// * This method returns the first link with the given name.
    /// * Except empty paths (you can't look up "").
    /// * Except links with forward slashes ('/') in them. That is, "a/b/c"
    ///   resolves to `self.links["a"].links["b"].links["c"]` (pseudocode).
    pub fn get(&self, path: &str) -> io::Result<CommittedObject> {
        if path == "" {
            // Don't resolve ""
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "cannot resolve empty path"));
        }
        if path.starts_with("/") {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "expected relative path"));
        }
        let mut splits = path.splitn(1, '/');
        let prefix = splits.next().unwrap();
        let suffix = splits.next();
        for link in &self.links {
            if link.name == prefix {
                return match suffix {
                    Some("")|None => get(&link.object.hash),
                    Some(suffix) => get(&format!("{}/{}", &link.object.hash, suffix))
                };
            }
        }
        Err(io::Error::new(io::ErrorKind::NotFound, "path lookup failed"))
    }
}

#[derive(Debug)]
pub struct CommitError {
    pub error: io::Error,
    pub object: Object,
}

impl fmt::Display for CommitError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        self.error.fmt(f)
    }
}

impl ::std::error::Error for CommitError {
    fn description(&self) -> &str {
        self.error.description()
    }
    fn cause(&self) -> Option<&::std::error::Error> {
        self.error.cause()
    }
}

impl From<CommitError> for io::Error {
    fn from(e: CommitError) -> io::Error {
        e.error
    }
}

impl Object {
    /// Create a new object.
    pub fn new() -> Object {
        Object {
            data: Vec::new(),
            links: Vec::new(),
        }
    }

    /// Commit this object to IPFS.
    pub fn commit(self) -> Result<CommittedObject, CommitError> {
        let mut node = merkledag::PBNode::new();
        node.set_Links(self.links
                           .iter()
                           .map(|l| {
                               let mut link = merkledag::PBLink::new();
                               link.set_Name(l.name.to_owned());
                               link.set_Hash(l.object.hash.from_base58().unwrap());
                               link.set_Tsize(l.object.size);
                               link
                           })
                           .collect());

        node.set_Data(self.data);

        #[derive(Deserialize, Debug)]
        struct PutResult {
            #[serde(rename="Hash")]
            hash: String,
        }

        // TODO: To unwrap or not to unwrap?
        let hash = match api::post_data::<Json, PutResult>("object/put",
                                                           &[("inputenc", "protobuf")],
                                                           &node.write_to_bytes().unwrap()[..]) {
            Ok(PutResult { hash, .. } ) => hash,
            Err(e) => {
                let data = node.take_Data();
                return Err(CommitError {
                    error: e,
                    object: Object {
                        links: self.links,
                        data: data,
                    },
                });
            }
        };

        let data = node.take_Data();
        let object = Object {
            links: self.links,
            data: data,
        };
        Ok(CommittedObject {
            reference: Reference {
                hash: hash,
                size: object.size(),
            },
            object: object,
        })
    }
}

fn bool_to_str(b: bool) -> &'static str {
    if b {
        "true"
    } else {
        "false"
    }
}

impl CommittedObject {
    /// Stat this object.
    ///
    /// Note: This method does not make any network calls.
    pub fn stat(&self) -> Stat {
        Stat {
            hash: self.hash().to_owned(),
            num_links: self.links.len() as u32,
            data_size: self.data.len() as u32,
            cumulative_size: self.size(),
            _non_exhaustive: (),
        }
    }

    /// Create a reference to this object.
    pub fn reference(&self) -> &Reference {
        &self.reference
    }

    /// Unpin this object.
    pub fn unpin(&self, recursive: bool) -> io::Result<()> {
        api::post::<Ignore, ()>("pin/rm", &[("recursive", bool_to_str(recursive)), ("arg", &self.reference.hash)])
            .or_else(|e| {
                if e.description() == api::ipfs_error::NOT_PINNED  {
                    // We consider this to be a success. That is, the object is
                    // no longer pinned.
                    return Ok(());
                }
                debug_assert!(e.description() != api::ipfs_error::INVALID_REF, "sent an invalid ref to the server");
                Err(e)
            })
    }

    /// Pin this object.
    pub fn pin(&self, recursive: bool) -> io::Result<()> {
        api::post::<Ignore, ()>("pin/add", &[("recursive", bool_to_str(recursive)), ("arg", &self.reference.hash)])
    }

    /// Get the IPFS multihash hash of the object.
    pub fn hash(&self) -> &str {
        &self.reference.hash
    }

    /// Get the (precomputed) size of the object
    pub fn size(&self) -> u64 {
        self.reference.size
    }

    /// Edit the object.
    pub fn edit(self) -> Object {
        self.object
    }
}

#[derive(Eq, PartialEq, Debug, Clone)]
pub struct Link {
    pub name: String,
    pub object: Reference,
}

/// A thin reference to an object.
#[derive(Eq, PartialEq, Debug, Clone)]
pub struct Reference {
    size: u64,
    hash: String,
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
}

/// Get an object.
pub fn get(path: &str) -> io::Result<CommittedObject> {
    let mut path = resolve(path, true)?;


    let mut value = api::get::<Protobuf, merkledag::PBNode>("object/get", &[("arg", &path)])?;

    let links: Vec<Link> = value.take_Links()
                                .into_iter()
                                .map(|mut l| {
                                    Link {
                                        name: l.take_Name(),
                                        object: Reference {
                                            hash: l.take_Hash().to_base58(),
                                            size: l.get_Tsize(),
                                        },
                                    }
                                })
                                .collect();

    let idx = path.rfind('/').unwrap();
    path.drain(..idx + 1);

    let object = Object {
        data: value.take_Data(),
        links: links,
    };
    Ok(CommittedObject {
        reference: Reference {
            size: object.size(),
            hash: path,
        },
        object: object,
    })
}

/// Resolve an IPFS path.
pub fn resolve(path: &str, recursive: bool) -> io::Result<String> {
    #[derive(Deserialize)]
    struct ResolveResult {
        #[serde(rename="Path")]
        path: String,
    }

    let resp = api::get::<Json, ResolveResult>("resolve", &[("recursive", bool_to_str(recursive)), ("arg", path)])?;
    Ok(resp.path)
}

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

/// Lookup object stats.
pub fn stat(path: &str) -> io::Result<Stat> {
    api::get::<Json, Stat>("object/stat", &[("arg", path)])
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

#[test]
fn main() {
    //println!("{:?}", get("/ipfs/Qme3UVucKczKbMwpx3HUR9cTej99YMMiGoNencRaKpGyk2/test\0basdf"))
}
