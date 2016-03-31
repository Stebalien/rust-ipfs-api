//! An IPFS API interface for Rust.
//!
//! To get an object, just call `get(object_name)` where `object_name` is an
//! object hash, ipfs path `/ipfs/$object_hash`, or ipns path `/ipns/$object_hash`.

#![feature(custom_derive, plugin, question_mark, associated_consts)]
#![plugin(serde_macros)]
#![deny(missing_docs)]

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

mod api;
mod encoding;
mod stat;
mod object;
mod resolve;

pub use api::{set_api_endpoint, get_api_endpoint};
pub use stat::{Stat, stat};
pub use object::{Object, CommitError, CommittedObject, Link, get};
pub use resolve::{Reference, resolve, lookup};
