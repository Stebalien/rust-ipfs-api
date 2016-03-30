use std::ops::Deref;
use std::io;
use std::fmt;
use std::error::Error as StdError;

use base58::{ToBase58, FromBase58};
use protobuf::{MessageStatic, Message};

use resolve::{resolve, Reference, new_reference};
use merkledag;
use api;
use encoding::{Json, Protobuf, Ignore};
use stat;

/// An IPFS object.
#[derive(Eq, PartialEq, Default, Debug, Clone)]
pub struct Object {
    pub data: Vec<u8>,
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
            reference: new_reference(object.size(), hash),
            object: object,
        })
    }
}

impl CommittedObject {
    /// Stat this object.
    ///
    /// Note: This method does not make any network calls.
    pub fn stat(&self) -> stat::Stat {
        stat::stat_object(self)
    }

    /// Get a reference to this object.
    pub fn reference(&self) -> &Reference {
        &self.reference
    }

    /// Unpin this object.
    pub fn unpin(&self, recursive: bool) -> io::Result<()> {
        api::post::<Ignore, ()>("pin/rm", &[("recursive", api::bool_to_str(recursive)), ("arg", &self.reference)])
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
        api::post::<Ignore, ()>("pin/add", &[("recursive", api::bool_to_str(recursive)), ("arg", &self.reference)])
    }

    /// Get the IPFS multihash hash of the object.
    pub fn hash(&self) -> &str {
        &self.reference
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

#[derive(Eq, PartialEq, Debug, Clone)]
pub struct Link {
    pub name: String,
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
                                        object: new_reference(l.get_Tsize(), l.take_Hash().to_base58()),
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
        reference: new_reference(object.size(), path),
        object: object,
    })
}

