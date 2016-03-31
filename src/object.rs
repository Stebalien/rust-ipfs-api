//! IPFS API for working with objects.
use std::ops::Deref;
use std::io;
use std::fmt;
use std::error::Error as StdError;

use base58::{ToBase58, FromBase58};
use protobuf::{MessageStatic, Message};

use merkledag;
use name::resolve;
use api;
use encoding::{Json, Protobuf, Ignore};

/// An IPFS object.
#[derive(Eq, PartialEq, Default, Debug, Clone)]
pub struct Object {
    /// The object's data.
    pub data: Vec<u8>,

    /// The object's links.
    pub links: Vec<Link>,
}

/// An IPFS object that has been committed.
///
/// Dereferences to an (immutable) [Object](struct.Object.html).
#[derive(Debug, Clone, Eq)]
pub struct CommittedObject {
    reference: Reference,
    object: Object,
}

impl PartialEq<CommittedObject> for CommittedObject {
    fn eq(&self, other: &Self) -> bool {
        self.reference == other.reference
    }
}

impl PartialEq<Object> for CommittedObject {
    fn eq(&self, other: &Object) -> bool {
        &self.object == other
    }
}

impl PartialEq<CommittedObject> for Object {
    fn eq(&self, other: &CommittedObject) -> bool {
        self == &other.object
    }
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
        self.data.len() as u64 + self.links.iter().fold(0, |c, l| c + l.object.size())
    }

    /// Get a child object.
    ///
    /// # Behavior
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
                    Some("")|None => get(link.object.hash()),
                    Some(suffix) => get(&format!("{}/{}", link.object.hash(), suffix))
                };
            }
        }
        Err(io::Error::new(io::ErrorKind::NotFound, "path lookup failed"))
    }
}

/// The error returned when an object fails to commit.
#[derive(Debug)]
pub struct CommitError {
    /// The error.
    pub error: io::Error,
    /// The object that failed to commit.
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
                               link.set_Hash(l.object.hash().from_base58().unwrap());
                               link.set_Tsize(l.object.size());
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
                size: object.size(),
                hash: hash,
            },
            object: object,
        })
    }
}

impl CommittedObject {

    /// Unpin this object.
    pub fn unpin(&self, recursive: bool) -> io::Result<()> {
        self.reference.unpin(recursive)
    }

    /// Pin this object.
    pub fn pin(&self, recursive: bool) -> io::Result<()> {
        self.reference.pin(recursive)
    }

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

    /// Get a reference to this object.
    pub fn reference(&self) -> &Reference {
        &self.reference
    }

    /// Get the IPFS multihash hash of the object.
    pub fn hash(&self) -> &str {
        self.reference.hash()
    }

    /// Get the (precomputed) size of the object
    pub fn size(&self) -> u64 {
        self.reference.size()
    }

    /// Edit the object.
    pub fn edit(self) -> Object {
        self.object
    }
}

/// An IPFS link. See [Object](struct.Object.html).
#[derive(Eq, PartialEq, Debug, Clone)]
pub struct Link {
    /// The link name.
    ///
    /// This can be arbitrary utf8 encoded text (but should be short).
    pub name: String,
    /// The object to which this link points.
    pub object: Reference,
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
                                            size: l.get_Tsize(),
                                            hash: l.take_Hash().to_base58(),
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

/// Lookup information about an object.
///
/// This *will* cause the IPFS node to fetch the object but won't try to
/// materialize it (so it's faster than get, especially if the object hash been
/// cached).
pub fn stat(path: &str) -> io::Result<Stat> {
    api::get::<Json, Stat>("object/stat", &[("arg", path)])
}

/// A thin reference to an object.
///
/// Dereferences to the object's hash.
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

    /// Get the hash of the referenced object.
    pub fn hash(&self) -> &str {
        &self.hash
    }

    /// Unpin this object.
    pub fn unpin(&self, recursive: bool) -> io::Result<()> {
        api::post::<Ignore, ()>("pin/rm", &[("recursive", api::bool_to_str(recursive)), ("arg", &self)])
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
        api::post::<Ignore, ()>("pin/add", &[("recursive", api::bool_to_str(recursive)), ("arg", &self)])
    }

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
