#![feature(custom_derive, plugin)]
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

mod ipfs_error {
    #[derive(Debug, Deserialize)]
    #[allow(non_snake_case)]
    pub struct Error {
        pub Message: String,
        pub Code: u32,
    }

    pub const NOT_PINNED: &'static str = "not pinned";
    pub const INVALID_REF: &'static str = "invalid ipfs ref path";
}

#[allow(non_snake_case)]
mod merkledag;

use protobuf::{MessageStatic, Message};
use std::{mem, fmt, iter};
use std::io::{self, Read};
use base58::{ToBase58, FromBase58};

mod api;

pub use api::{set_api_endpoint, get_api_endpoint};

pub enum Error {
    Http(hyper::Error),
    DecodeError(String),
}

#[derive(Debug, Clone)]
pub struct Object {
    size: u64,
    hash: String,
    data: Vec<u8>,
    links: Vec<Link>,
}

#[derive(Debug, Clone)]
pub struct ObjectEditor {
    pub data: Vec<u8>,
    pub links: Vec<LinkEditor>,
}

#[derive(Debug)]
pub struct CommitError {
    pub error: io::Error,
    pub editor: ObjectEditor,
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

impl ObjectEditor {
    pub fn new() -> ObjectEditor {
        ObjectEditor {
            data: Vec::new(),
            links: Vec::new(),
        }
    }
    pub fn commit(self) -> Result<Object, CommitError> {
        let mut new_links: Vec<Link> = Vec::with_capacity(self.links.len());
        let mut links_iter = self.links.into_iter();

        while let Some(link) = links_iter.next() {
            let link = match link.object.0 {
                ObjectRefInner::Link(size, hash) |
                ObjectRefInner::Object(Object { size, hash, .. }) => {
                    Link {
                        name: link.name,
                        size: size,
                        hash: hash,
                    }
                }
                ObjectRefInner::ObjectEditor(editor) => {
                    match editor.commit() {
                        Ok(object) => {
                            Link {
                                name: link.name,
                                size: object.size,
                                hash: object.hash,
                            }
                        }
                        Err(CommitError { error, editor }) => {
                            // TODO: Trace error!
                            // Roll back!
                            return Err(CommitError {
                                editor: ObjectEditor {
                                    data: self.data,
                                    links: new_links
                                        .into_iter()
                                        .map(From::from)
                                        .chain(iter::once(LinkEditor {
                                            name: link.name,
                                            object: ObjectRef(ObjectRefInner::ObjectEditor(editor)),
                                        }))
                                        .chain(links_iter).collect(),
                                },
                                error: error,
                            });
                        }
                    }
                }
            };
            new_links.push(link);
        }

        let size = new_links.iter().fold(0, |c, l| c + l.size) + self.data.len() as u64;

        let mut node = merkledag::PBNode::new();
        node.set_Links(new_links.iter()
                                .map(|l| {
                                    let mut link = merkledag::PBLink::new();
                                    link.set_Name(l.name.to_owned());
                                    link.set_Hash(l.hash.from_base58().unwrap());
                                    link.set_Tsize(l.size);
                                    link
                                })
                                .collect());

        node.set_Data(self.data);

        #[derive(Deserialize, Debug)]
        #[allow(non_snake_case)]
        struct PutResult {
            Hash: String,
        }

        // TODO: To unwrap or not to unwrap?
        let hash = match api::post_data("object/put",
                                        &[("inputenc", "protobuf"),
                                          ("encoding", "json"),
                                          ("stream-channels", "true")],
                                        &node.write_to_bytes().unwrap()[..])
                             .and_then(parse_json)
                             .map(|resp: PutResult| resp.Hash) {
            Ok(hash) => hash,
            Err(e) => {
                let data = node.take_Data();
                return Err(CommitError {
                    error: e,
                    editor: ObjectEditor {
                        links: new_links.into_iter().map(From::from).collect(),
                        data: data,
                    },
                });
            }
        };

        let data = node.take_Data();
        Ok(Object {
            size: size,
            hash: hash,
            links: new_links,
            data: data,
        })
    }
}

#[derive(Clone, Debug)]
pub struct ObjectRef(ObjectRefInner);

impl ObjectRef {
    pub fn edit(&mut self) -> io::Result<&mut ObjectEditor> {
        loop {
            let object = match self.0 {
                ObjectRefInner::Link(_, ref hash) => try!(get(hash)),
                ObjectRefInner::Object(ref mut o) => {
                    Object {
                        links: mem::replace(&mut o.links, Vec::new()),
                        data: mem::replace(&mut o.data, Vec::new()),
                        hash: mem::replace(&mut o.hash, String::new()),
                        size: o.size,
                    }
                }
                ObjectRefInner::ObjectEditor(ref mut editor) => return Ok(editor),
            };
            self.0 = ObjectRefInner::ObjectEditor(object.edit());
        }
    }
}

#[derive(Clone, Debug)]
pub enum ObjectRefInner {
    Link(u64, String),
    Object(Object),
    ObjectEditor(ObjectEditor),
}

impl From<Object> for ObjectRef {
    fn from(l: Object) -> Self {
        ObjectRef(ObjectRefInner::Object(l))
    }
}
impl From<ObjectEditor> for ObjectRef {
    fn from(l: ObjectEditor) -> Self {
        ObjectRef(ObjectRefInner::ObjectEditor(l))
    }
}

fn bool_to_str(b: bool) -> &'static str {
    if b {
        "true"
    } else {
        "false"
    }
}

impl Object {
    pub fn unpin(&self, recursive: bool) -> io::Result<()> {
        api::post("pin/rm",
                  &[("recursive", bool_to_str(recursive)), ("arg", &self.hash)])
            .and_then(|r| {
                if r.status.is_success() {
                    Ok(())
                } else {
                    let error: ipfs_error::Error = try!(parse_json(r));
                    match &*error.Message {
                        // We consider this to be a success. That is, the object is
                        // no longer pinned.
                        ipfs_error::NOT_PINNED => Ok(()),
                        _ => {
                            debug_assert!(error.Message != ipfs_error::INVALID_REF,
                                          "sent an invalid ref to the server");
                            Err(io::Error::new(io::ErrorKind::Other, error.Message))
                        }
                    }
                }
            })
    }

    pub fn pin(&self, recursive: bool) -> io::Result<()> {
        api::post("pin/add",
                  &[("recursive", bool_to_str(recursive)), ("arg", &self.hash)])
            .and_then(|r| {
                if r.status.is_success() {
                    Ok(())
                } else {
                    let error: ipfs_error::Error = try!(parse_json(r));
                    Err(io::Error::new(io::ErrorKind::Other, error.Message))
                }
            })
    }

    pub fn hash(&self) -> &str {
        &self.hash
    }
    pub fn data(&self) -> &[u8] {
        &self.data
    }
    pub fn links<'a>(&'a self) -> &[Link] {
        &self.links[..]
    }
    pub fn get(&self, name: &str) -> io::Result<Object> {
        if name == "" {
            // Don't resolve ""
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "cannot resolve empty path"));
        }
        // This way, we can get a/b/c in one go.
        get(&[&self.hash[..], name].join("/"))
    }
    pub fn edit(self) -> ObjectEditor {
        ObjectEditor {
            data: self.data,
            links: self.links.into_iter().map(From::from).collect(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct Link {
    pub name: String,
    pub size: u64,
    pub hash: String,
}

#[derive(Debug, Clone)]
pub struct LinkEditor {
    pub name: String,
    pub object: ObjectRef,
}

impl From<Link> for LinkEditor {
    fn from(link: Link) -> LinkEditor {
        LinkEditor {
            name: link.name,
            object: ObjectRef(ObjectRefInner::Link(link.size, link.hash)),
        }
    }
}

impl LinkEditor {
    pub fn new<N: Into<String>, O: Into<ObjectRef>>(name: N, object: O) -> Self {
        LinkEditor {
            name: name.into(),
            object: object.into(),
        }
    }
}

impl Link {
    pub fn get(&self) -> io::Result<Object> {
        get(&self.hash)
    }
}

fn parse_json<R: Read, O: serde::Deserialize>(mut r: R) -> io::Result<O> {
    use serde_json::error::Error::Io;
    serde_json::from_reader(&mut r).map_err(|e| {
        match e {
            Io(e) => io::Error::new(io::ErrorKind::InvalidData, e),
            e => io::Error::new(io::ErrorKind::InvalidData, e),
        }
    })
}

fn parse_proto<M: MessageStatic>(r: &mut Read) -> io::Result<M> {
    use protobuf::ProtobufError::*;
    protobuf::parse_from_reader::<M>(r).map_err(|e| {
        match e {
            IoError(e) => e,
            WireError(e) => io::Error::new(io::ErrorKind::InvalidData, e),
        }
    })
}

/// Get an object.
pub fn get(path: &str) -> io::Result<Object> {
    let mut path = try!(resolve(path, true));

    let mut value: merkledag::PBNode = try!(api::get("object/get",
                                                     &[("encoding", "protobuf"), ("arg", &path)])
                                                .and_then(|mut r| {
                                                    parse_proto(&mut r as &mut Read)
                                                }));
    let links: Vec<Link> = value.take_Links()
                                .into_iter()
                                .map(|mut l| {
                                    Link {
                                        name: l.take_Name(),
                                        hash: l.take_Hash().to_base58(),
                                        size: l.get_Tsize(),
                                    }
                                })
                                .collect();
    let data = value.take_Data();
    let size = links.iter().fold(0, |c, l| c + l.size) + data.len() as u64;
    let idx = path.rfind('/').unwrap();
    path.drain(..idx + 1);

    Ok(Object {
        hash: path,
        data: data,
        size: size,
        links: links,
    })
}

pub fn resolve(path: &str, recursive: bool) -> io::Result<String> {
    #[derive(Deserialize)]
    #[allow(non_snake_case)]
    struct Result {
        Path: String,
    }

    let resp = try!(api::get("resolve",
                             &[("encoding", "json"),
                               ("recursive", bool_to_str(recursive)),
                               ("arg", path)]));
    if resp.status.is_success() {
        let result: Result = try!(parse_json(resp));
        Ok(result.Path)
    } else {
        let result: ipfs_error::Error = try!(parse_json(resp));
        Err(io::Error::new(io::ErrorKind::Other, result.Message))
    }
}

#[test]
fn main() {
    // println!("{:?}",
    // get("QmYNy6HLNiacH4yT3RHbNspgoB5yVQapM3uFZK8DHATTX1").unwrap());
    let obj = get("QmTMqNJeTr38LqkqK842HV6oK6qGnohsGewUuGC44HrbyB").unwrap();
    obj.unpin(true).unwrap();
    // obj.data.extend_from_slice(b"testing");
    // obj.links.push(LinkEditor::new("test",
    // get("QmYiH9pxCCrtbiiwPtiiazfSCmvmx8zvaqyeS7WdrCoDjz").unwrap()));
    // obj.commit().unwrap();
    //
}
