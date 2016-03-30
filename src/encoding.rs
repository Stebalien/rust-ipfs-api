use std::io::{self, Read};
use protobuf::{self, MessageStatic};
use serde;
use serde_json;

pub trait Encoding<T> {
    const ENCODING: Option<&'static str>;
    fn parse(reader: &mut Read) -> io::Result<T>;
}

pub struct Json;
pub struct Ignore;
pub struct Protobuf;

impl Encoding<()> for Ignore {
    const ENCODING: Option<&'static str> = None;
    fn parse(_: &mut Read) -> io::Result<()> {
        Ok(())
    }
}

impl<T: serde::Deserialize> Encoding<T> for Json {
    const ENCODING: Option<&'static str> = Some("json");
    fn parse(r: &mut Read) -> io::Result<T> {
        use serde_json::error::Error::Io;
        serde_json::from_reader(r).map_err(|e| {
            match e {
                Io(e) => io::Error::new(io::ErrorKind::InvalidData, e),
                e => io::Error::new(io::ErrorKind::InvalidData, e),
            }
        })
    }
}

impl<T: MessageStatic> Encoding<T> for Protobuf {
    const ENCODING: Option<&'static str> = Some("protobuf");

    fn parse(r: &mut Read) -> io::Result<T> {
        use protobuf::ProtobufError::*;
        protobuf::parse_from_reader::<T>(r).map_err(|e| {
            match e {
                IoError(e) => e,
                WireError(e) => io::Error::new(io::ErrorKind::InvalidData, e),
            }
        })
    }
}
